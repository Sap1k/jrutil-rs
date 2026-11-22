//! `jrutil compare <reference> <candidate> [--expect ledger.toml] [--report out.json]
//! [--samples N]`: canonical comparison of two production packages, judged by a parity ledger.

use std::path::Path;
use std::process::ExitCode;
use std::time::Instant;

use jr_publish::compare::{Status, report_json};
use jr_publish::{Comparison, Ledger, Package, Profile, Verdict, compare_classified};

#[derive(Debug)]
pub struct Options<'a> {
    pub reference: &'a Path,
    pub candidate: &'a Path,
    pub expect: Option<&'a Path>,
    pub report: Option<&'a Path>,
    pub samples: Option<usize>,
}

/// Exit 0 when every difference is explained and in range (without a ledger: when there are
/// none), 1 otherwise, 2 when a package, the ledger or the report cannot be read or written.
pub fn run(options: &Options) -> ExitCode {
    let started = Instant::now();
    let ledger = match options.expect.map(read_ledger).transpose() {
        Ok(ledger) => ledger.unwrap_or_default(),
        Err(error) => {
            eprintln!("error: {error}");
            return ExitCode::from(2);
        }
    };
    let mut profile = Profile {
        labels: ledger
            .labels
            .iter()
            .map(|label| (label.relation.clone(), label.column.clone()))
            .collect(),
        trip_identity: ledger.trip_identity,
        ..Profile::default()
    };
    if let Some(samples) = options.samples {
        profile.samples = samples;
    }
    let comparison = Package::open(options.reference).and_then(|a| {
        let b = Package::open(options.candidate)?;
        compare_classified(&a, &b, &profile, &ledger)
    });
    let comparison = match comparison {
        Ok(comparison) => comparison,
        Err(error) => {
            eprintln!("error: {error}");
            return ExitCode::from(2);
        }
    };
    let verdict = ledger.judge(&comparison);
    print(&comparison, &verdict, &ledger);
    match comparison.trip_pairing {
        None => println!("trip identity: {}", profile.trip_identity.as_str()),
        Some(pairing) => println!(
            "trip identity: {} (paired by id {}, by content {}, near {}, by dates {}; \
             unpaired A {}, B {})",
            profile.trip_identity.as_str(),
            pairing.by_id,
            pairing.by_content,
            pairing.near,
            pairing.by_dates,
            pairing.unpaired[0],
            pairing.unpaired[1]
        ),
    }
    if let Some(path) = options.report
        && let Err(error) = std::fs::write(path, report_json(&comparison, &verdict))
    {
        eprintln!("error: {}: {error}", path.display());
        return ExitCode::from(2);
    }
    let elapsed = started.elapsed().as_secs_f64();
    let outcome = if verdict.passed() { "passed" } else { "FAILED" };
    println!(
        "{outcome}: {} differences, {} categories ({elapsed:.1} s)",
        comparison.differences(),
        comparison.categories.len()
    );
    if verdict.passed() {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

fn read_ledger(path: &Path) -> Result<Ledger, String> {
    let text =
        std::fs::read_to_string(path).map_err(|error| format!("{}: {error}", path.display()))?;
    Ledger::parse(&text).map_err(|error| format!("{}: {error}", path.display()))
}

fn print(comparison: &Comparison, verdict: &Verdict, ledger: &Ledger) {
    println!(
        "{:<20} {:<10} {:>10} {:>10}",
        "relation", "mode", "rows A", "rows B"
    );
    for relation in &comparison.relations {
        println!(
            "{:<20} {:<10} {:>10} {:>10}",
            relation.name,
            relation.mode.as_str(),
            relation.rows[0],
            relation.rows[1]
        );
    }
    for item in &comparison.unreferenced {
        println!(
            "unreferenced {}: A {}, B {}",
            item.target, item.counts[0], item.counts[1]
        );
    }
    for label in &ledger.labels {
        println!(
            "not compared: {}.{}: {}",
            label.relation, label.column, label.reason
        );
    }
    let reason = |name: &str| {
        ledger
            .categories
            .iter()
            .find(|category| category.name == name)
            .map_or("", |category| category.reason.as_str())
    };
    if !verdict.defaults.is_empty() {
        println!();
        println!("categories:");
    }
    for default in &verdict.defaults {
        println!(
            "{:>10}  {:<12}  {}",
            default.count, default.status, default.category
        );
        for (assigned, bucket) in &comparison.categories[&default.category].groups {
            match assigned {
                Some(name) => println!("{:>10}    -> {name}: {}", bucket.count, reason(name)),
                None => println!("{:>10}    -> {}", bucket.count, Status::Unexplained),
            }
            for sample in &bucket.samples {
                let value = |text: &Option<String>| text.clone().unwrap_or_else(|| "null".into());
                match &sample.column {
                    Some(column) => println!(
                        "                {} | {column}: {} => {}",
                        sample.key,
                        value(&sample.a),
                        value(&sample.b)
                    ),
                    None => println!("                {}", sample.key),
                }
            }
        }
    }
    if !verdict.categories.is_empty() {
        println!();
        println!("ledger:");
    }
    for category in &verdict.categories {
        let max = category
            .max
            .map_or_else(|| "..".to_owned(), |max| format!("..{max}"));
        println!(
            "{:>10}  {:<12}  {} ({}, {}{max}): {}",
            category.count,
            category.status,
            category.name,
            category.matches,
            category.min,
            category.reason
        );
    }
}
