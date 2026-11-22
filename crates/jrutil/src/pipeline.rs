//! The pipelines: which compiler and which passes make each package, in which order.
//!
//! This is the one place the order is written down (ARCHITECTURE.md, "Shape"). A stage here is
//! a call into a library crate; nothing in this file knows a rule.

use std::path::Path;
use std::time::Instant;

use jr_enrich::{rail_coordinates, route_stops, shapes};
use jr_model::edit::{ApplyError, Precedence};
use jr_model::{Diagnostics, Edits, Timetable, apply};
use jr_publish::write::{FeedInfo, PublishOptions, SourceEntry, write_package};
use jr_publish::{Package, validate};
use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::spec::{CzpttSpec, ShapesSpec};
use crate::{companion, load};

/// The policy files compiled into this binary. Their text is part of the build specification:
/// a changed threshold is a different build.
const POLICIES: &[(&str, &str)] = &[
    ("czptt.toml", include_str!("../../../policy/czptt.toml")),
    (
        "precedence.toml",
        include_str!("../../../policy/precedence.toml"),
    ),
    ("publish.toml", include_str!("../../../policy/publish.toml")),
    (
        "rail_coordinates.toml",
        include_str!("../../../policy/rail_coordinates.toml"),
    ),
];

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PublishPolicy {
    publisher_name: String,
    publisher_url: String,
    lang: String,
    contact_email: Option<String>,
    approximate_suffix: String,
    gtfs_timed_transfers_as_stop_pairs: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum BuildError {
    #[error(transparent)]
    Load(#[from] load::LoadError),
    #[error(transparent)]
    Compile(#[from] jr_czptt::CompileError),
    #[error(transparent)]
    Apply(#[from] ApplyError),
    #[error(transparent)]
    Publish(#[from] jr_publish::write::PublishError),
    #[error(transparent)]
    Package(#[from] jr_publish::PackageError),
    #[error("the package written to {path} is not valid: {first} ({count} findings)")]
    Invalid {
        path: String,
        first: String,
        count: usize,
    },
}

/// The version recorded in every manifest: the release tag or commit the binary was built
/// from when the build supplies one, else the crate version.
pub fn compiler_version() -> &'static str {
    option_env!("JRUTIL_BUILD_VERSION").unwrap_or(env!("CARGO_PKG_VERSION"))
}

fn policy<T: for<'de> Deserialize<'de>>(name: &str) -> T {
    let (_, text) = POLICIES
        .iter()
        .find(|(file, _)| *file == name)
        .expect("a compiled-in policy");
    toml::from_str(text).unwrap_or_else(|error| panic!("policy/{name}: {error}"))
}

/// Reports each stage's wall time on stderr as it finishes.
struct Stages {
    package: &'static str,
    started: Instant,
}

impl Stages {
    fn new(package: &'static str) -> Self {
        Self {
            package,
            started: Instant::now(),
        }
    }

    fn done(&mut self, stage: &str) {
        eprintln!(
            "[{}] {stage}: {:.1} s",
            self.package,
            self.started.elapsed().as_secs_f64()
        );
        self.started = Instant::now();
    }
}

/// The rail package: compile, locate the railway points, connect replacement buses to their
/// trains, order the route stops, publish.
pub fn czptt(spec: &CzpttSpec) -> Result<(), BuildError> {
    let mut stages = Stages::new("czptt");
    let mut diagnostics = Diagnostics::new();
    let precedence: Precedence = policy("precedence.toml");

    let sources = load::czptt::load(&spec.snapshot)?;
    let nodes = load::osm::tagged_nodes(&spec.osm)?;
    stages.done("read sources");

    let coordinates_policy = rail_coordinates::Policy::builtin();
    let evidence = rail_coordinates::Evidence::build(
        &sources.sr70,
        nodes,
        &coordinates_policy,
        &mut diagnostics,
    );
    let rail_policy = jr_czptt::Policy::builtin();
    let year = spec.timetable_year.unwrap_or(sources.timetable_year);
    let compiled = jr_czptt::compile(
        sources.messages,
        &sources.catalog,
        &evidence.point_names(),
        year,
        &rail_policy,
        &mut diagnostics,
    )?;
    let mut timetable = compiled.timetable;
    stages.done("compile");

    let (edits, coordinates) = rail_coordinates::rail_coordinates(
        &timetable,
        &evidence,
        &coordinates_policy,
        &mut diagnostics,
    );
    pass(&mut timetable, edits, &precedence, &mut diagnostics)?;
    stages.done("coordinates");

    let edits = jr_czptt::connections::nad_connections(&timetable, &rail_policy, &mut diagnostics);
    pass(&mut timetable, edits, &precedence, &mut diagnostics)?;
    stages.done("replacement-bus connections");

    let (edits, route_stop_order) = route_stops::route_stops(&timetable);
    pass(&mut timetable, edits, &precedence, &mut diagnostics)?;
    stages.done("route stops");

    let route_shapes = match &spec.shapes {
        Some(shapes_spec) => {
            let summary = route_shapes(&mut timetable, shapes_spec, &precedence, &mut diagnostics)?;
            stages.done("route shapes");
            Some(summary)
        }
        None => None,
    };

    let publish: PublishPolicy = policy("publish.toml");
    let snapshot_digest = digest_of_lines(&sources.inputs);
    let options = PublishOptions {
        compiler_version: compiler_version().to_owned(),
        build_spec_sha256: build_spec_sha256(&[("national-czptt", &snapshot_digest)]),
        sources: vec![SourceEntry {
            source_id: "national-czptt".to_owned(),
            payload_sha256: snapshot_digest,
            payload_kind: Some("snapshot".to_owned()),
        }],
        feed_info: FeedInfo {
            publisher_name: publish.publisher_name,
            publisher_url: publish.publisher_url,
            lang: publish.lang,
            contact_email: publish.contact_email,
        },
        approximate_suffix: publish.approximate_suffix,
        gtfs_timed_transfers_as_stop_pairs: publish.gtfs_timed_transfers_as_stop_pairs,
        extra_sections: [
            Some(("rail".to_owned(), section(&compiled.summary))),
            Some(("coordinates".to_owned(), section(&coordinates))),
            Some(("route_stop_order".to_owned(), section(&route_stop_order))),
            route_shapes.map(|summary| ("route_shapes".to_owned(), section(&summary))),
        ]
        .into_iter()
        .flatten()
        .collect(),
        // A passenger station nobody could place must not reach riders (CZPTT verdict Q18).
        publication_eligible: coordinates.unresolved_passenger_places == 0,
    };
    if let Some(parent) = spec.output.parent() {
        std::fs::create_dir_all(parent).map_err(|source| load::LoadError::Read {
            path: parent.to_owned(),
            source,
        })?;
    }
    write_package(&timetable, &diagnostics, &options, &spec.output)?;
    stages.done("publish");

    check(&spec.output)?;
    stages.done("validate");
    Ok(())
}

/// Shapes are best effort: a companion that cannot run leaves trips unshaped, never fails the
/// build.
fn route_shapes(
    timetable: &mut Timetable,
    spec: &ShapesSpec,
    precedence: &Precedence,
    diagnostics: &mut Diagnostics,
) -> Result<shapes::Summary, ApplyError> {
    let plan = shapes::Plan::of(timetable);
    let output = if plan.is_empty() {
        Err("no pattern to route".to_owned())
    } else {
        companion::run(
            &spec.companion,
            &spec.graph,
            spec.cache.as_deref(),
            &plan.patterns_csv(),
        )
    };
    let (edits, summary) = shapes::shapes(
        timetable,
        &plan,
        output.as_ref().map_err(String::as_str),
        diagnostics,
    );
    pass(timetable, edits, precedence, diagnostics)?;
    Ok(summary)
}

fn pass(
    timetable: &mut Timetable,
    edits: Edits,
    precedence: &Precedence,
    diagnostics: &mut Diagnostics,
) -> Result<(), ApplyError> {
    apply(timetable, edits, precedence, diagnostics)
}

/// A package is only left in place if it satisfies the contract.
fn check(output: &Path) -> Result<(), BuildError> {
    let findings = validate(&Package::open(output)?)?;
    match findings.first() {
        None => Ok(()),
        Some(first) => Err(BuildError::Invalid {
            path: output.display().to_string(),
            first: first.to_string(),
            count: findings.len(),
        }),
    }
}

fn section<T: serde::Serialize>(summary: &T) -> serde_json::Value {
    serde_json::to_value(summary).expect("a summary is plain data")
}

fn hex(digest: impl AsRef<[u8]>) -> String {
    digest
        .as_ref()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// SHA-256 of `name=sha256` lines: the identity of a set of files.
fn digest_of_lines(entries: &[(String, String)]) -> String {
    let mut hasher = Sha256::new();
    for (name, sha256) in entries {
        hasher.update(format!("{name}={sha256}\n"));
    }
    hex(hasher.finalize())
}

/// What a build is made from: the compiler version, the policy files and the inputs. Two
/// builds with the same specification produce the same bytes.
fn build_spec_sha256(inputs: &[(&str, &str)]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(format!("compiler={}\n", compiler_version()));
    for (name, text) in POLICIES {
        hasher.update(format!("policy/{name}={}\n", hex(Sha256::digest(text))));
    }
    for (source, digest) in inputs {
        hasher.update(format!("input/{source}={digest}\n"));
    }
    hex(hasher.finalize())
}
