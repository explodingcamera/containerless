use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use ::config::{File, FileFormat};
use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize, de};
use thiserror::Error;

mod validate;

use validate::validate;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(title = "Containerless configuration")]
/// A collection of OCI images and reusable layers to build.
pub struct Config {
    /// Reusable filesystem layers keyed by layer name.
    #[serde(default)]
    pub layers: BTreeMap<String, Layer>,

    /// Images keyed by the name used on the command line and by local image bases.
    #[serde(default)]
    pub images: BTreeMap<String, Image>,
}

impl Config {
    pub fn load(path: &Path) -> Result<Self, Error> {
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

    pub fn parse(source: &str, format: Format) -> Result<Self, Error> {
        let format: FileFormat = format.into();
        let parsed = ::config::Config::builder()
            .add_source(File::from_str(source, format))
            .build()?
            .try_deserialize()?;
        validate(parsed)
    }

    /// Returns the platforms configured or inferred for an image.
    ///
    /// Explicit image platforms take precedence. Otherwise, this method collects platforms from
    /// the image and its configured base chain. Images without platform-specific sources target
    /// Linux on the host CPU architecture.
    pub fn platforms_for_image(&self, image_name: &str) -> Option<Vec<String>> {
        let image = self.images.get(image_name)?;
        if !image.platforms.is_empty() {
            return Some(image.platforms.clone());
        }

        let mut chain = vec![image_name];
        while let Base::Local { image: parent } = &self.images.get(*chain.last()?)?.base {
            if chain.contains(&parent.as_str()) {
                return None;
            }
            chain.push(parent);
        }
        let mut platforms = BTreeMap::new();
        for image_name in &chain {
            let image = self.images.get(*image_name)?;
            collect_file_platforms(&image.files, &mut platforms);
            for layer_name in &image.layers {
                if let Some(layer) = self.layers.get(layer_name) {
                    collect_file_platforms(&layer.files, &mut platforms);
                }
            }
        }

        if platforms.is_empty() {
            Some(
                chain
                    .iter()
                    .find_map(|image_name| {
                        let platforms = &self.images[*image_name].platforms;
                        (!platforms.is_empty()).then(|| platforms.clone())
                    })
                    .unwrap_or_else(|| {
                        vec![containerless_core::Platform::linux_for_host().to_string()]
                    }),
            )
        } else {
            Some(platforms.into_keys().collect())
        }
    }
}

fn collect_file_platforms(files: &[FileMapping], platforms: &mut BTreeMap<String, ()>) {
    for file in files {
        if let PlatformValue::Platforms(sources) = &file.source {
            for platform in sources
                .keys()
                .filter(|platform| platform.as_str() != "default")
            {
                platforms.insert(platform.clone(), ());
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Toml,
    Json,
    Yaml,
}

impl From<Format> for FileFormat {
    fn from(value: Format) -> Self {
        match value {
            Format::Toml => FileFormat::Toml,
            Format::Json => FileFormat::Json,
            Format::Yaml => FileFormat::Yaml,
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

    /// OCI platforms to build. When omitted, platforms are inferred from platform-specific files,
    /// then default to Linux on the host CPU architecture.
    #[serde(default)]
    pub platforms: Vec<String>,

    /// Named layers to append in this order after all base-image layers.
    #[serde(default)]
    pub layers: Vec<String>,

    /// Files placed in one implicit final layer after all named layers.
    #[serde(default)]
    pub files: Vec<FileMapping>,

    /// Executable and fixed arguments used when the container starts. An empty list clears the
    /// base image's entrypoint.
    #[serde(default)]
    pub entrypoint: Option<Vec<String>>,
    /// Default arguments passed to the entrypoint. An empty list clears the base image's command.
    #[serde(default)]
    pub command: Option<Vec<String>>,
    /// User and optional group used to run the container, such as `65532:65532`. An empty string
    /// clears the base image's user.
    #[serde(default)]
    pub user: Option<String>,
    /// Working directory used when the container starts. An empty string clears the base value.
    #[serde(default)]
    pub workdir: Option<String>,
    /// Signal used to stop the container, such as `SIGTERM`. An empty string clears the base value.
    #[serde(default)]
    pub stop_signal: Option<String>,
    /// Ports exposed by the image in `PORT/PROTOCOL` form, such as `8080/tcp`. An empty list clears
    /// inherited ports.
    #[serde(default)]
    pub expose: Option<Vec<String>>,
    /// Container paths intended to hold externally mounted volumes. An empty list clears inherited
    /// volumes.
    #[serde(default)]
    pub volumes: Option<Vec<String>>,

    /// Environment variables merged with the base image. Values here take precedence.
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    /// Docker-compatible image labels merged with the base image. Values here take precedence.
    #[serde(default)]
    pub labels: BTreeMap<String, String>,
    /// OCI annotations merged with the base image. Values here take precedence.
    #[serde(default)]
    pub annotations: BTreeMap<String, String>,

    /// Complete image references used when publishing, such as `ghcr.io/example/app:v1`. Tags are
    /// not inherited from local base images.
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

/// Detailed options for copying a local file or directory into an image layer.
///
/// Sources are relative to the configuration file. A directory copies its contents into `to`, a
/// file uses `to` as its exact destination, and trailing slashes do not alter either behavior.
/// Mappings are applied in list order, with later mappings replacing earlier paths.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FileMapping {
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
    /// Portable file mode as a four-digit octal string, such as `0755`.
    #[serde(default, deserialize_with = "deserialize_mode")]
    pub mode: Option<String>,
    /// Numeric owner in `UID` or `UID:GID` form.
    #[serde(default)]
    pub owner: Option<String>,
    /// Follow symlink targets instead of archiving symlinks. Defaults to `false`.
    #[serde(default)]
    pub follow_symlinks: bool,
    /// Preserve the source path below the destination instead of copying only its contents.
    #[serde(default)]
    pub preserve_paths: bool,
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

fn deserialize_mode<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum ModeValue {
        String(String),
        Integer(i64),
    }

    match Option::<ModeValue>::deserialize(deserializer)? {
        Some(ModeValue::String(mode)) => Ok(Some(mode)),
        Some(ModeValue::Integer(mode)) => Err(de::Error::custom(format!(
            "file mode {mode} must be a quoted octal string such as \"0755\""
        ))),
        None => Ok(None),
    }
}

impl<T> PlatformValue<T> {
    pub fn for_platform(&self, platform: &str) -> Option<&T> {
        match self {
            Self::Scalar(value) => Some(value),
            Self::Platforms(values) => values.get(platform).or_else(|| values.get("default")),
        }
    }
}

#[derive(Debug, Error)]
pub enum Error {
    #[error(transparent)]
    Parse(#[from] ::config::ConfigError),
    #[error(
        "unsupported config format for {}; expected .toml, .json, .yaml, or .yml",
        .0.display()
    )]
    UnsupportedFormat(PathBuf),
    #[error("{0}")]
    Invalid(String),
}

fn format_for_path(path: &Path) -> Result<FileFormat, Error> {
    match path.extension().and_then(|extension| extension.to_str()) {
        Some("toml") => Ok(FileFormat::Toml),
        Some("json") => Ok(FileFormat::Json),
        Some("yaml" | "yml") => Ok(FileFormat::Yaml),
        _ => Err(Error::UnsupportedFormat(path.to_owned())),
    }
}
