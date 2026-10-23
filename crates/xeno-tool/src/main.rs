//! Developer CLI for examining the original game files while their formats are being
//! reverse engineered. It only reads; anything it writes goes to a directory the user
//! names, never into the repo.

mod dump;
mod inspect;
mod palette;
mod summary;
mod wav;

use std::path::PathBuf;
use std::process::ExitCode;

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
    /// Print a file's size, extension and first bytes, and what its parser makes of it.
    ///
    /// With `--all`, run every file in a directory through its parser instead, and print
    /// a table with a row per extension (files, parsed, failed, no parser), then each
    /// failure. Files that no parser handles (the executable and driver modules, text,
    /// disk markers, unknown types) are counted under "no parser" and do not fail the
    /// run: the exit status is non-zero only if a file that has a parser fails to parse
    /// or cannot be read.
    Inspect {
        /// The file to inspect, or with `--all` the directory.
        path: PathBuf,
        /// Summarise every file directly in the directory; subdirectories are not
        /// entered.
        #[arg(long)]
        all: bool,
    },
    /// Decode a file and write its images as PNGs. Supports: pcx, b@n, m@l, vcl, and the
    /// m00, m08, m16 and m32 models (one PNG per frame; a model's .m00 is read from beside
    /// it when its parts need one). dsounds.bin gives one WAV per sound, and mgavideo.dvr
    /// one sheet per font.
    Dump {
        /// The file to decode.
        file: PathBuf,
        /// Directory to write into; created if missing. Never point it into the repo
        /// outside the gitignored `out/`.
        #[arg(long, default_value = "out")]
        out: PathBuf,
        /// Take colours from this file's palette (pcx, vcl, m@l) for files that have none
        /// of their own, such as b@n and models. Without it they come out in grey.
        #[arg(long)]
        palette: Option<PathBuf>,
    },
    /// Print a file's 256-colour palette, one colour per line. Finds it by extension
    /// (pcx, vcl, m@l), or reads it raw from `--offset`.
    Palette {
        /// The file to read.
        file: PathBuf,
        /// Read a raw palette starting at this byte offset (decimal, or hex with `0x`).
        #[arg(long, value_parser = palette::parse_offset)]
        offset: Option<usize>,
        /// Encoding of the raw palette at `--offset`.
        #[arg(long, value_enum, default_value_t = palette::Encoding::Vga6, requires = "offset")]
        encoding: palette::Encoding,
    },
}

fn main() -> anyhow::Result<ExitCode> {
    match Cli::parse().command {
        Command::Inspect { path, all: true } => {
            let summary = summary::Summary::of_dir(&path)?;
            print!("{}", summary.table());
            if summary.failed() > 0 {
                eprintln!("{} file(s) failed to parse", summary.failed());
                return Ok(ExitCode::FAILURE);
            }
        }
        Command::Inspect { path, all: false } => {
            let bytes =
                std::fs::read(&path).with_context(|| format!("cannot read {}", path.display()))?;
            print!("{}", inspect::report(&path, &bytes));
        }
        Command::Dump { file, out, palette } => {
            let bytes =
                std::fs::read(&file).with_context(|| format!("cannot read {}", file.display()))?;
            let palette = palette
                .map(|path| {
                    let bytes = std::fs::read(&path)
                        .with_context(|| format!("cannot read {}", path.display()))?;
                    palette::find(&path, &bytes, None).map(|(palette, _)| palette)
                })
                .transpose()?;
            for path in dump::dump(&file, &bytes, &out, palette.as_ref())? {
                println!("wrote {}", path.display());
            }
        }
        Command::Palette {
            file,
            offset,
            encoding,
        } => {
            let bytes =
                std::fs::read(&file).with_context(|| format!("cannot read {}", file.display()))?;
            let raw = offset.map(|offset| palette::Raw { offset, encoding });
            let (palette, source) = palette::find(&file, &bytes, raw)?;
            eprintln!("{}: {source}", file.display());
            print!("{}", palette::listing(&palette));
        }
    }
    Ok(ExitCode::SUCCESS)
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
