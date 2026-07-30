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
pub struct Config {
    pub containerless: u32,

    #[serde(default)]
    pub layers: BTreeMap<String, Layer>,

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
pub struct Layer {
    #[serde(default)]
    pub files: Vec<FileMapping>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Image {
    #[serde(default = "default_base")]
    pub base: Base,

    #[serde(default)]
    pub layers: Vec<String>,

    #[serde(default)]
    pub files: Vec<FileMapping>,

    #[serde(default)]
    pub entrypoint: Option<Vec<String>>,
    #[serde(default)]
    pub command: Option<Vec<String>>,
    #[serde(default)]
    pub user: Option<String>,
    #[serde(default)]
    pub workdir: Option<String>,
    #[serde(default)]
    pub stop_signal: Option<String>,
    #[serde(default)]
    pub ports: Option<Vec<String>>,
    #[serde(default)]
    pub volumes: Option<Vec<String>>,

    #[serde(default)]
    pub env: BTreeMap<String, String>,
    #[serde(default)]
    pub labels: BTreeMap<String, String>,
    #[serde(default)]
    pub annotations: BTreeMap<String, String>,

    /// `None` inherits repositories from a configured-image base; an empty list clears them.
    #[serde(default)]
    pub repositories: Option<Vec<String>>,
    #[serde(default)]
    pub tags: Vec<String>,

    #[serde(default)]
    pub squash: bool,
    #[serde(default)]
    pub flatten: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum Base {
    External(String),
    Local { image: String },
}

fn default_base() -> Base {
    Base::External("scratch".to_owned())
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum FileMapping {
    Shorthand(String),
    Detailed(FileOptions),
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FileOptions {
    #[serde(rename = "from")]
    pub source: PlatformValue<PathBuf>,
    pub to: String,

    #[serde(default)]
    pub include: Vec<String>,
    #[serde(default)]
    pub exclude: Vec<String>,
    #[serde(default)]
    pub mode: Option<Mode>,
    #[serde(default)]
    pub owner: Option<String>,
    #[serde(default)]
    pub follow_symlinks: bool,
    #[serde(default)]
    pub parents: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum PlatformValue<T> {
    Scalar(T),
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
pub enum Mode {
    String(String),
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
        validate_files(name, &image.files)?;
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
        let destination = match file {
            FileMapping::Shorthand(mapping) => mapping
                .split_once(':')
                .filter(|(source, destination)| !source.is_empty() && !destination.is_empty())
                .map(|(_, destination)| destination)
                .ok_or_else(|| {
                    ConfigError::Invalid(format!(
                        "invalid file mapping {mapping:?} in {owner:?}; expected SOURCE:DESTINATION"
                    ))
                })?,
            FileMapping::Detailed(options) => {
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
                &options.to
            }
        };
        if !destination.starts_with('/') {
            return Err(ConfigError::Invalid(format!(
                "file destination {destination:?} in {owner:?} must be absolute"
            )));
        }
    }
    Ok(())
}
