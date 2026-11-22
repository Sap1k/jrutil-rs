//! The crate graph is the layering contract (AGENTS.md, "Layering"): dependencies between the
//! workspace crates point one way, and only `jr-publish` and `jrutil` touch the file system.
//! The graph is checked over the transitive dependencies `Cargo.lock` records (dev and build
//! dependencies included, so if anything stricter than needed); the sources are scanned for the
//! standard library's own I/O paths.

use std::collections::{BTreeMap, BTreeSet};

/// Lower layers first. A crate may depend only on crates listed before it.
const LAYERS: &[&str] = &[
    "jr-model",
    "jr-formats",
    "jr-geo",
    "jr-czptt",
    "jr-jdf",
    "jr-enrich",
    "jr-posts",
    "jr-overlay",
    "jr-publish",
    "jrutil",
];

/// The crates that may touch the file system: the package writer and the command line. Every
/// other crate decodes bytes or readers it is handed and computes.
const SHELL: &[&str] = &["jr-publish", "jrutil"];

/// Crates that exist to reach the file system or the network.
const IO: &[&str] = &[
    "tokio",
    "mio",
    "socket2",
    "hyper",
    "reqwest",
    "ureq",
    "rustls",
    "native-tls",
    "openssl",
    "walkdir",
    "tempfile",
    "memmap2",
    "notify",
];

/// What a crate outside the shell must not name in its sources.
const FORBIDDEN_PATHS: &[&str] = &["std::fs", "std::net", "std::process", "std::env"];

fn graph() -> BTreeMap<String, Vec<String>> {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../Cargo.lock");
    let lock: toml::Table = std::fs::read_to_string(path)
        .expect("Cargo.lock")
        .parse()
        .expect("TOML");
    let mut graph = BTreeMap::new();
    for package in lock["package"].as_array().expect("packages") {
        let name = package["name"].as_str().unwrap().to_owned();
        let deps = package
            .get("dependencies")
            .and_then(|deps| deps.as_array())
            .map(|deps| {
                deps.iter()
                    .map(|dep| dep.as_str().unwrap().split(' ').next().unwrap().to_owned())
                    .collect()
            })
            .unwrap_or_default();
        graph
            .entry(name)
            .or_insert_with(Vec::new)
            .extend::<Vec<String>>(deps);
    }
    graph
}

fn closure(graph: &BTreeMap<String, Vec<String>>, root: &str) -> BTreeSet<String> {
    let mut seen = BTreeSet::new();
    let mut stack = vec![root.to_owned()];
    while let Some(name) = stack.pop() {
        for dep in graph.get(&name).into_iter().flatten() {
            if seen.insert(dep.clone()) {
                stack.push(dep.clone());
            }
        }
    }
    seen
}

#[test]
fn workspace_crates_depend_only_on_lower_layers() {
    let graph = graph();
    for (level, name) in LAYERS.iter().enumerate() {
        let reachable = closure(&graph, name);
        for higher in &LAYERS[level..] {
            assert!(
                !reachable.contains(*higher),
                "{name} must not depend on {higher}"
            );
        }
    }
}

#[test]
fn only_the_shell_reaches_io_crates() {
    let graph = graph();
    for name in LAYERS.iter().filter(|name| !SHELL.contains(name)) {
        let reachable = closure(&graph, name);
        for io in IO {
            assert!(!reachable.contains(*io), "{name} must not depend on {io}");
        }
    }
}

#[test]
fn only_the_shell_names_the_file_system() {
    fn scan(dir: &std::path::Path, found: &mut Vec<String>) {
        for entry in std::fs::read_dir(dir).expect("source directory") {
            let path = entry.expect("directory entry").path();
            if path.is_dir() {
                scan(&path, found);
            } else if path.extension().is_some_and(|extension| extension == "rs") {
                let text = std::fs::read_to_string(&path).expect("source file");
                for forbidden in FORBIDDEN_PATHS {
                    if text.contains(forbidden) {
                        found.push(format!("{} names {forbidden}", path.display()));
                    }
                }
            }
        }
    }
    let crates = std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/.."));
    let mut found = Vec::new();
    for name in LAYERS.iter().filter(|name| !SHELL.contains(name)) {
        scan(&crates.join(name).join("src"), &mut found);
    }
    assert!(found.is_empty(), "{found:#?}");
}
