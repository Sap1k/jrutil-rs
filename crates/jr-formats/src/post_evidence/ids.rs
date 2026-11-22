//! The identities the evidence pack recomputes (`docs/rules/posts.md`, section 4).
//!
//! They are part of the evidence-v2 format: the reader recomputes context, movement-family and
//! same-stop-block IDs and rejects a pack whose stored ones differ (POST-R061), and the training
//! labels join on the context ID (POST-R044). A package that builds a pack in memory must use
//! these functions, not its own copies.

use sha2::{Digest, Sha256};

/// Lower-case hex SHA-256 of `bytes` (`sha(x)` of section 4).
pub fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut text = String::with_capacity(64);
    for byte in digest {
        text.push(char::from(HEX[usize::from(byte >> 4)]));
        text.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    text
}

const HEX: &[u8; 16] = b"0123456789abcdef";

/// `prefix + sha(parts joined with "|")` (F# `stableId`).
pub fn stable_id(prefix: &str, parts: &[&str]) -> String {
    let mut id = String::with_capacity(prefix.len() + 64);
    id.push_str(prefix);
    id.push_str(&sha256_hex(parts.join("|").as_bytes()));
    id
}

/// Whether `text` is 64 hex digits of either case (a SHA-256 identity of the manifest).
pub fn is_sha256(text: &str) -> bool {
    text.len() == 64 && text.bytes().all(|byte| byte.is_ascii_hexdigit())
}

/// The stop identity the pack stores: `jdf:stop:<N>` (verdict Q9).
pub fn stop_text(stop: u64) -> String {
    format!("jdf:stop:{stop}")
}

/// The stop number of a stored stop identity, the whole text matching
/// `(?:jdf|cis):stop:[0-9]+` and the number fitting a signed 64-bit integer, as the F# store
/// requires. This is the only place a stop number is read out of an identity (verdict Q9).
pub fn parse_stop(text: &str) -> Option<u64> {
    let digits = text
        .strip_prefix("jdf:stop:")
        .or_else(|| text.strip_prefix("cis:stop:"))?;
    if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let number: i64 = digits.parse().ok()?;
    u64::try_from(number).ok()
}

/// POST-R044: `stableId("context:", [stop, mode, line, distinction, direction, pattern hash,
/// position, role, authored key or ""])`.
#[allow(clippy::too_many_arguments)]
pub fn context_id(
    stop: u64,
    mode: &str,
    line: &str,
    distinction: i32,
    direction: i32,
    pattern_hash: &str,
    position: i32,
    role: &str,
    authored_post_key: Option<&str>,
) -> String {
    stable_id(
        "context:",
        &[
            &stop.to_string(),
            mode,
            line,
            &distinction.to_string(),
            &direction.to_string(),
            pattern_hash,
            &position.to_string(),
            role,
            authored_post_key.unwrap_or(""),
        ],
    )
}

/// POST-R054: `stableId("movement-family:", [stop, mode, rank-0 ingress thread or "", rank-0
/// egress thread or "", role])`.
pub fn movement_family_id(
    stop: u64,
    mode: &str,
    ingress_thread: Option<&str>,
    egress_thread: Option<&str>,
    role: &str,
) -> String {
    stable_id(
        "movement-family:",
        &[
            &stop.to_string(),
            mode,
            ingress_thread.unwrap_or(""),
            egress_thread.unwrap_or(""),
            role,
        ],
    )
}

/// POST-R042: `stableId("same-stop-block:", [line, distinction, pattern hash, first position,
/// last position + 1])`.
pub fn same_stop_block_id(
    line: &str,
    distinction: i32,
    pattern_hash: &str,
    start: i32,
    finish: i32,
) -> String {
    stable_id(
        "same-stop-block:",
        &[
            line,
            &distinction.to_string(),
            pattern_hash,
            &start.to_string(),
            &finish.to_string(),
        ],
    )
}
