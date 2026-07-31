use std::path::PathBuf;
use std::str::FromStr;

use clap::{Args, Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(name = "containerless", version)]
#[command(about = "Build minimal, multi-platform OCI images from local files")]
pub struct Cli {
    /// Configuration file. Supported extensions: toml, json, yaml, and yml.
    #[arg(short = 'f', long = "file", global = true, value_name = "PATH")]
    pub file: Option<PathBuf>,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Build images and write them to one or more explicit outputs.
    Build(BuildCommand),

    /// Build images and publish them to registries.
    Publish(PublishCommand),

    /// Resolve images and print what would be built.
    Inspect(InspectCommand),
}

#[derive(Debug, Args)]
pub struct BuildCommand {
    #[command(flatten)]
    pub selection: ImageSelection,

    #[command(flatten)]
    pub build: BuildOptions,

    /// Push to the configured or CLI-supplied references.
    #[arg(long)]
    pub push: bool,
}

#[derive(Debug, Args)]
pub struct PublishCommand {
    #[command(flatten)]
    pub selection: ImageSelection,

    #[command(flatten)]
    pub build: BuildOptions,
}

#[derive(Debug, Args)]
pub struct InspectCommand {
    #[command(flatten)]
    pub selection: ImageSelection,

    /// Print the resolved description as JSON.
    #[arg(long)]
    pub json: bool,
}

#[derive(Debug, Args)]
pub struct ImageSelection {
    /// Images to process.
    #[arg(value_name = "IMAGE")]
    pub targets: Vec<String>,

    /// Select every configured image.
    #[arg(long, conflicts_with = "targets")]
    pub all: bool,
}

#[derive(Debug, Args)]
pub struct BuildOptions {
    #[command(flatten)]
    pub registry: RegistryOptions,

    /// Copy a local source to a destination in the image.
    ///
    /// Examples:
    ///   --copy assets:/app/assets
    ///   --copy platform=linux/arm64,dist/linux-arm64/app:/app
    #[arg(long = "copy", value_name = "[platform=PLATFORM,]SOURCE:DESTINATION")]
    pub copies: Vec<CopyInput>,

    /// Write a build result using Docker-style exporter options.
    ///
    /// Examples:
    ///   --output type=oci,dest=image.tar
    ///   --output type=oci-dir,dest=image-layout
    ///   --output type=registry
    #[arg(short = 'o', long = "output", value_name = "OPTIONS")]
    pub outputs: Vec<Output>,

    /// Load the result into an available Docker or Podman installation.
    #[arg(long)]
    pub load: bool,

    /// Read tags and labels from Docker Metadata Action JSON.
    #[arg(long = "metadata-from", value_name = "PATH")]
    pub metadata_from: Option<PathBuf>,

    /// Write structured build result metadata.
    #[arg(long = "metadata-file", value_name = "PATH")]
    pub metadata_file: Option<PathBuf>,

    /// Write the top-level image digest.
    #[arg(long = "digest-file", value_name = "PATH")]
    pub digest_file: Option<PathBuf>,

    /// Set an image label.
    #[arg(long = "label", value_name = "KEY=VALUE")]
    pub labels: Vec<KeyValue>,

    /// Set an OCI annotation.
    #[arg(long = "annotation", value_name = "KEY=VALUE")]
    pub annotations: Vec<KeyValue>,

    /// Combine layers added by Containerless into one layer.
    #[arg(long, conflicts_with = "flatten")]
    pub squash: bool,

    /// Emit the complete resulting filesystem as one layer.
    #[arg(long, conflicts_with = "squash")]
    pub flatten: bool,
}

#[derive(Debug, Args)]
pub struct RegistryOptions {
    /// Full image reference to publish, optionally scoped as IMAGE=REFERENCE.
    #[arg(short = 't', long = "tag", value_name = "[IMAGE=]REFERENCE")]
    pub tags: Vec<String>,

    /// Allow plain HTTP for a registry host. May be repeated.
    #[arg(long = "plain-http", value_name = "REGISTRY")]
    pub plain_http: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CopyInput {
    pub platform: Option<String>,
    pub source: PathBuf,
    pub destination: String,
}

impl FromStr for CopyInput {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let mut parts = value.split(',').collect::<Vec<_>>();
        let mapping = parts
            .pop()
            .filter(|mapping| !mapping.is_empty())
            .ok_or_else(|| "copy mapping must end with SOURCE:DESTINATION".to_owned())?;
        let mut platform = None;
        for option in parts {
            let Some((name, value)) = option.split_once('=') else {
                return Err(format!("copy option {option:?} must be KEY=VALUE"));
            };
            match name {
                "platform" if platform.is_some() => {
                    return Err("copy platform cannot be specified more than once".to_owned());
                }
                "platform" if value.contains('/') => platform = Some(value.to_owned()),
                "platform" => {
                    return Err(
                        "copy platform must be an OCI platform such as linux/amd64".to_owned()
                    );
                }
                _ => return Err(format!("unknown copy option {name:?}")),
            }
        }
        let Some((source, destination)) = mapping.split_once(':') else {
            return Err("copy mapping must end with SOURCE:DESTINATION".to_owned());
        };
        if source.is_empty() {
            return Err("copy source cannot be empty".to_owned());
        }
        if !destination.starts_with('/') {
            return Err("copy destination must be an absolute path".to_owned());
        }

        Ok(Self {
            platform,
            source: source.into(),
            destination: destination.to_owned(),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyValue {
    pub key: String,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Output {
    pub kind: OutputType,
    pub destination: Option<PathBuf>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputType {
    Registry,
    Oci,
    Docker,
    OciDirectory,
}

impl FromStr for Output {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let mut kind = None;
        let mut destination = None;

        for option in value.split(',') {
            let Some((name, value)) = option.split_once('=') else {
                return Err(format!("output option {option:?} must be KEY=VALUE"));
            };
            if value.is_empty() {
                return Err(format!("output option {name:?} cannot be empty"));
            }
            match name {
                "type" if kind.is_some() => {
                    return Err("output type cannot be specified more than once".to_owned());
                }
                "type" => {
                    kind = Some(match value {
                        "registry" => OutputType::Registry,
                        "oci" => OutputType::Oci,
                        "docker" => OutputType::Docker,
                        "oci-dir" => OutputType::OciDirectory,
                        _ => return Err(format!("unknown output type {value:?}")),
                    });
                }
                "dest" if destination.is_some() => {
                    return Err("output destination cannot be specified more than once".to_owned());
                }
                "dest" => destination = Some(value.into()),
                _ => return Err(format!("unknown output option {name:?}")),
            }
        }

        let kind = kind.ok_or_else(|| "output requires type=TYPE".to_owned())?;
        if kind == OutputType::Registry && destination.is_some() {
            return Err("registry output does not accept dest".to_owned());
        }
        if kind != OutputType::Registry && destination.is_none() {
            return Err("non-registry output requires dest=PATH".to_owned());
        }

        Ok(Self { kind, destination })
    }
}

impl FromStr for KeyValue {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let Some((key, value)) = value.split_once('=') else {
            return Err("assignment must be KEY=VALUE".to_owned());
        };
        if key.is_empty() {
            return Err("assignment key cannot be empty".to_owned());
        }
        Ok(Self {
            key: key.to_owned(),
            value: value.to_owned(),
        })
    }
}
