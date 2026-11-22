//! The route-shapes companion process (`docs/rules/serving.md`, sections 8.2 and 8.3).
//!
//! `obehy-motis-shapes route` reads `patterns.csv` and writes its answer into a directory.
//! This module only runs it and hands the files back as text; `jr_enrich::shapes` decides what
//! they mean.

use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use jr_enrich::shapes::CompanionOutput;

/// Runs `<companion> route --graph <graph> --patterns <dir>/patterns.csv --out <dir>/out [--cache <cache>]`
/// in a fresh temporary directory, forwards its output lines to stderr prefixed `[shapes] `, reads
/// the output files and removes the directory. Any failure is `Err(text)`.
pub fn run(
    companion: &Path,
    graph: &Path,
    cache: Option<&Path>,
    patterns_csv: &str,
) -> Result<CompanionOutput, String> {
    let work = WorkDir::create()?;
    let patterns = work.0.join("patterns.csv");
    let out = work.0.join("out");
    std::fs::write(&patterns, patterns_csv)
        .map_err(|error| format!("cannot write {}: {error}", patterns.display()))?;

    let mut command = Command::new(companion);
    command
        .arg("route")
        .arg("--graph")
        .arg(graph)
        .arg("--patterns")
        .arg(&patterns)
        .arg("--out")
        .arg(&out);
    if let Some(cache) = cache {
        command.arg("--cache").arg(cache);
    }
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("cannot start {}: {error}", companion.display()))?;
    let stdout = child.stdout.take().expect("stdout is piped");
    let stderr = child.stderr.take().expect("stderr is piped");
    let status = std::thread::scope(|scope| {
        scope.spawn(|| forward(stdout));
        scope.spawn(|| forward(stderr));
        child.wait()
    })
    .map_err(|error| format!("waiting for {}: {error}", companion.display()))?;
    if !status.success() {
        return Err(format!("{} failed: {status}", companion.display()));
    }

    let read = |name: &str| {
        let path = out.join(name);
        std::fs::read_to_string(&path)
            .map_err(|error| format!("cannot read {}: {error}", path.display()))
    };
    let manifest_path = out.join("manifest.json");
    let manifest = if manifest_path.is_file() {
        let text = read("manifest.json")?;
        Some(
            serde_json::from_str(&text)
                .map_err(|error| format!("{} is not JSON: {error}", manifest_path.display()))?,
        )
    } else {
        None
    };
    Ok(CompanionOutput {
        pattern_status: read("pattern_status.csv")?,
        shapes: read("shapes.csv")?,
        offsets: read("offsets.csv")?,
        manifest,
    })
}

/// Copies the child's output to stderr line by line.
fn forward(stream: impl Read) {
    let mut reader = BufReader::new(stream);
    let mut line = Vec::new();
    loop {
        line.clear();
        match reader.read_until(b'\n', &mut line) {
            Ok(0) | Err(_) => return,
            Ok(_) => {
                let text = String::from_utf8_lossy(&line);
                eprintln!("[shapes] {}", text.trim_end_matches(['\r', '\n']));
            }
        }
    }
}

/// A temporary directory of this process, removed when dropped.
struct WorkDir(PathBuf);

impl WorkDir {
    fn create() -> Result<Self, String> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |elapsed| elapsed.as_nanos());
        let path = std::env::temp_dir().join(format!(
            "jrutil-route-shapes-{}-{nanos}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        // `create_dir`, not `create_dir_all`: an existing directory is not ours.
        std::fs::create_dir(&path)
            .map_err(|error| format!("cannot create {}: {error}", path.display()))?;
        Ok(Self(path))
    }
}

impl Drop for WorkDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
