//! The jrutil-rs command line. Each subcommand lives in its own module under `commands`; this
//! file only parses arguments and maps a failure to the exit code.

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};

mod commands {
    pub mod compare;
    pub mod validate;
}

#[derive(Debug, Parser)]
#[command(
    name = "jrutil",
    version,
    about = "The static timetable compiler for Oběhy"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Check a production package against the serving and production contracts.
    Validate {
        /// The package directory (manifest.json, gtfs.zip, serving/, diagnostics.json).
        package: PathBuf,
    },
    /// Compare a candidate package with a reference, judged by a parity ledger.
    Compare {
        /// The reference package (A).
        reference: PathBuf,
        /// The candidate package (B).
        candidate: PathBuf,
        /// The parity ledger (`parity/<package>.toml`) that explains the differences.
        #[arg(long)]
        expect: Option<PathBuf>,
        /// Write the full result as JSON.
        #[arg(long)]
        report: Option<PathBuf>,
        /// Samples kept per category (default 20).
        #[arg(long)]
        samples: Option<usize>,
    },
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match cli.command {
        Command::Validate { package } => commands::validate::run(&package),
        Command::Compare {
            reference,
            candidate,
            expect,
            report,
            samples,
        } => commands::compare::run(&commands::compare::Options {
            reference: &reference,
            candidate: &candidate,
            expect: expect.as_deref(),
            report: report.as_deref(),
            samples,
        }),
    }
}
