//! `xs:dateTime` and `xs:time` as CZPTT writes them: the local reading and the offset, if any.

use jiff::civil::{DateTime, Time};

/// `YYYY-MM-DDTHH:MM:SS[.f…][Z|±hh:mm]`.
pub(super) fn date_time(text: &str) -> Option<(DateTime, Option<i32>)> {
    let (local, offset) = split_offset(text)?;
    let bytes = local.as_bytes();
    let shape = bytes.len() >= 19
        && digits(&bytes[0..4])
        && bytes[4] == b'-'
        && digits(&bytes[5..7])
        && bytes[7] == b'-'
        && digits(&bytes[8..10])
        && bytes[10] == b'T'
        && clock(&bytes[11..]);
    if !shape {
        return None;
    }
    Some((local.parse().ok()?, offset))
}

/// `HH:MM:SS[.f…][Z|±hh:mm]`.
pub(super) fn time(text: &str) -> Option<(Time, Option<i32>)> {
    let (local, offset) = split_offset(text)?;
    if !clock(local.as_bytes()) {
        return None;
    }
    Some((local.parse().ok()?, offset))
}

/// `HH:MM:SS` with an optional non-empty fraction.
fn clock(bytes: &[u8]) -> bool {
    bytes.len() >= 8
        && digits(&bytes[0..2])
        && bytes[2] == b':'
        && digits(&bytes[3..5])
        && bytes[5] == b':'
        && digits(&bytes[6..8])
        && match &bytes[8..] {
            [] => true,
            [b'.', fraction @ ..] => !fraction.is_empty() && digits(fraction),
            _ => false,
        }
}

fn digits(bytes: &[u8]) -> bool {
    bytes.iter().all(u8::is_ascii_digit)
}

/// Splits a trailing `Z` or `±hh:mm` off; the offset in seconds east of UTC.
fn split_offset(text: &str) -> Option<(&str, Option<i32>)> {
    if let Some(local) = text.strip_suffix('Z') {
        return Some((local, Some(0)));
    }
    let bytes = text.as_bytes();
    let Some(at) = bytes.len().checked_sub(6) else {
        return Some((text, None));
    };
    let sign = match bytes[at] {
        b'+' => 1,
        b'-' => -1,
        _ => return Some((text, None)),
    };
    let tail = &bytes[at + 1..];
    if !(digits(&tail[0..2]) && tail[2] == b':' && digits(&tail[3..5])) {
        return None;
    }
    let number = |pair: &[u8]| i32::from(pair[0] - b'0') * 10 + i32::from(pair[1] - b'0');
    let (hours, minutes) = (number(&tail[0..2]), number(&tail[3..5]));
    if minutes >= 60 {
        return None;
    }
    Some((&text[..at], Some(sign * (hours * 3600 + minutes * 60))))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn time_variants() {
        let t = |h, m, s, n| Time::new(h, m, s, n).unwrap();
        assert_eq!(
            time("06:15:00.0000000+01:00"),
            Some((t(6, 15, 0, 0), Some(3600)))
        );
        assert_eq!(time("06:15:30"), Some((t(6, 15, 30, 0), None)));
        assert_eq!(
            time("23:59:59.5Z"),
            Some((t(23, 59, 59, 500_000_000), Some(0)))
        );
        assert_eq!(time("00:00:00-05:30"), Some((t(0, 0, 0, 0), Some(-19_800))));
        for bad in [
            "6:15:00",
            "06:15",
            "06:15:00.",
            "06:15:00+1:00",
            "25:00:00",
            "x",
        ] {
            assert_eq!(time(bad), None, "{bad}");
        }
    }

    #[test]
    fn date_time_variants() {
        let local: DateTime = "2025-11-05T17:32:23".parse().unwrap();
        assert_eq!(date_time("2025-11-05T17:32:23"), Some((local, None)));
        assert_eq!(
            date_time("2025-11-05T17:32:23+02:00"),
            Some((local, Some(7200)))
        );
        for bad in ["2025-11-05", "2025-11-05 17:32:23", "2025-02-30T00:00:00"] {
            assert_eq!(date_time(bad), None, "{bad}");
        }
    }
}
