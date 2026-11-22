"""Golden rail inputs: a CZPTT source snapshot and its bounded subset (docs/golden.md).

Run with Oběhy's environment, which supplies the source downloader and the message
flattening the F# reference still needs:

    uv run --project E:/Git/obehy/repo python tools/golden/czptt.py fetch  <root>
    uv run --project E:/Git/obehy/repo python tools/golden/czptt.py subset <root>
    uv run --project E:/Git/obehy/repo python tools/golden/czptt.py kadr   <root>

`fetch` downloads the timetable year's CZPTT objects from the CIS JŘ mirror into
`<root>/czptt/full` (reusing Oběhy's source cache) and copies SR70 from the geodata checkout.
It never contacts SŽ.

`kadr` snapshots the KADR catalogue into `<root>/czptt/full/kadr` and copies it into the
subset. It is the one step that calls an SŽ service (six requests, as the nightly build makes
them); run it deliberately and once.

`subset` keeps the trains whose TR identifier hashes into one bucket of twenty (the selection
of JrUtil's `scripts/golden/prepare.py`), in the same snapshot layout, as
`<root>/czptt/subset`, and writes the ordered `messages.zip` the F# compiler takes as
`<root>/czptt/fs/derived/messages.zip`.
"""

from __future__ import annotations

import argparse
import concurrent.futures
import gzip
import hashlib
import io
import json
import shutil
import sys
import xml.etree.ElementTree as ET
import zipfile
from dataclasses import asdict
from pathlib import Path

from obehy import national_czptt as oc
from obehy.runtime_config import load_runtime_config

TRAIN_MODULUS = 20
ZIP_EPOCH = (1980, 1, 1, 0, 0, 0)


def fetch(root: Path, year: int) -> int:
    runtime = load_runtime_config(None)
    sources = root / "czptt" / "full"
    sources.mkdir(parents=True, exist_ok=False)
    inventory = oc.discover_remote_inventory(oc.DEFAULT_SOURCE_BASE_URL, year)
    downloader = oc._HttpSourceDownloader(sources, runtime.czptt_source_cache_dir / str(year))
    try:
        with concurrent.futures.ThreadPoolExecutor(oc.DEFAULT_DOWNLOAD_CONNECTIONS) as executor:
            records = list(executor.map(downloader.download, inventory))
    finally:
        downloader.close()
    for record in records:
        oc._validate_object(sources / record.relative_path, record.kind)
    records.sort(key=lambda value: value.relative_path)
    sr70 = sources / "sr70" / "SR70.csv"
    sr70.parent.mkdir()
    shutil.copy2(runtime.jrunify_ext_geodata_dir / "rail" / "SR70.csv", sr70)
    _write_manifest(sources, year, records)
    print(f"{len(records)} objects ({downloader.cached} from the cache) in {sources}")
    return 0


def _write_manifest(sources: Path, year: int, records: list[oc.SourceRecord]) -> None:
    auxiliary = [
        {
            "relative_path": path.relative_to(sources).as_posix(),
            "bytes": path.stat().st_size,
            "sha256": hashlib.sha256(path.read_bytes()).hexdigest(),
        }
        for directory in (sources / "kadr", sources / "sr70")
        if directory.is_dir()
        for path in sorted(directory.rglob("*"))
        if path.is_file()
    ]
    manifest = {
        "schema_version": 1,
        "timetable_year": year,
        "source_base_url": oc.DEFAULT_SOURCE_BASE_URL,
        "objects": [asdict(value) for value in records],
        "auxiliary": auxiliary,
    }
    (sources / "sources.json").write_text(
        json.dumps(manifest, ensure_ascii=False, indent=2) + "\n", encoding="utf-8"
    )


def _kept(message: bytes) -> bool:
    """Whether the message's train falls into the kept bucket."""
    root = ET.fromstring(message)
    for element in root.iter():
        if element.tag.rsplit("}", 1)[-1] != "PlannedTransportIdentifiers":
            continue
        values = {child.tag.rsplit("}", 1)[-1]: (child.text or "") for child in element}
        if values.get("ObjectType") == "TR":
            key = "|".join(values.get(name, "") for name in ("Company", "Core", "Variant", "TimetableYear"))
            digest = hashlib.sha256(key.encode("utf-8")).digest()
            return int.from_bytes(digest[:8], "big") % TRAIN_MODULUS == 0
    return False


def kadr(root: Path) -> int:
    full = root / "czptt" / "full"
    oc.snapshot_kadr(full / "kadr")
    for sources in (full, root / "czptt" / "subset"):
        if not sources.is_dir():
            continue
        if sources != full:
            shutil.copytree(full / "kadr", sources / "kadr")
        manifest = json.loads((sources / "sources.json").read_text(encoding="utf-8"))
        records = [oc.SourceRecord(**value) for value in manifest["objects"]]
        _write_manifest(sources, manifest["timetable_year"], records)
    print(f"KADR catalogue in {full / 'kadr'}")
    return 0


def subset(root: Path) -> int:
    full = root / "czptt" / "full"
    target = root / "czptt" / "subset"
    target.mkdir(parents=True, exist_ok=False)
    manifest = json.loads((full / "sources.json").read_text(encoding="utf-8"))
    records: list[oc.SourceRecord] = []
    total = kept = 0

    def record(relative_path: str, kind: str) -> None:
        path = target / relative_path
        payload = path.read_bytes()
        records.append(
            oc.SourceRecord(
                relative_path=relative_path,
                url=None,
                bytes=len(payload),
                sha256=hashlib.sha256(payload).hexdigest(),
                kind=kind,
            )
        )

    for source in manifest["objects"]:
        relative_path, kind = source["relative_path"], source["kind"]
        path = full / relative_path
        destination = target / relative_path
        if kind == "annual_zip":
            destination.parent.mkdir(parents=True, exist_ok=True)
            with zipfile.ZipFile(path) as archive, zipfile.ZipFile(
                destination, "w", zipfile.ZIP_DEFLATED
            ) as output:
                for info in sorted(archive.infolist(), key=lambda value: value.filename):
                    if info.is_dir() or not info.filename.casefold().endswith(".xml"):
                        continue
                    total += 1
                    payload = archive.read(info)
                    if _kept(payload):
                        kept += 1
                        output.writestr(zipfile.ZipInfo(info.filename, ZIP_EPOCH), payload)
            record(relative_path, kind)
        else:
            total += 1
            with gzip.open(path, "rb") as stream:
                payload = stream.read()
            if _kept(payload):
                kept += 1
                destination.parent.mkdir(parents=True, exist_ok=True)
                shutil.copy2(path, destination)
                record(relative_path, kind)

    if (full / "kadr").is_dir():
        shutil.copytree(full / "kadr", target / "kadr")
    shutil.copytree(full / "sr70", target / "sr70")
    records.sort(key=lambda value: value.relative_path)
    _write_manifest(target, manifest["timetable_year"], records)

    flattened = root / "czptt" / "fs" / "derived" / "messages.zip"
    ordered = oc.flatten_messages(target, records, flattened)
    summary = {"messages_total": total, "messages_kept": kept, "messages_ordered": ordered}
    (root / "czptt" / "subset.json").write_text(json.dumps(summary, indent=2) + "\n", encoding="utf-8")
    print(json.dumps(summary, indent=2))
    return 0


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawTextHelpFormatter)
    commands = parser.add_subparsers(dest="command", required=True)
    fetch_parser = commands.add_parser("fetch")
    fetch_parser.add_argument("root", type=Path)
    fetch_parser.add_argument("--year", type=int, default=2026)
    subset_parser = commands.add_parser("subset")
    subset_parser.add_argument("root", type=Path)
    commands.add_parser("kadr").add_argument("root", type=Path)
    args = parser.parse_args()
    if args.command == "fetch":
        return fetch(args.root, args.year)
    if args.command == "kadr":
        return kadr(args.root)
    return subset(args.root)


if __name__ == "__main__":
    # The Windows console is not UTF-8 by default; station names are.
    if isinstance(sys.stdout, io.TextIOWrapper):
        sys.stdout.reconfigure(encoding="utf-8")
    raise SystemExit(main())
