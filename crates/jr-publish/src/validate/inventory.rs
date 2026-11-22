//! The closed file inventory, the manifest's fields, its `files[]` sizes and digests, its
//! relation declarations and its sources.

use std::collections::{BTreeMap, BTreeSet};
use std::fs::File;
use std::io::Read;
use std::path::Path;

use rayon::prelude::*;
use sha2::{Digest, Sha256};

use super::{Compatibility, Findings, code, relation_file};
use crate::contract::{self, Contract, parse_version};
use crate::error::PackageError;
use crate::manifest::{FileEntry, Manifest};
use crate::package::{DIAGNOSTICS, GTFS, MANIFEST, Package};

/// Every file under `root`, package-relative with `/`, with its size.
pub fn list_files(root: &Path) -> Result<BTreeMap<String, u64>, PackageError> {
    let mut files = BTreeMap::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(directory) = pending.pop() {
        for entry in std::fs::read_dir(&directory).map_err(PackageError::io(&directory))? {
            let entry = entry.map_err(PackageError::io(&directory))?;
            let path = entry.path();
            let kind = entry.file_type().map_err(PackageError::io(&path))?;
            if kind.is_dir() {
                pending.push(path);
            } else {
                let size = entry.metadata().map_err(PackageError::io(&path))?.len();
                let relative = path.strip_prefix(root).unwrap_or(&path);
                let parts: Vec<String> = relative
                    .components()
                    .map(|part| part.as_os_str().to_string_lossy().into_owned())
                    .collect();
                files.insert(parts.join("/"), size);
            }
        }
    }
    Ok(files)
}

pub fn check(
    findings: &mut Findings,
    package: &Package,
    contract: &Contract,
    compat: &Compatibility,
    files: &BTreeMap<String, u64>,
) {
    let mut expected: BTreeSet<String> = [GTFS, MANIFEST, DIAGNOSTICS]
        .into_iter()
        .map(str::to_owned)
        .collect();
    expected.extend(
        contract
            .relations
            .iter()
            .map(|relation| relation_file(&relation.name)),
    );
    if compat.newer_minor {
        expected.extend(
            compat
                .extra_relations
                .iter()
                .map(|name| relation_file(name)),
        );
    }
    for path in expected.iter().filter(|path| !files.contains_key(*path)) {
        findings.add(
            code::MISSING_FILE,
            path,
            "declared production file is missing",
        );
    }
    for path in files.keys().filter(|path| !expected.contains(*path)) {
        findings.add(
            code::UNDECLARED_FILE,
            path,
            "file is not part of the closed inventory",
        );
    }

    let manifest = package.manifest();
    check_fields(findings, manifest, contract);
    expected.remove(MANIFEST);
    check_files(findings, package, &expected, files);
    check_relations(findings, manifest, contract, compat);
    check_sources(findings, manifest);
}

fn check_fields(findings: &mut Findings, manifest: &Manifest, contract: &Contract) {
    let mut require = |ok: bool, name: &str, expected: &str| {
        if !ok {
            findings.add(
                code::MANIFEST_FIELD_INVALID,
                MANIFEST,
                format!("{name} must be {expected}"),
            );
        }
    };
    require(
        manifest.bundle_format.as_deref() == Some(contract::BUNDLE_FORMAT),
        "bundle_format",
        contract::BUNDLE_FORMAT,
    );
    require(
        manifest.bundle_version == Some(contract::BUNDLE_VERSION),
        "bundle_version",
        &contract::BUNDLE_VERSION.to_string(),
    );
    let major = manifest
        .serving_schema_version
        .as_deref()
        .and_then(parse_version)
        .map(|v| v.0);
    require(
        major == Some(contract.major),
        "serving_schema_version",
        &format!("{}.<minor>", contract.major),
    );
    require(
        manifest.diagnostics_schema_version == Some(contract::DIAGNOSTICS_SCHEMA_VERSION),
        "diagnostics_schema_version",
        &contract::DIAGNOSTICS_SCHEMA_VERSION.to_string(),
    );
    require(
        manifest.identity_contract.as_deref() == Some(contract::IDENTITY_CONTRACT),
        "identity_contract",
        contract::IDENTITY_CONTRACT,
    );
}

/// `files[]` lists exactly the payloads, with their sizes and SHA-256 digests.
fn check_files(
    findings: &mut Findings,
    package: &Package,
    payloads: &BTreeSet<String>,
    files: &BTreeMap<String, u64>,
) {
    let Some(entries) = &package.manifest().files else {
        findings.add(
            code::MANIFEST_FILES_MISSING,
            MANIFEST,
            "files inventory is missing or malformed",
        );
        return;
    };
    let mut inventory: BTreeMap<&str, &FileEntry> = BTreeMap::new();
    for entry in entries {
        if inventory.insert(&entry.path, entry).is_some() {
            findings.add(
                code::INVENTORY_DUPLICATE,
                MANIFEST,
                format!("{} is inventoried twice", entry.path),
            );
        }
    }
    for path in payloads
        .iter()
        .filter(|path| !inventory.contains_key(path.as_str()))
    {
        findings.add(
            code::INVENTORY_MISSING,
            MANIFEST,
            format!("payload is not inventoried: {path}"),
        );
    }
    for path in inventory.keys().filter(|path| !payloads.contains(**path)) {
        findings.add(
            code::INVENTORY_UNDECLARED,
            MANIFEST,
            format!("undeclared inventory entry: {path}"),
        );
    }
    let present: Vec<(&FileEntry, u64)> = inventory
        .values()
        .filter(|entry| payloads.contains(&entry.path))
        .filter_map(|entry| files.get(&entry.path).map(|&size| (*entry, size)))
        .collect();
    let digests: Vec<Result<String, PackageError>> = present
        .par_iter()
        .map(|(entry, _)| sha256_file(&package.root().join(&entry.path)))
        .collect();
    for ((entry, size), digest) in present.into_iter().zip(digests) {
        if size != entry.size_bytes {
            findings.add(
                code::FILE_SIZE_MISMATCH,
                &entry.path,
                format!("{size} bytes, manifest says {}", entry.size_bytes),
            );
        }
        match digest {
            Ok(digest) if digest == entry.sha256 => {}
            Ok(digest) => findings.add(
                code::FILE_HASH_MISMATCH,
                &entry.path,
                format!("SHA-256 {digest}, manifest says {}", entry.sha256),
            ),
            Err(error) => findings.add(code::FILE_HASH_MISMATCH, &entry.path, error.to_string()),
        }
    }
}

fn sha256_file(path: &Path) -> Result<String, PackageError> {
    let mut file = File::open(path).map_err(PackageError::io(path))?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0_u8; 1 << 20];
    loop {
        let read = file.read(&mut buffer).map_err(PackageError::io(path))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hex(&hasher.finalize()))
}

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut text = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        text.push(DIGITS[usize::from(byte >> 4)] as char);
        text.push(DIGITS[usize::from(byte & 15)] as char);
    }
    text
}

/// Every contract relation is declared; nothing else unless a later minor added it. The
/// declared schema matches the contract (the consumer loads by it).
fn check_relations(
    findings: &mut Findings,
    manifest: &Manifest,
    contract: &Contract,
    compat: &Compatibility,
) {
    let Some(declared) = &manifest.relations else {
        findings.add(
            code::MANIFEST_RELATIONS_MISSING,
            MANIFEST,
            "relations are missing or malformed",
        );
        return;
    };
    for relation in &contract.relations {
        let Some(declaration) = manifest.relation(&relation.name) else {
            findings.add(
                code::RELATION_UNDECLARED,
                MANIFEST,
                format!("relation is not declared: {}", relation.name),
            );
            continue;
        };
        let fields = &declaration.schema;
        let prefix_matches = fields.len() >= relation.fields.len()
            && relation.fields.iter().zip(fields).all(|(expected, field)| {
                field.name == expected.name
                    && field.ty == expected.ty.as_str()
                    && field.nullable == expected.nullable
                    && field.enumeration == expected.enumeration
            });
        let extra_allowed = compat.newer_minor
            && fields[relation.fields.len().min(fields.len())..]
                .iter()
                .all(|f| f.nullable);
        let same_length = fields.len() == relation.fields.len();
        if !prefix_matches
            || !(same_length || extra_allowed)
            || declaration.primary_key != relation.primary_key
        {
            findings.add(
                code::MANIFEST_SCHEMA_MISMATCH,
                MANIFEST,
                format!(
                    "declared schema or primary key of {} differs from the contract",
                    relation.name
                ),
            );
        }
    }
    if !compat.newer_minor {
        for relation in declared
            .iter()
            .filter(|relation| contract.relation(&relation.name).is_none())
        {
            findings.add(
                code::RELATION_UNKNOWN,
                MANIFEST,
                format!("unknown relation declaration: {}", relation.name),
            );
        }
    }
}

/// Every source records a real payload digest.
fn check_sources(findings: &mut Findings, manifest: &Manifest) {
    let sources = match &manifest.sources {
        Some(sources) if !sources.is_empty() => sources,
        _ => {
            findings.add(
                code::MANIFEST_SOURCES_MISSING,
                MANIFEST,
                "sources are missing",
            );
            return;
        }
    };
    for source in sources {
        let valid = source.payload_sha256.as_deref().is_some_and(|digest| {
            digest.len() == 64
                && digest
                    .bytes()
                    .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
                && digest.bytes().any(|b| b != b'0')
        });
        if !valid {
            let id = source.source_id.as_deref().unwrap_or("?");
            findings.add(
                code::SOURCE_DIGEST_MISSING,
                MANIFEST,
                format!("source {id} has no payload_sha256 digest"),
            );
        }
    }
}
