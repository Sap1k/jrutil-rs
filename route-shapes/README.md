# obehy-motis-shapes

Route-shapes companion of the compiler's route-shapes phase (`[czptt.shapes]`, `[jdf.shapes]`:
`crates/jrutil/src/companion.rs` runs it, `jr_enrich::shapes` decides what its answer means;
`docs/rules/serving.md` sections 8.2 and 8.3; Oběhy `BASE_PLAN.md` section 17). It is C++ because
osr is; Oběhy runs `extract` to build the graph, the compiler runs `route`. It map-matches the
phase's stop patterns onto OSM with [osr](https://github.com/motis-project/osr), MOTIS's router,
pinned in `.pkg` to the commit MOTIS v2.11.0 uses. `pkg` resolves the dependencies into `deps/`
(ignored) and records them in `.pkg.lock` (committed).

```text
obehy-motis-shapes extract --osm <routing.osm.pbf> --graph <dir> --key <text>
obehy-motis-shapes route   --graph <dir> --patterns <patterns.csv> --out <dir>
                           [--cache <dir>] [--threads auto|N] [--simplify-m M]
```

- The PBF must be sorted by id (`osmium sort`); osr keeps only ways that meet another way.
- `--key` names the graph's input (Oběhy passes the routing PBF's sha256); it is part of the
  cache key, together with the osr commit and the simplification tolerance.
- Classes: `bus` (osr `bus`), `rail` (`railway`), `ferry`; anything else is `unsupported`.
- Outputs, sorted by pattern: `pattern_status.csv`, `shapes.csv`, `offsets.csv`,
  `diagnostics.json`, `manifest.json`. Cold and warm runs and any thread count write identical
  bytes.

CI (`.github/workflows/route-shapes.yml`) builds it on Linux and publishes the binary as release
`route-shapes-<source hash>` (the hash of `git ls-files -s route-shapes`); Oběhy's nightly build
downloads the release matching its jrutil-rs checkout. It came here from the F# JrUtil
repository on 2026-11-24 (`c5b62e8` there).

## Local build (Windows, Visual Studio 2026)

From a VS x64 developer prompt, with the bundled CMake and Ninja on `PATH`:

```bat
cmake -S route-shapes -B ..\build\route-shapes -G Ninja ^
      -DCMAKE_BUILD_TYPE=Release -DCMAKE_NINJA_CMCLDEPS_RC=OFF
cmake --build ..\build\route-shapes
..\build\route-shapes\obehy-motis-shapes-test.exe
```

`CMAKE_NINJA_CMCLDEPS_RC=OFF` avoids a resource-compiler failure when Visual Studio prints its
include notes in a language other than English.

The fixture PBF is generated from `test/fixture.osm`:
`osmium sort test/fixture.osm -o test/fixture.osm.pbf -f pbf -O`.
