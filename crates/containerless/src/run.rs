use std::path::PathBuf;

use clap::CommandFactory;
use containerless_core::{BuildError, BuildResult, OciOutput, PublishError};
use thiserror::Error;
use tracing::info;

use crate::builder::ImageBuilder;
use crate::cli;
use crate::cli::{
    BuildCommand, Cli, Command, CompletionsCommand, InspectCommand, OutputFormat, PublishCommand,
};
use crate::config::Error as ConfigError;

/// An error produced while invoking a Containerless command.
#[derive(Debug, Error)]
pub enum CliError {
    /// Command-line arguments were invalid or requested help or version output.
    #[error(transparent)]
    Arguments(#[from] clap::Error),
    /// Configuration loading or validation failed.
    #[error(transparent)]
    Config(#[from] ConfigError),
    /// Image resolution failed.
    #[error("{0}")]
    Resolution(String),
    /// OCI image construction failed.
    #[error(transparent)]
    Build(#[from] BuildError),
    /// Image publication failed.
    #[error(transparent)]
    Publish(#[from] PublishError),
    /// A filesystem operation failed.
    #[error(transparent)]
    Io(#[from] std::io::Error),
    /// JSON output could not be serialized.
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

/// Executes a parsed command.
pub(super) async fn execute(cli: Cli) -> Result<(), CliError> {
    match cli.command {
        Command::Build(command) => command.run(cli.file).await,
        Command::Completions(command) => command.run(),
        Command::Inspect(command) => command.run(cli.file).await,
        Command::Publish(command) => command.run(cli.file).await,
    }
}

impl CompletionsCommand {
    fn run(self) -> Result<(), CliError> {
        clap_complete::generate(
            self.shell,
            &mut Cli::command(),
            "containerless",
            &mut std::io::stdout(),
        );
        Ok(())
    }
}

impl BuildCommand {
    async fn run(mut self, file: Option<PathBuf>) -> Result<(), CliError> {
        self.options.apply_metadata()?;
        let output_destination = self.output.clone();
        let output = self
            .output
            .map(|destination| match self.format.unwrap_or_default() {
                OutputFormat::Oci => OciOutput::Archive(destination),
                OutputFormat::OciDirectory => OciOutput::Layout(destination),
            });
        self.options.validate_sidecars(output.as_ref())?;
        let push = self.push;
        let builder = ImageBuilder::load(file, self.selection, self.options)?;
        let images = builder.resolve().await.map_err(CliError::Resolution)?;
        let images = images.with_source_date_epoch(source_date_epoch()?);
        let result = if push {
            builder.registry().publish(images, output).await?
        } else {
            images.build(output.expect("clap requires a build output"))?
        };
        if let Some(destination) = output_destination {
            info!(output = %destination.display(), digest = %result.index_digest, "build complete");
        } else {
            info!(digest = %result.index_digest, "build complete");
        }
        builder.options().report(&result)
    }
}

impl PublishCommand {
    async fn run(mut self, file: Option<PathBuf>) -> Result<(), CliError> {
        self.options.apply_metadata()?;
        self.options.validate_sidecars(None)?;
        let builder = if let Some(source) = self.source {
            ImageBuilder::from_source(source, self.options)
        } else {
            ImageBuilder::load(file, self.selection, self.options)?
        };
        let images = builder.resolve().await.map_err(CliError::Resolution)?;
        let result = builder
            .registry()
            .publish(images.with_source_date_epoch(source_date_epoch()?), None)
            .await?;
        info!(digest = %result.index_digest, "publish complete");
        builder.options().report(&result)
    }
}

impl InspectCommand {
    async fn run(self, file: Option<PathBuf>) -> Result<(), CliError> {
        let options = cli::BuildOptions {
            platforms: self.platforms,
            ..cli::BuildOptions::default()
        };
        let images = ImageBuilder::load(file, self.selection, options)?
            .resolve()
            .await
            .map_err(CliError::Resolution)?;
        if self.json {
            println!("{}", serde_json::to_string_pretty(images.images())?);
        } else {
            for image in images.images() {
                println!("{}", image.name);
                for platform in &image.platforms {
                    println!("  {} ({} layers)", platform.platform, platform.layers.len());
                }
            }
        }
        Ok(())
    }
}

impl cli::BuildOptions {
    fn validate_sidecars(&self, output: Option<&OciOutput>) -> Result<(), CliError> {
        if let (Some(digest), Some(metadata)) =
            (self.digest_file.as_ref(), self.metadata_file.as_ref())
            && normalized_output_path(digest)? == normalized_output_path(metadata)?
        {
            return Err(CliError::Resolution(
                "--digest-file and --metadata-file must be different files".to_owned(),
            ));
        }
        let Some(output) = output else {
            return Ok(());
        };
        let destination = output.path();
        let output_path = normalized_output_path(destination)?;
        for sidecar in [self.digest_file.as_ref(), self.metadata_file.as_ref()]
            .into_iter()
            .flatten()
        {
            if normalized_output_path(sidecar)? == output_path {
                return Err(CliError::Resolution(format!(
                    "output {} and sidecar {} must be different files",
                    destination.display(),
                    sidecar.display()
                )));
            }
        }
        Ok(())
    }

    fn report(&self, result: &BuildResult) -> Result<(), CliError> {
        if let Some(path) = &self.digest_file {
            std::fs::write(path, format!("{}\n", result.index_digest))?;
        }
        if let Some(path) = &self.metadata_file {
            std::fs::write(path, format!("{}\n", serde_json::to_string_pretty(result)?))?;
        }
        println!("{}", result.index_digest);
        Ok(())
    }
}

fn source_date_epoch() -> Result<u64, CliError> {
    match std::env::var("SOURCE_DATE_EPOCH") {
        Ok(value) => value.parse().map_err(|_| {
            CliError::Resolution("SOURCE_DATE_EPOCH must be a non-negative integer".to_owned())
        }),
        Err(std::env::VarError::NotPresent) => Ok(0),
        Err(error) => Err(CliError::Resolution(format!(
            "cannot read SOURCE_DATE_EPOCH: {error}"
        ))),
    }
}

fn normalized_output_path(path: &std::path::Path) -> Result<PathBuf, CliError> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| std::path::Path::new("."));
    std::fs::create_dir_all(parent)?;
    let name = path
        .file_name()
        .ok_or_else(|| CliError::Resolution(format!("invalid output path {}", path.display())))?;
    Ok(parent.canonicalize()?.join(name))
}
