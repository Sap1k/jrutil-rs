# AGENTS.md

jrutil-rs compiles Czech timetable sources (JDF, CZPTT, regional and foreign GTFS, curated data)
into the `jrutil-production` packages Oběhy serves. It replaces the F# JrUtil
(`E:/Git/obehy/jrutil`), which is the behavioural reference until each package has switched.
`ARCHITECTURE.md` is the design; this file is the rules. Read both before changing anything.

## The rules

1. **One model.** Every stage reads and writes `jr_model::Timetable`. No stage parses another
   stage's output. GTFS, Parquet, JDF text and XML exist only in `jr-formats` and `jr-publish`.
2. **Facts, then decisions.** `jr-formats` turns bytes into typed source facts and decides
   nothing: no thresholds, no defaults that hide a missing value, no repairs. Compilers and passes
   decide. `jr-publish` serialises and decides nothing.
3. **Passes return edits.** A pass is a function of `(&Timetable, evidence, policy)` returning
   `jr_model::Edits`. Only `jr_model::apply` changes a timetable. It checks invariants, resolves
   conflicts by the precedence table and records which pass and rule set each value.
4. **Every special case is a named rule.** A rule has an ID (`CZPTT-R007`), a `Rule` constant with
   a one-line summary, a doc comment giving the source-data reason, its thresholds in policy
   TOML (`policy/`), a diagnostic code when it fires, a test whose name contains the ID, and a row
   in `docs/rules/<area>.md`. A special case without all of these is a defect. Word lists,
   abbreviation tables and name exceptions are data files (`data/`, or the reviewed
   `jrunify-ext-geodata` checkout), never code.
5. **One implementation per concept.** One name normaliser, one distance function
   (`jr_model::geo`), one mode classifier, one calendar type (`DateSet`), one interval resolver,
   one ID builder (`jr_model::identity`). Before writing a helper, look for it.
6. **IDs are opaque.** Public IDs are built only in `jr_model::identity` and are never parsed.
   Whatever a consumer needs from an ID is carried as a field next to it.
7. **Deterministic.** Output is in sorted or source order, never hash order. Results do not depend
   on the thread count or on the stage cache; tests assert both. `HashMap` is fine for lookups
   that are never iterated into output; anything iterated is a `BTreeMap` or a sorted `Vec`.
8. **In memory, flat, parallel.** Dense integer handles, vectors, `DateSet` bitsets, rayon. No
   spill files, no memory budgets. If a structure does not fit, redesign it.
9. **The cache is an optimisation.** A build with the stage cache disabled is byte-identical.
10. **Parity is explained, not approximate.** Until a package has switched, its output is compared
    with F# on the golden subset (`jrutil compare --expect parity/<package>.toml`). Every
    difference belongs to a ledger category with a reason. F# bugs are fixed and ledgered, never
    reproduced.

## Layering

Dependencies point one way (`Cargo.toml` has the picture). `jr-model` depends on no other
workspace crate. Only `jr-publish` and `jrutil` touch the file system. `jr-formats` decodes bytes
and readers it is handed (a ZIP, an XML document, a PBF) and never opens or discovers a file; the
compilers (`jr-czptt`, `jr-jdf`) and the passes (`jr-enrich`, `jr-posts`, `jr-overlay`) are pure
functions over in-memory values. `crates/jrutil/tests/dependency_direction.rs` checks all of it.

## Working conventions

- Errors: `thiserror` enums per crate, no `anyhow`, no `unwrap` on data. A malformed source
  entity is a diagnostic and the entity is dropped or quarantined; only a broken input file or a
  violated invariant is an error.
- Never silently guess an identity or a coordinate. Ambiguity is quarantined with a diagnostic.
- Time: `ServiceTime` (seconds from local midnight of the service date, GTFS semantics) and
  `jiff::civil::Date`. No timestamps in the model. Calendars are `DateSet`s over the GVD window;
  weekday masks exist only in `jr-publish`.
- Czech text is UTF-8 with diacritics. JDF is CP1250 on disk and is decoded in `jr-formats`.
- Tests are small fixtures in, expected values out, named after the rule they pin. Do not add
  tests that restate the implementation, and do not run real-data jobs to "make sure".
- Real data: the golden subset only (`docs/golden.md`). Never the full national feed.
- Modules stay small; a file past ~500 lines wants splitting.
- New dependencies, new `Edit` kinds, new model fields and contract changes are architecture
  decisions: raise them, do not slip them in.

## Checks

```
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Use your own `CARGO_TARGET_DIR` when another build may be running.

## Commits

Coarse conventional commits to `main` (`feat(czptt): …`). Follow-up fixes are folded into the
commit they belong to while it is unpushed. No attribution trailers.
