//! The router's source identity: FNV-1a 128 over every file under `src` (sorted relative paths
//! and contents), so a routing cache written by another router build is never read. No human
//! bumps a constant for it.

use std::fs;
use std::path::{Path, PathBuf};

const OFFSET: u128 = 0x6c62_272e_07bb_0142_62b8_2175_6295_c58d;
const PRIME: u128 = 0x0000_0000_0100_0000_0000_0000_0000_013b;

fn files(dir: &Path, out: &mut Vec<PathBuf>) {
    let mut entries: Vec<PathBuf> = fs::read_dir(dir)
        .expect("source directory")
        .map(|entry| entry.expect("source entry").path())
        .collect();
    entries.sort();
    for path in entries {
        if path.is_dir() {
            files(&path, out);
        } else {
            out.push(path);
        }
    }
}

fn main() {
    let root = Path::new("src");
    let mut paths = Vec::new();
    files(root, &mut paths);
    let mut relative: Vec<(String, PathBuf)> = paths
        .into_iter()
        .map(|path| {
            let name = path
                .strip_prefix(root)
                .expect("under src")
                .to_string_lossy()
                .replace('\\', "/");
            (name, path)
        })
        .collect();
    relative.sort();
    let mut hash = OFFSET;
    let mut feed = |bytes: &[u8]| {
        for &byte in bytes {
            hash = (hash ^ u128::from(byte)).wrapping_mul(PRIME);
        }
    };
    for (name, path) in &relative {
        let contents = fs::read(path).expect("source file");
        feed(name.as_bytes());
        feed(&[0]);
        feed(&(contents.len() as u64).to_le_bytes());
        feed(&contents);
    }
    println!("cargo:rustc-env=JR_GEO_SOURCE_HASH={hash:032x}");
    println!("cargo:rerun-if-changed=src");
}
