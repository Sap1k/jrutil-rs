//! OSM PBF: a tiny file built here, with plain and dense nodes, raw and zlib blobs.

use std::io::Write;

use jr_formats::osm::{self, Node};

fn varint(out: &mut Vec<u8>, mut value: u64) {
    while value >= 0x80 {
        out.push((value as u8) | 0x80);
        value >>= 7;
    }
    out.push(value as u8);
}

fn zigzag(value: i64) -> u64 {
    ((value << 1) ^ (value >> 63)) as u64
}

fn number(out: &mut Vec<u8>, field: u64, value: u64) {
    varint(out, field << 3);
    varint(out, value);
}

fn bytes(out: &mut Vec<u8>, field: u64, data: &[u8]) {
    varint(out, (field << 3) | 2);
    varint(out, data.len() as u64);
    out.extend_from_slice(data);
}

fn packed(values: impl IntoIterator<Item = u64>) -> Vec<u8> {
    let mut out = Vec::new();
    for value in values {
        varint(&mut out, value);
    }
    out
}

/// A length-prefixed `BlobHeader` and its `Blob`.
fn blob(file: &mut Vec<u8>, kind: &str, block: &[u8], zlib: bool) {
    let mut blob = Vec::new();
    if zlib {
        let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::fast());
        encoder.write_all(block).unwrap();
        number(&mut blob, 2, block.len() as u64);
        bytes(&mut blob, 3, &encoder.finish().unwrap());
    } else {
        bytes(&mut blob, 1, block);
    }
    let mut header = Vec::new();
    bytes(&mut header, 1, kind.as_bytes());
    number(&mut header, 3, blob.len() as u64);
    file.extend_from_slice(&(header.len() as u32).to_be_bytes());
    file.extend_from_slice(&header);
    file.extend_from_slice(&blob);
}

fn string_table(strings: &[&str]) -> Vec<u8> {
    let mut table = Vec::new();
    for s in strings {
        bytes(&mut table, 1, s.as_bytes());
    }
    table
}

fn plain_node(id: i64, lat: i64, lon: i64, tags: &[(u64, u64)]) -> Vec<u8> {
    let mut node = Vec::new();
    number(&mut node, 1, zigzag(id));
    if !tags.is_empty() {
        bytes(&mut node, 2, &packed(tags.iter().map(|t| t.0)));
        bytes(&mut node, 3, &packed(tags.iter().map(|t| t.1)));
    }
    number(&mut node, 8, zigzag(lat));
    number(&mut node, 9, zigzag(lon));
    node
}

fn pbf() -> Vec<u8> {
    let strings = string_table(&[
        "",
        "railway",
        "station",
        "name",
        "Praha hl.n.",
        "highway",
        "crossing",
    ]);
    let mut file = Vec::new();
    blob(&mut file, "OSMHeader", b"\x22\x0eOsmSchema-V0.6", false);

    // Block 1 (zlib): a tagged and an untagged plain node, dense nodes, a way.
    let mut nodes = Vec::new();
    bytes(
        &mut nodes,
        1,
        &plain_node(1, 500_000_000, 140_000_000, &[(1, 2), (3, 4)]),
    );
    bytes(&mut nodes, 1, &plain_node(2, 500_000_001, 140_000_001, &[]));
    let mut dense = Vec::new();
    bytes(&mut dense, 1, &packed([10, 1, 1].map(zigzag)));
    bytes(&mut dense, 8, &packed([501_000_000, 1, -2].map(zigzag)));
    bytes(&mut dense, 9, &packed([141_000_000, 1, -2].map(zigzag)));
    bytes(&mut dense, 10, &packed([5, 6, 0, 0, 1, 2, 0]));
    let mut dense_group = Vec::new();
    bytes(&mut dense_group, 2, &dense);
    let mut way_group = Vec::new();
    bytes(&mut way_group, 3, &[0x08, 0x07]);
    let mut block = Vec::new();
    bytes(&mut block, 2, &nodes);
    bytes(&mut block, 2, &dense_group);
    bytes(&mut block, 2, &way_group);
    bytes(&mut block, 1, &strings); // the string table may come last
    blob(&mut file, "OSMData", &block, true);

    // Block 2 (raw): a coarser granularity and an offset.
    let mut nodes = Vec::new();
    bytes(&mut nodes, 1, &plain_node(-20, 1, 2, &[(1, 2)]));
    let mut block = Vec::new();
    bytes(&mut block, 1, &strings);
    bytes(&mut block, 2, &nodes);
    number(&mut block, 17, 1000);
    number(&mut block, 19, 1_000_000_000);
    blob(&mut file, "OSMData", &block, false);
    file
}

#[test]
fn tagged_nodes_in_file_order() {
    let mut nodes = Vec::new();
    osm::tagged_nodes(pbf().as_slice(), |node| nodes.push(node)).expect("PBF");

    let summary: Vec<_> = nodes
        .iter()
        .map(|Node { id, coord, tags }| {
            let tags: Vec<_> = tags.iter().map(|(k, v)| format!("{k}={v}")).collect();
            (*id, coord.lat, coord.lon, tags.join(";"))
        })
        .collect();
    assert_eq!(
        summary,
        [
            (1, 50.0, 14.0, "railway=station;name=Praha hl.n.".to_owned()),
            (10, 50.1, 14.1, "highway=crossing".to_owned()),
            (12, 50.0999999, 14.0999999, "railway=station".to_owned()),
            (-20, 1.000001, 0.000002, "railway=station".to_owned()),
        ]
    );
}

#[test]
fn truncated_file_is_an_error() {
    let file = pbf();
    assert!(osm::tagged_nodes(&file[..file.len() - 3], |_| {}).is_err());
}
