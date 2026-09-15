use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
use std::str::FromStr;

use clap::builder::styling::{AnsiColor, Styles};
use clap::{Args, Parser, Subcommand, ValueEnum};
use clap_complete::Shell;
use serde::Deserialize;

use crate::CliError;

const HELP_STYLES: Styles = Styles::styled()
    .header(AnsiColor::Green.on_default().bold())
    .usage(AnsiColor::Green.on_default().bold())
    .literal(AnsiColor::Cyan.on_default().bold())
    .placeholder(AnsiColor::Cyan.on_default())
    .error(AnsiColor::Red.on_default().bold())
    .valid(AnsiColor::Cyan.on_default().bold())
    .invalid(AnsiColor::Yellow.on_default().bold());

#[derive(Debug, Parser)]
#[command(name = "containerless", version)]
#[command(about = "Build minimal, multi-platform OCI images from local files")]
#[command(styles = HELP_STYLES)]
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

    /// Generate a shell completion script.
    Completions(CompletionsCommand),
}

#[derive(Debug, Args)]
pub struct CompletionsCommand {
    /// Shell for which to generate completions.
    #[arg(value_enum)]
    pub shell: Shell,
}

#[derive(Debug, Args)]
#[group(
    id = "destination",
    required = true,
    multiple = true,
    args = ["push", "output"]
)]
pub struct BuildCommand {
    #[command(flatten)]
    pub selection: ImageSelection,

    #[command(flatten)]
    pub options: BuildOptions,

    /// Push to the configured or CLI-supplied references.
    #[arg(long)]
    pub push: bool,

    /// Write an OCI archive or another selected format to this path.
    #[arg(short = 'o', long = "output", value_name = "PATH")]
    pub output: Option<PathBuf>,

    /// Format written by --output. Defaults to oci.
    #[arg(long, value_enum, requires = "output")]
    pub format: Option<OutputFormat>,
}

#[derive(Debug, Args)]
pub struct PublishCommand {
    #[command(flatten)]
    pub selection: ImageSelection,

    #[command(flatten)]
    pub options: BuildOptions,

    /// Package one local file without a configuration file.
    #[arg(
        long = "from",
        value_name = "PATH",
        conflicts_with_all = ["file", "targets", "all"]
    )]
    pub source: Option<PathBuf>,
}

#[derive(Debug, Args)]
pub struct InspectCommand {
    #[command(flatten)]
    pub selection: ImageSelection,

    /// Resolve only these OCI platforms. May be repeated or comma-separated.
    #[arg(
        long = "platform",
        value_name = "PLATFORM",
        value_delimiter = ',',
        value_parser = parse_platform
    )]
    pub platforms: Vec<String>,

    /// Print the image definition as JSON.
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

#[derive(Debug, Args, Default)]
pub struct BuildOptions {
    /// Copy a local source to a destination in the image.
    ///
    /// Examples:
    ///   --copy assets:/app/assets
    #[arg(long = "copy", value_name = "SOURCE:DESTINATION")]
    pub copies: Vec<CopyInput>,

    /// Build only these OCI platforms. May be repeated or comma-separated.
    #[arg(
        long = "platform",
        value_name = "PLATFORM",
        value_delimiter = ',',
        value_parser = parse_platform
    )]
    pub platforms: Vec<String>,

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

    /// Full image reference to publish, optionally scoped as IMAGE=REFERENCE. May be repeated.
    #[arg(short = 't', long = "tag", value_name = "[IMAGE=]REFERENCE")]
    pub tags: Vec<String>,

    /// Allow plain HTTP for a registry host. May be repeated.
    #[arg(long = "plain-http", value_name = "REGISTRY")]
    pub plain_http: Vec<String>,
}

impl BuildOptions {
    pub(super) fn apply_metadata(&mut self) -> Result<(), CliError> {
        let Some(path) = &self.metadata_from else {
            return Ok(());
        };
        let metadata: DockerMetadata = serde_json::from_slice(&fs::read(path)?)?;
        self.tags.splice(0..0, metadata.tags);
        self.labels.splice(0..0, metadata.labels.assignments()?);
        self.annotations
            .splice(0..0, metadata.annotations.assignments()?);
        Ok(())
    }
}

#[derive(Deserialize)]
struct DockerMetadata {
    #[serde(default)]
    tags: Vec<String>,
    #[serde(default)]
    labels: MetadataValues,
    #[serde(default)]
    annotations: MetadataValues,
}

#[derive(Default, Deserialize)]
#[serde(untagged)]
enum MetadataValues {
    Map(BTreeMap<String, String>),
    List(Vec<String>),
    #[default]
    Empty,
}

impl MetadataValues {
    fn assignments(self) -> Result<Vec<KeyValue>, CliError> {
        match self {
            Self::Map(values) => Ok(values
                .into_iter()
                .map(|(key, value)| KeyValue { key, value })
                .collect()),
            Self::List(values) => values
                .into_iter()
                .map(|assignment| assignment.parse().map_err(CliError::Resolution))
                .collect(),
            Self::Empty => Ok(Vec::new()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CopyInput {
    pub source: PathBuf,
    pub destination: String,
}

impl FromStr for CopyInput {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        if value.starts_with("platform=") {
            return Err(
                "platform-specific copies belong in the configuration file; use --platform to select output platforms"
                    .to_owned(),
            );
        }
        let Some((source, destination)) = value.split_once(':') else {
            return Err("copy mapping must be SOURCE:DESTINATION".to_owned());
        };
        if source.is_empty() {
            return Err("copy source cannot be empty".to_owned());
        }
        if !destination.starts_with('/') {
            return Err("copy destination must be an absolute path".to_owned());
        }

        Ok(Self {
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

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, ValueEnum)]
pub enum OutputFormat {
    #[default]
    Oci,
    #[value(name = "oci-dir")]
    OciDirectory,
}

fn parse_platform(value: &str) -> Result<String, String> {
    containerless_core::Platform::from_str(value)
        .map(|platform| platform.to_string())
        .map_err(|error| error.to_string())
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_build_outputs_and_scoped_tags() {
        let cli = Cli::try_parse_from([
            "containerless",
            "build",
            "app",
            "--output",
            "app.tar",
            "--format",
            "oci-dir",
            "--push",
            "--tag",
            "app=ghcr.io/example/app:latest",
        ])
        .unwrap();

        let Command::Build(command) = cli.command else {
            panic!("expected build command");
        };
        assert_eq!(command.output, Some("app.tar".into()));
        assert_eq!(command.format, Some(OutputFormat::OciDirectory));
        assert_eq!(command.options.tags, ["app=ghcr.io/example/app:latest"]);
    }

    #[test]
    fn parses_zero_config_publish() {
        let cli = Cli::try_parse_from([
            "containerless",
            "publish",
            "--from",
            "dist/app",
            "--tag",
            "ghcr.io/example/app:latest",
            "--platform",
            "linux/amd64,linux/arm64",
        ])
        .unwrap();

        let Command::Publish(command) = cli.command else {
            panic!("expected publish command");
        };
        assert_eq!(command.source, Some("dist/app".into()));
        assert_eq!(command.options.platforms, ["linux/amd64", "linux/arm64"]);
    }

    #[test]
    fn parses_completion_shell() {
        let cli = Cli::try_parse_from(["containerless", "completions", "fish"]).unwrap();

        let Command::Completions(command) = cli.command else {
            panic!("expected completions command");
        };
        assert_eq!(command.shell, Shell::Fish);
    }
}
