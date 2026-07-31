use std::collections::BTreeMap;
use std::error::Error;
use std::fmt;
use std::path::{Path, PathBuf};

use ::config::{File, FileFormat};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

mod validate;

use validate::validate;

pub const CONFIG_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(title = "Containerless configuration")]
/// A versioned collection of OCI images and reusable layers to build.
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
        let source = match path.extension() {
            Some(_) => File::from(path).format(format_for_path(path)?),
            None => File::with_name(&path.to_string_lossy()),
        };
        let parsed = ::config::Config::builder()
            .add_source(source)
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
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigFormat {
    Toml,
    Json,
    Yaml,
}

impl From<ConfigFormat> for FileFormat {
    fn from(value: ConfigFormat) -> Self {
        match value {
            ConfigFormat::Toml => FileFormat::Toml,
            ConfigFormat::Json => FileFormat::Json,
            ConfigFormat::Yaml => FileFormat::Yaml,
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
                "unsupported config format for {}; expected .toml, .json, .yaml, or .yml",
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
        Some("json") => Ok(FileFormat::Json),
        Some("yaml" | "yml") => Ok(FileFormat::Yaml),
        _ => Err(ConfigError::UnsupportedFormat(path.to_owned())),
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    #[test]
    fn self_packaging_config_is_valid() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../containerless");
        Config::load(&path).unwrap();
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
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../containerless.schema.json");
        fs::write(path, format!("{json}\n")).unwrap();
    }
}
