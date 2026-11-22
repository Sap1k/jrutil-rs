# jrutil-rs

The static timetable compiler for [Oběhy](https://github.com/Sap1k/obehy): Czech JDF and CZPTT
timetables, regional GTFS and curated data in, `jrutil-production` packages (`gtfs.zip`, typed
serving relations, manifest, diagnostics) out.

It is a Rust rewrite of the F# [JrUtil](https://gitlab.com/dvdkon/jrutil) fork Oběhy has used so
far, and is licensed AGPL-3.0 like it. The output contract is unchanged
(`contracts/serving-v5.json`).

- `ARCHITECTURE.md`: how it is put together.
- `AGENTS.md`: the rules for changing it.
- `docs/rules/`: every special case the compiler knows, and what became of each F# rule.
- `docs/golden.md`: the bounded real-data subset parity is judged on.

```
cargo test --workspace
cargo run --release -p jrutil -- validate <package-dir>
```

Status: the rail (CZPTT) package is ported and matches the F# reference on the golden subset
(`parity/czptt.toml`); it has not replaced F# in the nightly build yet. Road (JDF, the post
estimator and the regional overlay) is in progress.

```
cargo run --release -p jrutil -- build --spec build.toml
cargo run --release -p jrutil -- compare <reference-package> <candidate-package> --expect parity/czptt.toml
```
