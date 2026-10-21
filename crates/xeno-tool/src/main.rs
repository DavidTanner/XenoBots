//! Developer CLI for examining the original game files while their formats are being
//! reverse engineered. It only reads; anything it writes goes to a directory the user
//! names, never into the repo.

mod dump;
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
    /// Decode a file and write its images as PNGs. Supports: pcx.
    Dump {
        /// The file to decode.
        file: PathBuf,
        /// Directory to write into; created if missing. Never point it into the repo
        /// outside the gitignored `out/`.
        #[arg(long, default_value = "out")]
        out: PathBuf,
    },
}

fn main() -> anyhow::Result<()> {
    match Cli::parse().command {
        Command::Inspect { file } => {
            let bytes =
                std::fs::read(&file).with_context(|| format!("cannot read {}", file.display()))?;
            print!("{}", inspect::report(&file, &bytes));
        }
        Command::Dump { file, out } => {
            let bytes =
                std::fs::read(&file).with_context(|| format!("cannot read {}", file.display()))?;
            for path in dump::dump(&file, &bytes, &out)? {
                println!("wrote {}", path.display());
            }
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
