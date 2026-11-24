# jrutil-rs

The static timetable compiler for [Oběhy](https://github.com/Sap1k/obehy): Czech JDF and CZPTT
timetables, regional GTFS and curated data in, `jrutil-production` packages (`gtfs.zip`, typed
serving relations, manifest, diagnostics) out.

It is a Rust rewrite of the F# [JrUtil](https://gitlab.com/dvdkon/jrutil) fork Oběhy used
before, and is licensed AGPL-3.0 like it. Oběhy's production build has run on it since
2026-11-23 (Oběhy `001ccdf`), for both packages:

- **`czptt`** (rail): the CZPTT annual and change messages, KADR, SR70 and OSM railway
  locations; replacement-bus connections; IDS JMK train trip IDs as keys of the national trains;
  route shapes.
- **`jdf`** (road): the national JDF exports with stop matching against the catalogues and the
  stop registry, estimated boarding points (posts), the PID and IDS JMK overlay, route shapes,
  and the filtered feed (the road feed without the lines other feeds publish).

Both were compared with F# nationally before the switch; every difference is explained in
`parity/` and `docs/golden.md`. The serving contract is `contracts/serving-v5.json` (5.3).

## Using it

```
cargo build --release --locked -p jrutil
jrutil build --spec build.toml [--no-cache] [--progress-events]
jrutil validate <package-dir>
jrutil compare <reference-package> <candidate-package> --expect parity/<package>.toml
```

The build specification is TOML with a `[czptt]` and/or a `[jdf]` section
(`crates/jrutil/src/spec.rs`; `docs/switch.md` has full examples). Oběhy writes one per run
(`src/obehy/static_build.py`) and its `build.yml` builds this repository from a ref
(`jrutil_rs_ref`, default `main`).

## Documents

- `ARCHITECTURE.md`: how it is put together.
- `AGENTS.md`: the rules for changing it.
- `docs/rules/`: every special case the compiler knows, and what became of each F# rule.
- `docs/golden.md`: the bounded real-data subset, and the national comparisons.
- `docs/switch.md`: how Oběhy runs the compiler, and what changed when it switched.
- `route-shapes/`: the route-shapes companion (C++ on MOTIS's osr router), built and released
  by `.github/workflows/route-shapes.yml`.

```
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```
