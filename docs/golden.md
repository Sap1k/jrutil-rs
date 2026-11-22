# The golden subset

Parity with the F# JrUtil and speed are judged on a bounded, frozen subset of real data, never
on the full national feed. The subset lives outside the repository, under one root
(`E:/Git/obehy/work/jrutil-rs-golden` on the development machine).

## Rail (CZPTT)

`tools/golden/czptt.py` (run with Oběhy's Python environment):

1. `fetch <root>` downloads the timetable year's CZPTT objects from the CIS JŘ mirror into
   `<root>/czptt/full`, in the source snapshot layout the compiler reads:
   `annual/JR<year>.zip`, `changes/<yyyy-mm>/*.xml.zip`, `sr70/SR70.csv`, `sources.json`.
2. `subset <root>` keeps the messages of the trains whose TR identifier hashes into one bucket
   of twenty (about 5%), as `<root>/czptt/subset` in the same layout, and writes
   `<root>/czptt/fs/derived/messages.zip`, the pre-ordered form the F# compiler takes.
3. `kadr <root>` snapshots the KADR catalogue (`kadr/catalog.json`) into both. It is the only
   step that calls an SŽ service; run it once, deliberately.

The F# reference package is produced once with `czptt-to-bundle` on that `messages.zip` and kept
as `<root>/czptt/fs/package`. The Rust compiler reads `<root>/czptt/subset` directly: ordering
the messages is part of the compiler, so the comparison also covers it.

## Road (JDF, posts, overlay)

Prepared when the road port starts (M2), from JrUtil's `scripts/golden/prepare.py` selection:
about 1,000 JDF batches, the PID and IDS JMK snapshots, the OSM extracts and the geodata
checkout.

## Rules

- One F# reference run per chain; it is pinned, not re-measured.
- While working, run only the stage being changed, once.
- A full national build happens once per package switch, with the maintainer.
