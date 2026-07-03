use clap::{Args, Parser, Subcommand, ValueEnum};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::{path::PathBuf, str::FromStr};

#[derive(Debug, Parser)]
#[command(name = "uniko")]
#[command(version)]
#[command(about = "Pack binaries into minimal OCI images")]
pub struct Cli {
    /// Path to uniko.toml
    #[arg(short, long, global = true, default_value = "uniko.toml")]
    pub config: PathBuf,

    /// Do not load config file, only use CLI args
    pub no_config: bool,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Inspect a binary or image
    Inspect(InspectCommand),

    /// Pack one or more existing binaries into an image
    Pack(PackCommand),

    /// Build using a supported build system, then pack
    Build(BuildCommand),
}

#[derive(Debug, Args)]
pub struct InspectCommand {
    /// Binary path or image reference
    pub input: String,

    /// Print JSON
    #[arg(long)]
    pub json: bool,
}

#[derive(Debug, Args)]
pub struct PackCommand {
    #[command(flatten)]
    pub args: PackArgs,
}

#[derive(Debug, Args)]
pub struct BuildCommand {
    #[command(flatten)]
    pub args: PackArgs,

    /// Build system to use. If omitted, the build system is inferred from the current directory.
    #[arg(short = 'b', long = "build-system", value_enum, default_value_t = BuildPreset::Auto)]
    pub build_system: BuildPreset,
}

#[derive(Debug, Args)]
pub struct PackArgs {
    /// Artifact to include.
    ///
    /// Forms:
    ///   ./server:/app/server
    ///   linux/amd64=./server-amd64:/app/server
    ///   linux/amd64,linux/arm64=./server:/app/server
    #[arg(short = 'a', long = "artifact")]
    pub artifacts: Vec<Artifact>,

    /// Output platforms, e.g. linux/amd64,linux/arm64.
    ///
    /// If omitted, platforms are inferred from artifacts/config/binaries.
    #[arg(long, value_delimiter = ',')]
    pub platform: Vec<String>,

    /// Image tag/ref, e.g. ghcr.io/me/app:latest
    #[arg(short = 't', long = "tag")]
    pub tag: Option<String>,

    /// Push after packing
    #[arg(long)]
    pub push: bool,
}

#[derive(Debug, Clone)]
pub struct Artifact {
    pub path: PathBuf,
    pub dest: String,
    pub platforms: Option<Vec<String>>,
}

impl FromStr for Artifact {
    type Err = String;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        let (platforms, rest) = match input.split_once('=') {
            Some((lhs, rhs)) if lhs.contains('/') => {
                let platforms = lhs
                    .split(',')
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(str::to_owned)
                    .collect::<Vec<_>>();

                if platforms.is_empty() {
                    return Err("artifact platform list cannot be empty".to_owned());
                }

                (Some(platforms), rhs)
            }
            _ => (None, input),
        };

        let Some((path, dest)) = rest.split_once(':') else {
            return Err("artifact must be in the form <path>:<dest>".to_owned());
        };

        if path.is_empty() {
            return Err("artifact path cannot be empty".to_owned());
        }

        if dest.is_empty() {
            return Err("artifact destination cannot be empty".to_owned());
        }

        Ok(Self {
            path: PathBuf::from(path),
            dest: dest.to_owned(),
            platforms,
        })
    }
}

#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema, ValueEnum,
)]
#[serde(rename_all = "kebab-case")]
pub enum BuildPreset {
    #[default]
    Auto,
    Cargo,
    Go,
    Zig,
}
