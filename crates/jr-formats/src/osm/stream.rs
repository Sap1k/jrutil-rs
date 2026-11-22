//! The blob container: blobs are read in file order, decompressed and decoded in parallel,
//! and handed to the visitor in file order.

use std::io::{self, Read};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;

use super::block::{self, Block};
use super::protobuf::{Field, Message};
use super::{Elements, OsmError};

/// The format's limits: 64 KiB for a `BlobHeader`, 32 MiB for a `Blob` (compressed or not).
const MAX_HEADER: u64 = 64 * 1024;
const MAX_BLOB: u64 = 32 * 1024 * 1024;

/// Blobs decoded per batch and decoding thread.
const BLOBS_PER_WORKER: usize = 4;

pub(super) fn read_blocks<R: Read + Send>(
    mut pbf: R,
    elements: Elements,
    mut visit: impl FnMut(&Block),
) -> Result<(), OsmError> {
    let workers = thread::available_parallelism().map_or(1, |n| n.get());
    let batch = workers * BLOBS_PER_WORKER;
    // While one batch is decoded, the previous one is visited on this thread.
    let mut decoded: Vec<Result<Block, OsmError>> = Vec::new();
    loop {
        let read = read_batch(&mut pbf, batch);
        let raw = match &read {
            Ok(raw) => raw.as_slice(),
            Err(_) => &[],
        };
        let (visited, next) = thread::scope(|scope| {
            let decoding = (!raw.is_empty()).then(|| scope.spawn(|| decode_all(raw, elements)));
            let visited = decoded.drain(..).try_for_each(|block| {
                visit(&block?);
                Ok::<(), OsmError>(())
            });
            let next = decoding.map(|handle| handle.join().expect("decoding thread"));
            (visited, next)
        });
        visited?;
        let raw = read?;
        match next {
            Some(next) => decoded = next,
            None if raw.is_empty() => return Ok(()),
            None => unreachable!("a non-empty batch is decoded"),
        }
    }
}

/// Decompresses and decodes the blobs on all cores; results in input order.
fn decode_all(raw: &[Vec<u8>], elements: Elements) -> Vec<Result<Block, OsmError>> {
    let workers = thread::available_parallelism()
        .map_or(1, |n| n.get())
        .min(raw.len());
    let next = AtomicUsize::new(0);
    let mut results: Vec<(usize, Result<Block, OsmError>)> = thread::scope(|scope| {
        let handles: Vec<_> = (0..workers)
            .map(|_| {
                scope.spawn(|| {
                    let mut done = Vec::new();
                    let mut buffer = Vec::new();
                    loop {
                        let index = next.fetch_add(1, Ordering::Relaxed);
                        let Some(blob) = raw.get(index) else {
                            return done;
                        };
                        let block = decompress(blob, &mut buffer)
                            .and_then(|()| block::decode(&buffer, elements));
                        done.push((index, block));
                    }
                })
            })
            .collect();
        handles
            .into_iter()
            .flat_map(|handle| handle.join().expect("decoding thread"))
            .collect()
    });
    results.sort_unstable_by_key(|(index, _)| *index);
    results.into_iter().map(|(_, block)| block).collect()
}

/// Reads up to `count` `OSMData` blobs (still compressed); empty at the end of the file.
fn read_batch(reader: &mut impl Read, count: usize) -> Result<Vec<Vec<u8>>, OsmError> {
    let mut blobs = Vec::new();
    let mut header = Vec::new();
    while blobs.len() < count {
        let Some(length) = read_length(reader)? else {
            break;
        };
        read_exact(
            reader,
            &mut header,
            u64::from(length),
            "BlobHeader",
            MAX_HEADER,
        )?;
        let (kind, data_size) = blob_header(&header)?;
        let is_data = kind == b"OSMData";
        let mut blob = Vec::new();
        read_exact(reader, &mut blob, data_size, "Blob", MAX_BLOB)?;
        if is_data {
            blobs.push(blob);
        }
    }
    Ok(blobs)
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
