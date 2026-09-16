use clap::{Args, Parser};

#[derive(Debug, Parser)]
#[command(name = "cargo", bin_name = "cargo")]
#[command(styles = clap_cargo::style::CLAP_STYLING)]
enum Cargo {
    /// Build Rust artifacts and package them as OCI images.
    #[command(version)]
    Containerless(Options),
}

#[derive(Debug, Args)]
struct Options {
    #[command(flatten)]
    manifest: clap_cargo::Manifest,

    #[command(flatten)]
    workspace: clap_cargo::Workspace,

    #[command(flatten)]
    features: clap_cargo::Features,
}

fn main() {
    match Cargo::parse() {
        Cargo::Containerless(options) => {
            let _ = (options.manifest, options.workspace, options.features);
        }
    }
}
