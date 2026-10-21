//! Developer CLI for examining the original game files while their formats are being
//! reverse engineered. It only reads; anything it writes goes to a directory the user
//! names, never into the repo.

mod inspect;

use std::path::PathBuf;

use anyhow::Context;
use clap::{Parser, Subcommand};

/// Inspect the original Xenobots files.
#[derive(Debug, Parser)]
#[command(version)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Print a file's size, extension and first bytes.
    Inspect {
        /// The file to inspect.
        file: PathBuf,
    },
}

fn main() -> anyhow::Result<()> {
    match Cli::parse().command {
        Command::Inspect { file } => {
            let bytes =
                std::fs::read(&file).with_context(|| format!("cannot read {}", file.display()))?;
            print!("{}", inspect::report(&file, &bytes));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cli_definition_is_valid() {
        use clap::CommandFactory;
        Cli::command().debug_assert();
    }
}
