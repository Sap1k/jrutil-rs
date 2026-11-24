//! The pipelines: which compiler and which passes make each package, in which order.
//!
//! This is the one place the order is written down (ARCHITECTURE.md, "Shape"). A stage here is
//! a call into a library crate; nothing in this file knows a rule.

use std::path::Path;
use std::time::Instant;

use jr_enrich::{presentation, rail_coordinates, route_stops, shapes};
use jr_formats::geodata::synonyms;
use jr_geo::names::Analyzer;
use jr_geo::routing::ContextCache;
use jr_jdf::{assemble, fix, merge};
use jr_model::edit::{ApplyError, Precedence};
use jr_model::{Diagnostics, Edits, Timetable, apply};
use jr_publish::write::{FeedInfo, PublishOptions, SourceEntry, write_package};
use jr_publish::{Package, validate};
use rayon::prelude::*;
use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::spec::{
    CzpttSpec, FilteredSpec, JdfSpec, OverlaySpec, PostsSpec, ShapesSpec, TrainKeysSpec,
};
use crate::{companion, load};

/// The stop-name synonym table, shared by every road name matcher.
const STOP_SYNONYMS: &str = include_str!("../../../data/names/stop_synonyms.txt");

/// The policy and data files compiled into this binary. Their text is part of the build
/// specification: a changed threshold is a different build of the packages that read it.
const POLICIES: &[(&str, &str)] = &[
    ("czptt.toml", include_str!("../../../policy/czptt.toml")),
    ("jdf.toml", include_str!("../../../policy/jdf.toml")),
    (
        "jdf_convert.toml",
        include_str!("../../../policy/jdf_convert.toml"),
    ),
    ("jdf_fix.toml", include_str!("../../../policy/jdf_fix.toml")),
    (
        "jdf_merge.toml",
        include_str!("../../../policy/jdf_merge.toml"),
    ),
    (
        "names/road_names.toml",
        include_str!("../../../data/names/road_names.toml"),
    ),
    ("names/stop_synonyms.txt", STOP_SYNONYMS),
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
    Merge(#[from] merge::MergeError),
    #[error(transparent)]
    Assemble(#[from] assemble::AssembleError),
    #[error(transparent)]
    Presentation(#[from] presentation::Tie),
    #[error(transparent)]
    Filtered(#[from] jr_jdf::filtered::FilterError),
    #[error(transparent)]
    Overlay(#[from] jr_overlay::OverlayError),
    #[error(transparent)]
    Posts(#[from] jr_posts::EstimateError),
    #[error("post estimator policy: {0}")]
    PostPolicy(String),
    #[error("post evidence pack: {0}")]
    PostEvidence(String),
    #[error("no overlay source profile is named {0}")]
    UnknownProfile(String),
    #[error("cannot write {path}: {source}")]
    Write {
        path: String,
        source: std::io::Error,
    },
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

/// What the rail package is built with.
const CZPTT_POLICIES: &[&str] = &[
    "czptt.toml",
    "precedence.toml",
    "publish.toml",
    "rail_coordinates.toml",
];

/// What the road package is built with.
const JDF_POLICIES: &[&str] = &[
    "jdf.toml",
    "jdf_convert.toml",
    "jdf_fix.toml",
    "jdf_merge.toml",
    "names/road_names.toml",
    "names/stop_synonyms.txt",
    "precedence.toml",
    "publish.toml",
];

/// How many batches have their stops matched ahead of the merge. The merge takes batches one
/// at a time in name order; matching runs in parallel on a group of them. The size bounds what
/// is held as read and does not change the result.
const JDF_BATCH_GROUP: usize = 128;

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

/// How a build runs; nothing here changes what it writes.
#[derive(Clone, Copy, Debug)]
pub struct RunOptions {
    /// Whether the caches the specification names are read and written.
    pub use_cache: bool,
    /// Whether each finished stage is also reported as a `JRUTIL_PROGRESS` line, the form
    /// Oběhy's build reporter reads.
    pub progress_events: bool,
}

/// The prefix of a machine-readable progress line.
const PROGRESS_PREFIX: &str = "JRUTIL_PROGRESS ";

/// Reports each stage's wall time on stderr as it finishes.
struct Stages {
    package: &'static str,
    events: bool,
    started: Instant,
}

impl Stages {
    fn new(package: &'static str, options: &RunOptions) -> Self {
        Self {
            package,
            events: options.progress_events,
            started: Instant::now(),
        }
    }

    fn done(&mut self, stage: &str) {
        let seconds = self.started.elapsed().as_secs_f64();
        eprintln!("[{}] {stage}: {seconds:.1} s", self.package);
        if self.events {
            let event = serde_json::json!({
                "event": "phase",
                "stage": self.package,
                "name": stage,
                "state": "completed",
                "seconds": (seconds * 10.0).round() / 10.0,
            });
            eprintln!("{PROGRESS_PREFIX}{event}");
        }
        self.started = Instant::now();
    }
}

/// The rail package: compile, locate the railway points, connect replacement buses to their
/// trains, order the route stops, publish.
pub fn czptt(spec: &CzpttSpec, options: &RunOptions) -> Result<(), BuildError> {
    let use_cache = options.use_cache;
    let mut stages = Stages::new("czptt", options);
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

    let mut train_key_sources = Vec::new();
    let mut train_key_summaries = serde_json::Map::new();
    for source in &spec.train_keys {
        let (source_id, digest, summary) =
            train_keys(&mut timetable, source, &precedence, &mut diagnostics)?;
        train_key_summaries.insert(source_id.clone(), section(&summary));
        train_key_sources.push((source_id, digest));
    }
    if !spec.train_keys.is_empty() {
        stages.done("regional train keys");
    }

    let (edits, route_stop_order) = route_stops::route_stops(&timetable);
    pass(&mut timetable, edits, &precedence, &mut diagnostics)?;
    stages.done("route stops");

    let route_shapes = match &spec.shapes {
        Some(shapes_spec) => {
            let summary = route_shapes(
                &mut timetable,
                shapes_spec,
                use_cache,
                &precedence,
                &mut diagnostics,
            )?;
            stages.done("route shapes");
            Some(summary)
        }
        None => None,
    };

    let publish: PublishPolicy = policy("publish.toml");
    let snapshot_digest = digest_of_lines(&sources.inputs);
    let mut build_inputs: Vec<(&str, &str)> = vec![("national-czptt", &snapshot_digest)];
    let mut source_entries = vec![SourceEntry {
        source_id: "national-czptt".to_owned(),
        payload_sha256: snapshot_digest.clone(),
        payload_kind: Some("snapshot".to_owned()),
    }];
    for (source_id, digest) in &train_key_sources {
        build_inputs.push((source_id, digest));
        source_entries.push(SourceEntry {
            source_id: source_id.clone(),
            payload_sha256: digest.clone(),
            payload_kind: Some("zip".to_owned()),
        });
    }
    let options = PublishOptions {
        compiler_version: compiler_version().to_owned(),
        build_spec_sha256: build_spec_sha256(CZPTT_POLICIES, &build_inputs),
        sources: source_entries,
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
            (!train_key_summaries.is_empty()).then(|| {
                (
                    "train_keys".to_owned(),
                    serde_json::Value::Object(train_key_summaries),
                )
            }),
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

/// The road package: admit each batch and match its stops, merge the batches, apply the
/// international filter, assemble the timetable, present the routes, order the route stops,
/// publish.
pub fn jdf(spec: &JdfSpec, options: &RunOptions) -> Result<(), BuildError> {
    let use_cache = options.use_cache;
    let mut stages = Stages::new("jdf", options);
    let mut diagnostics = Diagnostics::new();
    let precedence: Precedence = policy("precedence.toml");

    let sources = load::jdf::load(spec)?;
    stages.done("read sources");

    let jdf_policy = jr_jdf::Policy::builtin();
    let fix_policy = fix::Policy::builtin();
    let synonym_lines = synonyms::parse(STOP_SYNONYMS);
    let identity_names = Analyzer::new(
        fix_policy.names.identity.clone(),
        synonym_lines.iter().map(|line| &line.alternatives),
    );
    let (catalogue_names, catalogue_rows): (Vec<String>, Vec<_>) =
        sources.catalogues.into_iter().unzip();
    let evidence = fix::Evidence::build(
        fix::EvidenceInputs {
            catalogues: catalogue_names
                .iter()
                .map(String::as_str)
                .zip(catalogue_rows)
                .collect(),
            osm_nodes: sources.osm_nodes,
            okresy: sources.okresy,
            countries: sources.countries,
            towns: sources.towns,
            foreign_towns: sources.foreign_towns,
            synonyms: synonym_lines.clone(),
        },
        &fix_policy,
        &mut diagnostics,
    );
    stages.done("stop evidence");

    let merge_policy = merge::Policy::builtin();
    let registry = match &sources.registry {
        Some(stops) => merge::Registry::new(stops, &merge_policy, &mut diagnostics),
        None => merge::Registry::empty(),
    };
    let mut merger = merge::Merger::new(
        merge::MergeContext {
            registry: &registry,
            names: &identity_names,
            jdf: &jdf_policy,
            timetable_year: spec.timetable_year,
            reference_date: spec.reference_date,
        },
        &merge_policy,
    )?;

    // A group of batches is read and has its stops matched in parallel, each batch on its
    // own; the merge then takes them in name order.
    let batch_count = sources.batches.len();
    let mut inputs = sources.inputs;
    let mut unreadable_batches = 0usize;
    let mut matched_batches = 0usize;
    for group in sources.batches.chunks(JDF_BATCH_GROUP) {
        let matched: Vec<_> = group
            .par_iter()
            .map(|file| {
                let loaded = load::jdf::read_batch(file)?;
                let mut found = Diagnostics::new();
                let unreadable = loaded.read.is_err();
                let fixed = jr_jdf::source::admit(&file.name, loaded.read, &jdf_policy, &mut found)
                    .and_then(|batch| fix::fix(batch, &evidence, &fix_policy, &mut found));
                Ok((fixed, found, unreadable, loaded.input))
            })
            .collect::<Result<_, load::LoadError>>()?;
        for (fixed, found, unreadable, input) in matched {
            diagnostics.append(found);
            inputs.push(input);
            unreadable_batches += usize::from(unreadable);
            if let Some(fixed) = fixed {
                matched_batches += 1;
                merger.add(fixed, &mut diagnostics)?;
            }
        }
    }
    drop(evidence);
    inputs.sort();
    stages.done("read batches, match stops, merge (pass 1)");

    let mut resolved = merger.resolve(&mut diagnostics);
    stages.done("line versions and calendars");

    // Pass 2: the batches that still have a published trip are read again for their calls. The
    // admission was reported in pass 1, and the input digests recorded there.
    let wanted: Vec<&load::jdf::BatchFile> = {
        let names: std::collections::BTreeSet<&str> = resolved.wanted().collect();
        sources
            .batches
            .iter()
            .filter(|file| names.contains(file.name.as_str()))
            .collect()
    };
    for group in wanted.chunks(JDF_BATCH_GROUP) {
        let read: Vec<_> = group
            .par_iter()
            .map(|file| {
                let loaded = load::jdf::read_batch(file)?;
                let mut reported = Diagnostics::new();
                Ok(jr_jdf::source::admit(
                    &file.name,
                    loaded.read,
                    &jdf_policy,
                    &mut reported,
                ))
            })
            .collect::<Result<_, load::LoadError>>()?;
        for batch in read.into_iter().flatten() {
            resolved.add_calls(batch, &mut diagnostics)?;
        }
    }
    let mut merged = resolved.finish(&mut diagnostics)?;
    let filtered = merge::apply_international(&mut merged, &jdf_policy, &mut diagnostics);
    stages.done("place calls (pass 2), international filter");

    if let Some(path) = &spec.registry_candidates {
        write_registry_candidates(path, &merged.registry_candidates).map_err(|source| {
            BuildError::Write {
                path: path.display().to_string(),
                source,
            }
        })?;
    }
    let provisional_stops = merged
        .stops
        .iter()
        .filter(|stop| stop.provisional.is_some())
        .count();
    let merge_section = serde_json::json!({
        "batches": batch_count,
        "unreadable_batches": unreadable_batches,
        "matched_batches": matched_batches,
        "stops": merged.stops.len(),
        "provisional_stops": provisional_stops,
        "registry_candidates": merged.registry_candidates.len(),
        "line_versions": merged.versions.len(),
        "reconciled": {
            "exact": merged.statistics.exact,
            "suffix": merged.statistics.suffix,
            "fuzzy": merged.statistics.fuzzy,
            "ambiguous": merged.statistics.ambiguous,
        },
        "international_filter_decisions": filtered.len(),
    });

    let assembled = assemble::assemble(
        assemble::AssembleInput {
            merged,
            transport_modes: &sources.transport_modes,
            jdf: &jdf_policy,
        },
        &assemble::Policy::builtin(),
        &mut diagnostics,
    )?;
    let mut timetable = assembled.timetable;
    stages.done("assemble");

    let mut estimated_posts = None;
    if let Some(posts) = &spec.posts {
        let summary = estimate_posts(
            &mut timetable,
            &assembled.post_facts,
            posts,
            PostsRun {
                use_cache,
                day: day_number(spec.reference_date),
            },
            spec.registry.as_deref(),
            &digest_of_lines(&inputs),
            &precedence,
            &mut diagnostics,
            &mut stages,
        )?;
        estimated_posts = Some(summary);
    }

    // JDF-R484: after the posts, so each part keeps the boarding points chosen for the trip. A
    // variant route belongs to its route's line.
    let mut lines = assembled.lines;
    if !assembled.operators.is_empty() {
        let (edits, variants) = assemble::operators::edits(&timetable, &assembled.operators);
        pass(&mut timetable, edits, &precedence, &mut diagnostics)?;
        for (route, variant) in variants {
            let of_route: Vec<_> = lines
                .iter()
                .filter(|line| line.route == Some(route))
                .cloned()
                .collect();
            lines.extend(of_route.into_iter().map(|mut line| {
                line.route = Some(variant);
                line
            }));
        }
    }

    let (edits, mut route_presentation) =
        presentation::route_presentation(&timetable, &sources.presentation, &mut diagnostics)?;
    pass(&mut timetable, edits, &precedence, &mut diagnostics)?;

    // The filtered feed is the road timetable as it stands before the overlay and the shapes.
    let before_overlay = (spec.filtered.is_some()
        && (spec.overlay.is_some() || spec.shapes.is_some()))
    .then(|| timetable.clone());
    let mut overlay_sources = Vec::new();
    let mut overlay_summary = None;
    if let Some(overlay) = &spec.overlay {
        let (feeds, summary) = regional_overlay(
            &mut timetable,
            overlay,
            spec.registry.as_deref(),
            &synonym_lines,
            &precedence,
            &mut diagnostics,
        )?;
        overlay_sources = feeds;
        overlay_summary = Some(summary);
        stages.done("overlay");
        // What the overlay brought is presented like the rest.
        let (edits, again) =
            presentation::route_presentation(&timetable, &sources.presentation, &mut diagnostics)?;
        pass(&mut timetable, edits, &precedence, &mut diagnostics)?;
        route_presentation.overridden_routes += again.overridden_routes;
        for (field, count) in again.by_field {
            *route_presentation.by_field.entry(field).or_default() += count;
        }
    }
    let (edits, route_stop_order) = route_stops::route_stops(&timetable);
    pass(&mut timetable, edits, &precedence, &mut diagnostics)?;
    stages.done("presentation and route stops");

    let route_shapes = match &spec.shapes {
        Some(shapes_spec) => {
            let summary = route_shapes(
                &mut timetable,
                shapes_spec,
                use_cache,
                &precedence,
                &mut diagnostics,
            )?;
            stages.done("route shapes");
            Some(summary)
        }
        None => None,
    };

    let publish: PublishPolicy = policy("publish.toml");
    let inputs_digest = digest_of_lines(&inputs);
    let year = spec.timetable_year.to_string();
    let reference = spec.reference_date.to_string();
    let mut build_inputs: Vec<(&str, &str)> = vec![
        ("national-jdf", &inputs_digest),
        ("timetable-year", &year),
        ("reference-date", &reference),
    ];
    let mut source_entries = vec![SourceEntry {
        source_id: "national-jdf".to_owned(),
        payload_sha256: inputs_digest.clone(),
        payload_kind: Some("snapshot".to_owned()),
    }];
    for (source_id, digest) in &overlay_sources {
        build_inputs.push((source_id, digest));
        source_entries.push(SourceEntry {
            source_id: source_id.clone(),
            payload_sha256: digest.clone(),
            payload_kind: Some("zip".to_owned()),
        });
    }
    let options = PublishOptions {
        compiler_version: compiler_version().to_owned(),
        build_spec_sha256: build_spec_sha256(JDF_POLICIES, &build_inputs),
        sources: source_entries,
        feed_info: FeedInfo {
            publisher_name: publish.publisher_name,
            publisher_url: publish.publisher_url,
            lang: publish.lang,
            contact_email: publish.contact_email,
        },
        approximate_suffix: publish.approximate_suffix,
        gtfs_timed_transfers_as_stop_pairs: publish.gtfs_timed_transfers_as_stop_pairs,
        extra_sections: [
            Some(("road_merge".to_owned(), merge_section)),
            Some(("road".to_owned(), section(&assembled.summary))),
            estimated_posts.map(|summary| ("estimated_posts".to_owned(), summary)),
            overlay_summary.map(|summary| ("overlay".to_owned(), summary)),
            Some((
                "route_presentation".to_owned(),
                section(&route_presentation),
            )),
            Some(("route_stop_order".to_owned(), section(&route_stop_order))),
            route_shapes.map(|summary| ("route_shapes".to_owned(), section(&summary))),
        ]
        .into_iter()
        .flatten()
        .collect(),
        // A batch that could not be read is missing lines (JDF-R004).
        publication_eligible: unreadable_batches == 0,
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

    if let Some(filtered) = &spec.filtered {
        filtered_feed(
            before_overlay.unwrap_or(timetable),
            &lines,
            filtered,
            spec.reference_date,
            &options,
            &precedence,
        )?;
        stages.done("filtered feed");
    }
    Ok(())
}

/// Boarding posts estimated for the road timetable: the routing graph is built from the
/// extract, the estimator returns edits, the evidence and review reports. Returns the summary
/// for diagnostics.json.
#[allow(clippy::too_many_arguments)]
fn estimate_posts(
    timetable: &mut Timetable,
    facts: &jr_jdf::assemble::PostFacts,
    spec: &PostsSpec,
    run: PostsRun,
    registry: Option<&Path>,
    inputs_digest: &str,
    precedence: &Precedence,
    diagnostics: &mut Diagnostics,
    stages: &mut Stages,
) -> Result<serde_json::Value, BuildError> {
    use jr_formats::post_evidence::{self, PackIdentity, RouterEvidence};

    let routing = jr_geo::routing::RoutingPolicy::builtin();
    let document = load::posts::policy(&spec.policy)?;
    let policy = jr_posts::evaluate::EvaluationPolicy::from_document(&document)
        .map_err(|error| BuildError::PostPolicy(error.to_string()))?;
    let registry_posts = match registry {
        Some(directory) => load::posts::registry_posts(directory)?,
        None => Vec::new(),
    };
    let cache_file = spec
        .cache
        .as_ref()
        .filter(|_| run.use_cache)
        .map(|directory| {
            directory.join(format!(
                "{CACHE_PREFIX}{}{CACHE_SUFFIX}",
                ContextCache::identity(&routing)
            ))
        });
    let mut cache = cache_file
        .as_deref()
        .map(|path| load::posts::routing_cache(path, run.day));
    let (graph, routing_sha256) = load::posts::graph(&spec.routing, &routing)?;
    stages.done("routing graph");

    let estimated = jr_posts::estimate(
        jr_posts::EstimateInput {
            timetable,
            facts,
            graph,
            routing: &routing,
            policy: &policy,
            registry: &registry_posts,
            identity: PackIdentity {
                capture_tool_version: compiler_version().to_owned(),
                merged_jdf_sha256: inputs_digest.to_owned(),
                routing_pbf_sha256: routing_sha256,
                source_id: "national-jdf".to_owned(),
                router: RouterEvidence::Rust,
            },
            cache: cache.as_mut(),
        },
        diagnostics,
    )?;
    if let (Some(cache), Some(path)) = (&cache, &cache_file) {
        save_routing_cache(path, cache, spec.cache_retention_days)?;
    }
    pass(timetable, estimated.edits, precedence, diagnostics)?;

    let io = |path: &Path| {
        let path = path.display().to_string();
        move |source| BuildError::Write { path, source }
    };
    if let (Some(directory), Some(pack)) = (&spec.evidence, &estimated.pack) {
        if directory.exists() {
            return Err(io(directory)(std::io::Error::other(
                "the evidence directory exists",
            )));
        }
        let files = post_evidence::write(pack)
            .map_err(|error| BuildError::PostEvidence(error.to_string()))?;
        std::fs::create_dir_all(directory).map_err(io(directory))?;
        let contents = [
            &files.manifest,
            &files.observations,
            &files.route_points,
            &files.contexts,
            &files.corridor_variants,
            &files.route_point_evidence,
        ];
        for (name, bytes) in post_evidence::FILES.iter().zip(contents) {
            let path = directory.join(name);
            std::fs::write(&path, bytes).map_err(io(&path))?;
        }
    }
    if let Some(directory) = &spec.reports {
        write_reports(directory, &estimated.reports)?;
    }
    stages.done("post estimator");
    if let Some(cache) = &cache {
        let counts = cache.counts(spec.cache_retention_days);
        eprintln!(
            "[jdf] routing cache: {} reused, {} invalidated, {} routed, {} kept",
            counts.reused, counts.invalidated, counts.routed, counts.kept
        );
    }
    Ok(section(&estimated.summary))
}

/// How the posts stage runs: with or without its cache, and on which day.
#[derive(Clone, Copy, Debug)]
struct PostsRun {
    use_cache: bool,
    /// The build's reference date as days since 1970-01-01: the routing cache's clock.
    day: i32,
}

fn day_number(date: jr_model::Date) -> i32 {
    date.since(jr_model::Date::constant(1970, 1, 1))
        .map_or(0, |span| span.get_days())
}

/// The routing cache's file name around its identity.
const CACHE_PREFIX: &str = "routing-contexts-";
const CACHE_SUFFIX: &str = ".bin";

/// Writes the routing cache under a temporary name and renames it into place; the caches of
/// other identities in the directory are removed.
fn save_routing_cache(
    path: &Path,
    cache: &ContextCache,
    retention_days: u32,
) -> Result<(), BuildError> {
    let io = |path: &Path| {
        let path = path.display().to_string();
        move |source| BuildError::Write { path, source }
    };
    let directory = path.parent().unwrap_or(Path::new("."));
    std::fs::create_dir_all(directory).map_err(io(directory))?;
    let temporary = path.with_extension("bin.tmp");
    std::fs::write(&temporary, cache.to_bytes(retention_days)).map_err(io(&temporary))?;
    std::fs::rename(&temporary, path).map_err(io(path))?;
    let name = path.file_name().and_then(|name| name.to_str());
    for entry in std::fs::read_dir(directory).map_err(io(directory))? {
        let entry = entry.map_err(io(directory))?;
        let other = entry.file_name();
        let Some(other) = other.to_str() else {
            continue;
        };
        if other.starts_with(CACHE_PREFIX) && other.ends_with(CACHE_SUFFIX) && Some(other) != name {
            std::fs::remove_file(entry.path()).map_err(io(&entry.path()))?;
        }
    }
    Ok(())
}

/// Review reports, one CSV each, in a directory.
fn write_reports(directory: &Path, reports: &[jr_model::ReportTable]) -> Result<(), BuildError> {
    let io = |path: &Path| {
        let path = path.display().to_string();
        move |source| BuildError::Write { path, source }
    };
    std::fs::create_dir_all(directory).map_err(io(directory))?;
    for report in reports {
        let path = directory.join(format!("{}.csv", report.name));
        let write = || -> Result<(), csv::Error> {
            let mut writer = csv::WriterBuilder::new()
                .terminator(csv::Terminator::Any(b'\n'))
                .from_path(&path)?;
            writer.write_record(&report.header)?;
            for row in &report.rows {
                writer.write_record(row)?;
            }
            Ok(writer.flush()?)
        };
        write().map_err(|error| io(&path)(std::io::Error::other(error)))?;
    }
    Ok(())
}

/// Regional feeds overlaid on the road timetable: each source is read with what its profile
/// asks for, the engine returns edits and review reports. Returns each source's ID and the
/// SHA-256 of its feed, in source order, and the summary for diagnostics.json.
/// OVL-R132 for one regional feed: its source ID, the feed's digest and what the pass found.
fn train_keys(
    timetable: &mut Timetable,
    spec: &TrainKeysSpec,
    precedence: &Precedence,
    diagnostics: &mut Diagnostics,
) -> Result<(String, String, jr_overlay::train_keys::Summary), BuildError> {
    let profile = jr_overlay::profile::SourceProfile::builtin(&spec.profile)
        .ok_or_else(|| BuildError::UnknownProfile(spec.profile.clone()))?
        .map_err(jr_overlay::OverlayError::from)?;
    let (feed, digest) = load::gtfs::feed(&spec.feed, &profile.reader())?;
    let (edits, summary) =
        jr_overlay::train_keys::train_keys(timetable, &profile, &feed, diagnostics);
    pass(timetable, edits, precedence, diagnostics)?;
    Ok((profile.source_id.to_string(), digest, summary))
}

fn regional_overlay(
    timetable: &mut Timetable,
    spec: &OverlaySpec,
    registry: Option<&Path>,
    synonym_lines: &[synonyms::Line],
    precedence: &Precedence,
    diagnostics: &mut Diagnostics,
) -> Result<(Vec<(String, String)>, serde_json::Value), BuildError> {
    use jr_overlay::profile::{BaseProfile, SourceProfile};

    let mut loaded = Vec::with_capacity(spec.sources.len());
    for source in &spec.sources {
        let profile = SourceProfile::builtin(&source.profile)
            .ok_or_else(|| BuildError::UnknownProfile(source.profile.clone()))?
            .map_err(jr_overlay::OverlayError::from)?;
        let (feed, digest) = load::gtfs::feed(&source.feed, &profile.reader())?;
        let overrides = match &source.overrides {
            Some(path) => load::gtfs::stop_overrides(path)?,
            None => Vec::new(),
        };
        loaded.push((profile, feed, overrides, digest));
    }
    loaded.sort_by(|a, b| a.0.source_id.cmp(&b.0.source_id));
    let digests: Vec<(String, String)> = loaded
        .iter()
        .map(|(profile, _, _, digest)| (profile.source_id.to_string(), digest.clone()))
        .collect();
    let places = match registry {
        Some(directory) => load::gtfs::overlay_places(directory)?,
        None => Vec::new(),
    };
    let base_profile = BaseProfile::road().map_err(jr_overlay::OverlayError::from)?;
    let profiles: Vec<SourceProfile> = loaded.iter().map(|(profile, ..)| profile.clone()).collect();
    let sources = loaded
        .into_iter()
        .zip(&profiles)
        .map(
            |((_, feed, overrides, _), profile)| jr_overlay::SourceInput {
                profile,
                feed,
                overrides,
            },
        )
        .collect();
    let overlaid = jr_overlay::overlay(
        jr_overlay::OverlayInput {
            base: timetable,
            base_profile: &base_profile,
            sources,
            places: &places,
            synonyms: synonym_lines,
        },
        diagnostics,
    )?;
    pass(timetable, overlaid.edits, precedence, diagnostics)?;

    if let Some(directory) = &spec.reports {
        write_reports(directory, &overlaid.reports)?;
    }
    Ok((digests, section(&overlaid.summary)))
}

/// The filtered road feed: the published road timetable without the lines other feeds publish
/// and without calls at unplaced stops, as GTFS, with the line selection and the counts.
fn filtered_feed(
    mut timetable: Timetable,
    lines: &[jr_jdf::filtered::LineVersion],
    spec: &FilteredSpec,
    reference: jr_model::Date,
    options: &PublishOptions,
    precedence: &Precedence,
) -> Result<(), BuildError> {
    let (rules, rules_sha256) = load::jdf::filter_rules(&spec.rules)?;
    let filtered = jr_jdf::filtered::filtered_feed(&timetable, lines, &rules, reference)?;
    let mut diagnostics = Diagnostics::new();
    pass(&mut timetable, filtered.edits, precedence, &mut diagnostics)?;
    // The route stops are those of the trips and calls that remain.
    let (edits, _) = route_stops::route_stops(&timetable);
    pass(&mut timetable, edits, precedence, &mut diagnostics)?;

    let io = |path: &Path| {
        let path = path.display().to_string();
        move |source| BuildError::Write { path, source }
    };
    if spec.output.exists() {
        return Err(io(&spec.output)(std::io::Error::other(
            "the output directory exists",
        )));
    }
    std::fs::create_dir_all(&spec.output).map_err(io(&spec.output))?;
    let gtfs = spec.output.join("gtfs.zip");
    jr_publish::write::write_gtfs(&timetable, options, &gtfs)?;
    let (gtfs_sha256, gtfs_bytes) = load::jdf::written(&gtfs)?;

    let json = |value: &serde_json::Value| {
        let mut text = serde_json::to_string_pretty(value).expect("plain data");
        text.push('\n');
        text
    };
    let snapshot = serde_json::json!({
        "schema_version": 3,
        "source": "jdf-linext",
        "reference_date": reference.to_string(),
        "queries": filtered.queries,
    });
    let snapshot_path = spec.output.join("line-snapshot.json");
    std::fs::write(&snapshot_path, json(&snapshot)).map_err(io(&snapshot_path))?;
    let mut report = serde_json::json!({
        "schema_version": 3,
        "rules": {
            "path": spec.rules.file_name().map(|name| name.to_string_lossy()),
            "sha256": rules_sha256,
        },
        "gtfs": { "bytes": gtfs_bytes, "sha256": gtfs_sha256 },
    });
    if let (Some(report), serde_json::Value::Object(counts)) =
        (report.as_object_mut(), section(&filtered.report))
    {
        report.extend(counts);
    }
    let report_path = spec.output.join("filter-report.json");
    std::fs::write(&report_path, json(&report)).map_err(io(&report_path))?;
    Ok(())
}

/// What the stop registry is missing, in the form its review tooling reads (JDF-R232–R234):
/// the provisional stops, then the new spellings of registered ones.
fn write_registry_candidates(
    path: &Path,
    candidates: &[merge::RegistryCandidate],
) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut writer = csv::WriterBuilder::new()
        .terminator(csv::Terminator::Any(b'\n'))
        .from_path(path)?;
    writer.write_record([
        "provisional_id",
        "town",
        "district",
        "nearby_place",
        "okres",
        "country",
        "lat",
        "lon",
        "reason",
        "alias_of_suggestion",
    ])?;
    for candidate in candidates {
        let text = |value: &Option<Box<str>>| value.as_deref().unwrap_or("").to_owned();
        let coordinate = |value: Option<f64>| value.map_or(String::new(), |v| format!("{v:.6}"));
        let name = &candidate.spelling;
        writer.write_record([
            candidate
                .provisional_number
                .map_or(String::new(), |number| number.to_string()),
            name.town.to_string(),
            text(&name.district),
            text(&name.nearby_place),
            text(&name.okres),
            text(&name.country),
            coordinate(candidate.location.map(|coord| coord.lat)),
            coordinate(candidate.location.map(|coord| coord.lon)),
            candidate.reason.to_string(),
            candidate
                .suggestions
                .iter()
                .map(u64::to_string)
                .collect::<Vec<_>>()
                .join(";"),
        ])?;
    }
    writer.flush()
}

/// Shapes are best effort: a companion that cannot run leaves trips unshaped, never fails the
/// build.
fn route_shapes(
    timetable: &mut Timetable,
    spec: &ShapesSpec,
    use_cache: bool,
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
            spec.cache.as_deref().filter(|_| use_cache),
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
fn build_spec_sha256(policies: &[&str], inputs: &[(&str, &str)]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(format!("compiler={}\n", compiler_version()));
    for (name, text) in POLICIES {
        if policies.contains(name) {
            hasher.update(format!("policy/{name}={}\n", hex(Sha256::digest(text))));
        }
    }
    for (source, digest) in inputs {
        hasher.update(format!("input/{source}={digest}\n"));
    }
    hex(hasher.finalize())
}
