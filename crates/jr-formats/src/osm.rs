//! A generic OSM PBF node stream; which tags matter is the caller's business.
//!
//! The PBF container is decoded here directly (a length-prefixed `BlobHeader`, a `Blob` holding
//! raw or zlib-compressed `PrimitiveBlock`s; <https://wiki.openstreetmap.org/wiki/PBF_Format>):
//! plain and dense nodes are read, ways, relations and metadata skipped. Blobs compressed with
//! anything but zlib are an error.

mod protobuf;

use std::io::{self, Read};

use jr_model::Coord;
use protobuf::{Field, Message};

/// A node with its tags in file order.
#[derive(Clone, Debug, PartialEq)]
pub struct Node {
    pub id: i64,
    pub coord: Coord,
    pub tags: Vec<(Box<str>, Box<str>)>,
}

#[derive(Debug, thiserror::Error)]
pub enum OsmError {
    #[error("OSM PBF: {0}")]
    Io(#[from] io::Error),
    #[error("OSM PBF: truncated file")]
    Truncated,
    #[error("OSM PBF: malformed protobuf in {0}")]
    Malformed(&'static str),
    #[error("OSM PBF: {what} of {size} bytes exceeds the format's limit")]
    TooLarge { what: &'static str, size: u64 },
    #[error("OSM PBF: unsupported blob compression (field {0})")]
    Compression(u32),
    #[error("OSM PBF: string table entry {0} is out of range")]
    StringIndex(u64),
    #[error("OSM PBF: tag text is not UTF-8")]
    Utf8,
}

/// The format's limits: 64 KiB for a `BlobHeader`, 32 MiB for a `Blob` (compressed or not).
const MAX_HEADER: u64 = 64 * 1024;
const MAX_BLOB: u64 = 32 * 1024 * 1024;

/// Calls `visit` for every node that has at least one tag, in file order.
pub fn tagged_nodes<R: Read + Send>(
    mut pbf: R,
    mut visit: impl FnMut(Node),
) -> Result<(), OsmError> {
    let mut header = Vec::new();
    let mut blob = Vec::new();
    let mut block = Vec::new();
    while let Some(length) = read_length(&mut pbf)? {
        read_exact(
            &mut pbf,
            &mut header,
            u64::from(length),
            "BlobHeader",
            MAX_HEADER,
        )?;
        let (kind, data_size) = blob_header(&header)?;
        read_exact(&mut pbf, &mut blob, data_size, "Blob", MAX_BLOB)?;
        if kind != b"OSMData" {
            continue;
        }
        decompress(&blob, &mut block)?;
        primitive_block(&block, &mut visit)?;
    }
    Ok(())
}

/// The 4-byte big-endian length before each `BlobHeader`, `None` at a clean end of file.
fn read_length(reader: &mut impl Read) -> Result<Option<u32>, OsmError> {
    let mut bytes = [0u8; 4];
    let mut filled = 0;
    while filled < 4 {
        match reader.read(&mut bytes[filled..]) {
            Ok(0) if filled == 0 => return Ok(None),
            Ok(0) => return Err(OsmError::Truncated),
            Ok(n) => filled += n,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) => return Err(error.into()),
        }
    }
    Ok(Some(u32::from_be_bytes(bytes)))
}

fn read_exact(
    reader: &mut impl Read,
    buffer: &mut Vec<u8>,
    size: u64,
    what: &'static str,
    limit: u64,
) -> Result<(), OsmError> {
    if size > limit {
        return Err(OsmError::TooLarge { what, size });
    }
    buffer.clear();
    let read = reader.take(size).read_to_end(buffer)?;
    if read as u64 != size {
        return Err(OsmError::Truncated);
    }
    Ok(())
}

/// `BlobHeader`: `type` (1) and `datasize` (3).
fn blob_header(bytes: &[u8]) -> Result<(&[u8], u64), OsmError> {
    let (mut kind, mut size) = (None, None);
    let mut message = Message::new(bytes, "BlobHeader");
    while let Some((number, field)) = message.next()? {
        match (number, field) {
            (1, Field::Bytes(value)) => kind = Some(value),
            (3, Field::Varint(value)) => size = Some(value),
            _ => {}
        }
    }
    match (kind, size) {
        (Some(kind), Some(size)) => Ok((kind, size)),
        _ => Err(OsmError::Malformed("BlobHeader")),
    }
}

/// `Blob`: `raw` (1) or `zlib_data` (3); other compressions (4–7) are refused.
fn decompress(bytes: &[u8], out: &mut Vec<u8>) -> Result<(), OsmError> {
    out.clear();
    let mut message = Message::new(bytes, "Blob");
    while let Some((number, field)) = message.next()? {
        match (number, field) {
            (1, Field::Bytes(raw)) => {
                out.extend_from_slice(raw);
                return Ok(());
            }
            (3, Field::Bytes(zlib)) => {
                let decoder = flate2::read::ZlibDecoder::new(zlib);
                decoder.take(MAX_BLOB + 1).read_to_end(out)?;
                if out.len() as u64 > MAX_BLOB {
                    return Err(OsmError::TooLarge {
                        what: "decompressed Blob",
                        size: out.len() as u64,
                    });
                }
                return Ok(());
            }
            (4..=7, _) => return Err(OsmError::Compression(number)),
            _ => {}
        }
    }
    Err(OsmError::Malformed("Blob"))
}

/// A `PrimitiveBlock`'s string table and coordinate encoding.
struct Block<'a> {
    strings: Vec<&'a [u8]>,
    granularity: i64,
    lat_offset: i64,
    lon_offset: i64,
}

impl Block<'_> {
    fn coord(&self, lat: i64, lon: i64) -> Coord {
        let degrees = |offset: i64, value: i64| (offset + self.granularity * value) as f64 / 1e9;
        Coord::new(degrees(self.lon_offset, lon), degrees(self.lat_offset, lat))
    }

    fn string(&self, index: u64) -> Result<Box<str>, OsmError> {
        let bytes = usize::try_from(index)
            .ok()
            .and_then(|index| self.strings.get(index))
            .ok_or(OsmError::StringIndex(index))?;
        Ok(std::str::from_utf8(bytes)
            .map_err(|_| OsmError::Utf8)?
            .into())
    }
}

fn primitive_block(bytes: &[u8], visit: &mut impl FnMut(Node)) -> Result<(), OsmError> {
    // The string table and the coordinate encoding may follow the groups: read them first.
    let mut block = Block {
        strings: Vec::new(),
        granularity: 100,
        lat_offset: 0,
        lon_offset: 0,
    };
    let mut message = Message::new(bytes, "PrimitiveBlock");
    while let Some((number, field)) = message.next()? {
        match (number, field) {
            (1, Field::Bytes(table)) => {
                let mut table = Message::new(table, "StringTable");
                while let Some((number, field)) = table.next()? {
                    if let (1, Field::Bytes(text)) = (number, field) {
                        block.strings.push(text);
                    }
                }
            }
            (17, Field::Varint(value)) => block.granularity = value as i32 as i64,
            (19, Field::Varint(value)) => block.lat_offset = value as i64,
            (20, Field::Varint(value)) => block.lon_offset = value as i64,
            _ => {}
        }
    }
    let mut message = Message::new(bytes, "PrimitiveBlock");
    while let Some((number, field)) = message.next()? {
        if let (2, Field::Bytes(group)) = (number, field) {
            let mut group = Message::new(group, "PrimitiveGroup");
            while let Some((number, field)) = group.next()? {
                match (number, field) {
                    (1, Field::Bytes(node)) => plain_node(&block, node, visit)?,
                    (2, Field::Bytes(dense)) => dense_nodes(&block, dense, visit)?,
                    _ => {}
                }
            }
        }
    }
    Ok(())
}

/// `Node`: `id` (1, sint64), `keys` (2) and `vals` (3) as packed string indexes, `lat` (8) and
/// `lon` (9) as sint64.
fn plain_node(
    block: &Block<'_>,
    bytes: &[u8],
    visit: &mut impl FnMut(Node),
) -> Result<(), OsmError> {
    let (mut id, mut lat, mut lon) = (0, 0, 0);
    let (mut keys, mut values) = (Vec::new(), Vec::new());
    let mut message = Message::new(bytes, "Node");
    while let Some((number, field)) = message.next()? {
        match number {
            1 => id = field.sint64("Node")?,
            2 => field.each_varint("Node", |key| keys.push(key))?,
            3 => field.each_varint("Node", |value| values.push(value))?,
            8 => lat = field.sint64("Node")?,
            9 => lon = field.sint64("Node")?,
            _ => {}
        }
    }
    if keys.is_empty() {
        return Ok(());
    }
    if keys.len() != values.len() {
        return Err(OsmError::Malformed("Node"));
    }
    let tags = keys
        .into_iter()
        .zip(values)
        .map(|(key, value)| Ok((block.string(key)?, block.string(value)?)))
        .collect::<Result<_, OsmError>>()?;
    visit(Node {
        id,
        coord: block.coord(lat, lon),
        tags,
    });
    Ok(())
}

/// `DenseNodes`: delta-coded `id` (1), `lat` (8), `lon` (9), and `keys_vals` (10): per node,
/// key/value string indexes ended by `0`.
fn dense_nodes(
    block: &Block<'_>,
    bytes: &[u8],
    visit: &mut impl FnMut(Node),
) -> Result<(), OsmError> {
    let (mut ids, mut lats, mut lons) = (None, None, None);
    let mut keys_vals = None;
    let mut message = Message::new(bytes, "DenseNodes");
    while let Some((number, field)) = message.next()? {
        match (number, field) {
            (1, Field::Bytes(packed)) => ids = Some(packed),
            (8, Field::Bytes(packed)) => lats = Some(packed),
            (9, Field::Bytes(packed)) => lons = Some(packed),
            (10, Field::Bytes(packed)) => keys_vals = Some(packed),
            _ => {}
        }
    }
    let mut ids = protobuf::Varints::new(ids.unwrap_or_default(), "DenseNodes");
    let mut lats = protobuf::Varints::new(lats.unwrap_or_default(), "DenseNodes");
    let mut lons = protobuf::Varints::new(lons.unwrap_or_default(), "DenseNodes");
    let mut keys_vals = protobuf::Varints::new(keys_vals.unwrap_or_default(), "DenseNodes");
    let has_tags = !keys_vals.is_empty();
    let (mut id, mut lat, mut lon) = (0i64, 0i64, 0i64);
    while let Some(delta) = ids.next_sint64()? {
        id += delta;
        lat += lats
            .next_sint64()?
            .ok_or(OsmError::Malformed("DenseNodes"))?;
        lon += lons
            .next_sint64()?
            .ok_or(OsmError::Malformed("DenseNodes"))?;
        if !has_tags {
            continue;
        }
        let mut tags = Vec::new();
        loop {
            let key = keys_vals.next()?.ok_or(OsmError::Malformed("DenseNodes"))?;
            if key == 0 {
                break;
            }
            let value = keys_vals.next()?.ok_or(OsmError::Malformed("DenseNodes"))?;
            tags.push((block.string(key)?, block.string(value)?));
        }
        if !tags.is_empty() {
            visit(Node {
                id,
                coord: block.coord(lat, lon),
                tags,
            });
        }
    }
    Ok(())
}
