mod builder;
mod cli;
pub mod config;
mod run;

use std::ffi::OsString;

use clap::Parser;

pub use run::CliError;

/// Parses and executes a Containerless command-line invocation.
///
/// `arguments` includes the executable name followed by command-line arguments.
pub async fn run_cli<I, T>(arguments: I) -> Result<(), CliError>
where
    I: IntoIterator<Item = T>,
    T: Into<OsString> + Clone,
{
    run::execute(cli::Cli::try_parse_from(arguments)?).await
}
