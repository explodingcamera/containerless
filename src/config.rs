use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fmt;
use std::path::{Path, PathBuf};

use ::config::{File, FileFormat};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub const CONFIG_VERSION: u32 = 1;
pub const DEFAULT_CONFIG_FILES: [&str; 2] = ["containerless.toml", "containerless.json"];

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(title = "Containerless configuration")]
/// A versioned collection of reusable layers and images to package.
pub struct Config {
    /// Containerless configuration format version. The only supported value is `1`.
    #[schemars(range(min = 1, max = 1))]
    pub containerless: u32,

    /// Reusable filesystem layers keyed by layer name.
    #[serde(default)]
    pub layers: BTreeMap<String, Layer>,

    /// Images keyed by the name used on the command line and by local image bases.
    #[serde(default)]
    pub images: BTreeMap<String, Image>,
}

impl Config {
    pub fn load(path: &Path) -> Result<Self, ConfigError> {
        let format = format_for_path(path)?;
        let parsed = ::config::Config::builder()
            .add_source(File::from(path).format(format))
            .build()?
            .try_deserialize()?;
        validate(parsed)
    }

    pub fn parse(source: &str, format: ConfigFormat) -> Result<Self, ConfigError> {
        let format: FileFormat = format.into();
        let parsed = ::config::Config::builder()
            .add_source(File::from_str(source, format))
            .build()?
            .try_deserialize()?;
        validate(parsed)
    }

    /// Finds the default config file and rejects ambiguous directories.
    pub fn discover(directory: &Path) -> Result<Option<PathBuf>, ConfigError> {
        let matches = DEFAULT_CONFIG_FILES
            .iter()
            .map(|name| directory.join(name))
            .filter(|path| path.is_file())
            .collect::<Vec<_>>();

        match matches.as_slice() {
            [] => Ok(None),
            [path] => Ok(Some(path.clone())),
            _ => Err(ConfigError::Invalid(format!(
                "multiple config files found: {}",
                matches
                    .iter()
                    .map(|path| path.display().to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            ))),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigFormat {
    Toml,
    Json,
}

impl From<ConfigFormat> for FileFormat {
    fn from(value: ConfigFormat) -> Self {
        match value {
            ConfigFormat::Toml => FileFormat::Toml,
            ConfigFormat::Json => FileFormat::Json5,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
/// A reusable, ordered collection of local files that produces one OCI layer.
pub struct Layer {
    /// Files and directories to include in this layer.
    #[serde(default)]
    pub files: Vec<FileMapping>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
/// An OCI image assembled from a base, named layers, local files, and runtime metadata.
pub struct Image {
    /// External OCI image reference or configured image to use as the base. Defaults to `scratch`.
    #[serde(default = "default_base")]
    pub base: Base,

    /// Named layers to append in this order after all base-image layers.
    #[serde(default)]
    pub layers: Vec<String>,

    /// Files placed in one implicit final layer after all named layers. Accepts either an array of
    /// mappings or one detailed mapping object.
    #[serde(default)]
    pub files: ImageFiles,

    /// Executable and fixed arguments used when the container starts.
    #[serde(default)]
    pub entrypoint: Option<Vec<String>>,
    /// Default arguments passed to the entrypoint.
    #[serde(default)]
    pub command: Option<Vec<String>>,
    /// User and optional group used to run the container, such as `65532:65532`.
    #[serde(default)]
    pub user: Option<String>,
    /// Working directory used when the container starts.
    #[serde(default)]
    pub workdir: Option<String>,
    /// Signal used to stop the container, such as `SIGTERM`.
    #[serde(default)]
    pub stop_signal: Option<String>,
    /// Exposed ports in `PORT/PROTOCOL` form, such as `8080/tcp`.
    #[serde(default)]
    pub ports: Option<Vec<String>>,
    /// Container paths intended to hold externally mounted volumes.
    #[serde(default)]
    pub volumes: Option<Vec<String>>,

    /// Environment variables added to the image runtime configuration.
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    /// Docker-compatible image labels added to the runtime configuration.
    #[serde(default)]
    pub labels: BTreeMap<String, String>,
    /// OCI annotations added to the image manifest or index.
    #[serde(default)]
    pub annotations: BTreeMap<String, String>,

    /// Complete image references used when publishing, such as `ghcr.io/example/app:v1`.
    #[serde(default)]
    pub tags: Vec<String>,

    /// Combine all layers added by Containerless into one new layer while preserving base layers.
    #[serde(default)]
    pub squash: bool,
    /// Apply the base and added layers, then emit the complete filesystem as one layer.
    #[serde(default)]
    pub flatten: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
/// The external or locally configured image used as an image's base.
pub enum Base {
    /// An OCI image reference, or `scratch` for an empty base.
    External(String),
    /// Another image declared in this configuration.
    Local { image: String },
}

fn default_base() -> Base {
    Base::External("scratch".to_owned())
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
/// A local file mapping in shorthand `SOURCE:DESTINATION` or detailed object form.
pub enum FileMapping {
    /// A `SOURCE:DESTINATION` mapping with default behavior.
    Shorthand(String),
    /// A mapping with explicit source, destination, filtering, and archive metadata.
    Detailed(FileOptions),
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
/// One detailed image file mapping or an array of shorthand and detailed mappings.
pub enum ImageFiles {
    /// One detailed file mapping.
    Single(FileOptions),
    /// An ordered list of file mappings.
    Multiple(Vec<FileMapping>),
}

impl Default for ImageFiles {
    fn default() -> Self {
        Self::Multiple(Vec::new())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
/// Detailed options for copying a local file or directory into an image layer.
pub struct FileOptions {
    /// Local source path, optionally selected by OCI platform.
    #[serde(rename = "from")]
    pub source: PlatformValue<PathBuf>,
    /// Absolute destination path in the image.
    pub to: String,

    /// Git-style glob patterns to include, evaluated relative to the source directory.
    #[serde(default)]
    pub include: Vec<String>,
    /// Git-style glob patterns to exclude after inclusion.
    #[serde(default)]
    pub exclude: Vec<String>,
    /// Portable file mode, preferably an octal string such as `0755`.
    #[serde(default)]
    pub mode: Option<Mode>,
    /// Numeric owner in `UID` or `UID:GID` form.
    #[serde(default)]
    pub owner: Option<String>,
    /// Follow symlink targets instead of archiving symlinks. Defaults to `false`.
    #[serde(default)]
    pub follow_symlinks: bool,
    /// Preserve leading source path components similarly to Docker COPY `--parents`.
    #[serde(default)]
    pub parents: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
/// A value shared by every platform or selected from a map keyed by OCI platform.
pub enum PlatformValue<T> {
    /// A value used for every output platform.
    Scalar(T),
    /// Values keyed by OCI platform, with an optional `default` fallback.
    Platforms(BTreeMap<String, T>),
}

impl<T> PlatformValue<T> {
    pub fn resolve(&self, platform: &str) -> Option<&T> {
        match self {
            Self::Scalar(value) => Some(value),
            Self::Platforms(values) => values.get(platform).or_else(|| values.get("default")),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
/// A Unix permission mode represented as a portable octal string or an integer.
pub enum Mode {
    /// Octal mode string, such as `0755`.
    String(String),
    /// Numeric mode accepted by formats that preserve integer representation.
    Integer(u32),
}

#[derive(Debug)]
pub enum ConfigError {
    Parse(::config::ConfigError),
    UnsupportedFormat(PathBuf),
    Invalid(String),
}

impl fmt::Display for ConfigError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Parse(error) => error.fmt(formatter),
            Self::UnsupportedFormat(path) => write!(
                formatter,
                "unsupported config format for {}; expected .toml or .json",
                path.display()
            ),
            Self::Invalid(message) => formatter.write_str(message),
        }
    }
}

impl Error for ConfigError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Parse(error) => Some(error),
            Self::UnsupportedFormat(_) | Self::Invalid(_) => None,
        }
    }
}

impl From<::config::ConfigError> for ConfigError {
    fn from(value: ::config::ConfigError) -> Self {
        Self::Parse(value)
    }
}

fn format_for_path(path: &Path) -> Result<FileFormat, ConfigError> {
    match path.extension().and_then(|extension| extension.to_str()) {
        Some("toml") => Ok(FileFormat::Toml),
        Some("json") => Ok(FileFormat::Json5),
        _ => Err(ConfigError::UnsupportedFormat(path.to_owned())),
    }
}

fn validate(config: Config) -> Result<Config, ConfigError> {
    if config.containerless != CONFIG_VERSION {
        return Err(ConfigError::Invalid(format!(
            "unsupported containerless version {}; expected {CONFIG_VERSION}",
            config.containerless
        )));
    }
    if config.images.is_empty() {
        return Err(ConfigError::Invalid(
            "config must define at least one image".to_owned(),
        ));
    }

    for (name, image) in &config.images {
        if image.squash && image.flatten {
            return Err(ConfigError::Invalid(format!(
                "image {name:?} cannot enable both squash and flatten"
            )));
        }
        for layer in &image.layers {
            if !config.layers.contains_key(layer) {
                return Err(ConfigError::Invalid(format!(
                    "image {name:?} references unknown layer {layer:?}"
                )));
            }
        }
        if let Base::Local { image: base } = &image.base
            && !config.images.contains_key(base)
        {
            return Err(ConfigError::Invalid(format!(
                "image {name:?} references unknown base image {base:?}"
            )));
        }
        validate_image_files(name, &image.files)?;
    }
    for (name, layer) in &config.layers {
        validate_files(name, &layer.files)?;
    }

    let mut complete = BTreeSet::new();
    for name in config.images.keys() {
        validate_base_cycle(name, &config.images, &mut BTreeSet::new(), &mut complete)?;
    }
    Ok(config)
}

fn validate_base_cycle(
    name: &str,
    images: &BTreeMap<String, Image>,
    visiting: &mut BTreeSet<String>,
    complete: &mut BTreeSet<String>,
) -> Result<(), ConfigError> {
    if complete.contains(name) {
        return Ok(());
    }
    if !visiting.insert(name.to_owned()) {
        return Err(ConfigError::Invalid(format!(
            "configured-image base cycle includes {name:?}"
        )));
    }
    if let Base::Local { image: base } = &images[name].base {
        validate_base_cycle(base, images, visiting, complete)?;
    }
    visiting.remove(name);
    complete.insert(name.to_owned());
    Ok(())
}

fn validate_files(owner: &str, files: &[FileMapping]) -> Result<(), ConfigError> {
    for file in files {
        match file {
            FileMapping::Shorthand(mapping) => {
                let destination = mapping
                .split_once(':')
                .filter(|(source, destination)| !source.is_empty() && !destination.is_empty())
                .map(|(_, destination)| destination)
                .ok_or_else(|| {
                    ConfigError::Invalid(format!(
                        "invalid file mapping {mapping:?} in {owner:?}; expected SOURCE:DESTINATION"
                    ))
                })?;
                validate_destination(owner, destination)?;
            }
            FileMapping::Detailed(options) => validate_file_options(owner, options)?,
        }
    }
    Ok(())
}

fn validate_image_files(image: &str, files: &ImageFiles) -> Result<(), ConfigError> {
    match files {
        ImageFiles::Single(options) => validate_file_options(image, options),
        ImageFiles::Multiple(files) => validate_files(image, files),
    }
}

fn validate_file_options(owner: &str, options: &FileOptions) -> Result<(), ConfigError> {
    if let PlatformValue::Platforms(values) = &options.source {
        if values.is_empty() {
            return Err(ConfigError::Invalid(format!(
                "file source platform map in {owner:?} cannot be empty"
            )));
        }
        if let Some(platform) = values
            .keys()
            .find(|platform| platform.as_str() != "default" && !platform.contains('/'))
        {
            return Err(ConfigError::Invalid(format!(
                "invalid OCI platform {platform:?} in {owner:?}"
            )));
        }
    }
    validate_destination(owner, &options.to)
}

fn validate_destination(owner: &str, destination: &str) -> Result<(), ConfigError> {
    if !destination.starts_with('/') {
        return Err(ConfigError::Invalid(format!(
            "file destination {destination:?} in {owner:?} must be absolute"
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    #[test]
    fn self_packaging_config_is_valid() {
        Config::parse(include_str!("../containerless.toml"), ConfigFormat::Toml).unwrap();
    }

    #[test]
    #[ignore = "writes containerless.schema.json"]
    fn generate_config_schema() {
        let schema = schemars::schema_for!(Config);
        let mut schema = serde_json::to_value(schema).unwrap();
        schema.as_object_mut().unwrap().insert(
            "x-tombi-toml-version".to_owned(),
            serde_json::json!("v1.1.0"),
        );
        let json = serde_json::to_string_pretty(&schema).unwrap();
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("containerless.schema.json");
        fs::write(path, format!("{json}\n")).unwrap();
    }
}
