//! `jrutil validate <package-dir>`: contract validation of a production package.

use std::path::Path;
use std::process::ExitCode;
use std::time::Instant;

use jr_publish::{Package, validate};

/// Prints every finding (`code subject: detail`) and a summary; fails when there are any.
pub fn run(root: &Path) -> ExitCode {
    let started = Instant::now();
    let findings = match Package::open(root).and_then(|package| validate(&package)) {
        Ok(findings) => findings,
        Err(error) => {
            eprintln!("error: {error}");
            return ExitCode::from(2);
        }
    };
    for finding in &findings {
        println!("{finding}");
    }
    let elapsed = started.elapsed().as_secs_f64();
    if findings.is_empty() {
        println!("valid: {} ({elapsed:.1} s)", root.display());
        ExitCode::SUCCESS
    } else {
        println!(
            "invalid: {} findings in {} ({elapsed:.1} s)",
            findings.len(),
            root.display()
        );
        ExitCode::FAILURE
    }
}
