//! Public identifiers: the only place one is built.
//!
//! A [`PublicId`] is what a package publishes (`czptt:stop:CZ:53414`, `jdf:trip:…`). Nothing
//! outside this module formats one, and nothing anywhere parses one: whatever a consumer needs
//! from an ID (a train number, a part index) is a field next to it. Each feed's scheme is a
//! submodule here, added with the compiler that uses it and documented in `docs/identity.md`.

use std::borrow::Cow;
use std::fmt;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// The feed a timetable belongs to. Its prefix starts every ID the feed's compiler builds.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Feed {
    /// Road, tram and urban transit, from JDF and the regional overlays.
    Jdf,
    /// Heavy rail, from CZPTT.
    Czptt,
}

impl Feed {
    pub const fn prefix(self) -> &'static str {
        match self {
            Feed::Jdf => "jdf",
            Feed::Czptt => "czptt",
        }
    }
}

/// An opaque published identifier.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct PublicId(Box<str>);

impl PublicId {
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// `<prefix>:<part>:<part>…`, each part escaped so a `:` inside one cannot collide with the
    /// separator.
    pub(crate) fn build(prefix: &str, parts: &[&str]) -> Self {
        let mut id = String::from(prefix);
        for part in parts {
            id.push(':');
            id.push_str(&escape(part));
        }
        Self(id.into_boxed_str())
    }

    /// `<prefix>:<32 hex>`: an ID for a row that has no natural key of its own (a note
    /// assignment, a generated transfer), derived from the facts that make it what it is. The
    /// fields are hashed in the order given, each as `name=value` on its own line, so the
    /// caller fixes a canonical order once.
    pub(crate) fn from_content(prefix: &str, fields: &[(&str, &str)]) -> Self {
        let mut hasher = Sha256::new();
        hasher.update(b"jrutil-rs-id-v1\n");
        for (name, value) in fields {
            hasher.update(name.as_bytes());
            hasher.update(b"=");
            hasher.update(escape(value).as_bytes());
            hasher.update(b"\n");
        }
        let digest = hasher.finalize();
        let mut id = String::with_capacity(prefix.len() + 33);
        id.push_str(prefix);
        id.push(':');
        for byte in &digest[..16] {
            id.push_str(&format!("{byte:02x}"));
        }
        Self(id.into_boxed_str())
    }
}

impl fmt::Display for PublicId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl fmt::Debug for PublicId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self.0)
    }
}

/// Percent-encodes everything but the RFC 3986 unreserved characters (`A–Z a–z 0–9 - _ . ~`),
/// as UTF-8 with upper-case hex. This is the escaping the published ID scheme has always used
/// for text components.
pub fn escape(component: &str) -> Cow<'_, str> {
    let unreserved =
        |byte: u8| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~');
    if component.bytes().all(unreserved) {
        return Cow::Borrowed(component);
    }
    let mut escaped = String::with_capacity(component.len() + 8);
    for byte in component.bytes() {
        if unreserved(byte) {
            escaped.push(byte as char);
        } else {
            escaped.push_str(&format!("%{byte:02X}"));
        }
    }
    Cow::Owned(escaped)
}

impl PublicId {
    /// `<self>:<part>…`: an ID inside this one's namespace (a station's boarding point).
    fn child(&self, parts: &[&str]) -> Self {
        Self::build(self.as_str(), parts)
    }
}

/// A service is its dates: `<feed>:service:<16 hex>` of the date set's bits, so equal
/// calendars share an ID and the ID survives everything but a change of dates.
pub fn service(feed: Feed, dates: &crate::DateSet) -> PublicId {
    let digest = Sha256::digest(dates.to_bytes());
    let hex: String = digest[..8]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    PublicId(format!("{}:service:{hex}", feed.prefix()).into_boxed_str())
}

/// One slot of a route direction's stop list:
/// `<route>/<direction or ->/<place>/<visit>`, each component escaped but with its colons
/// kept readable.
pub fn route_stop(
    route: &PublicId,
    direction: Option<u8>,
    place: &PublicId,
    visit: u32,
) -> PublicId {
    let readable = |component: &str| escape(component).replace("%3A", ":");
    let direction = direction.map_or_else(|| "-".to_owned(), |direction| direction.to_string());
    PublicId(
        format!(
            "{}/{}/{}/{visit}",
            readable(route.as_str()),
            readable(&direction),
            readable(place.as_str())
        )
        .into_boxed_str(),
    )
}

/// A shape the compiler generated is its geometry: `<feed>:shape:<method>:<20 hex>` of the
/// mode class and the points at seven decimals, so trips over the same path share one shape.
pub fn generated_shape(feed: Feed, method: &str, class: &str, points: &[crate::Coord]) -> PublicId {
    let mut hasher = Sha256::new();
    hasher.update(class.as_bytes());
    for point in points {
        hasher.update(
            format!(
                "
{:.7},{:.7}",
                point.lat, point.lon
            )
            .as_bytes(),
        );
    }
    let hex: String = hasher.finalize()[..10]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    PublicId(format!("{}:shape:{method}:{hex}", feed.prefix()).into_boxed_str())
}

/// A row that has no natural key (an assignment, a transfer): `<feed>:<kind>:<32 hex>` of
/// its defining fields, given in a fixed order by the projection that publishes it.
pub fn content(feed: Feed, kind: &str, fields: &[(&str, &str)]) -> PublicId {
    PublicId::from_content(&format!("{}:{kind}", feed.prefix()), fields)
}

/// The rail feed's scheme (`docs/rules/czptt.md`, section 4). Every text component is escaped.
pub mod czptt {
    use super::PublicId;

    /// The published form of a path (PA) or train (TR) identifier, `Pa:<company>:<core>:
    /// <variant>:<year>`. It is a source key's identifier and a trip's run key, not a public
    /// ID of ours; the components are verbatim.
    pub fn transport_identifier(
        object_type: &str,
        company: &str,
        core: &str,
        variant: &str,
        year: &str,
    ) -> String {
        format!("{object_type}:{company}:{core}:{variant}:{year}")
    }

    pub fn agency(responsible_ru: &str) -> PublicId {
        PublicId::build("czptt:agency", &[responsible_ru])
    }

    pub fn station(country: &str, primary_code: &str) -> PublicId {
        PublicId::build("czptt:stop", &[country, primary_code])
    }

    /// The boarding point of calls for which the source names no track.
    pub fn unspecified_point(station: &PublicId) -> PublicId {
        station.child(&["unspecified"])
    }

    pub fn track(station: &PublicId, subsidiary_code: &str) -> PublicId {
        station.child(&["platform", subsidiary_code])
    }

    /// Where a rail replacement bus calls at a station.
    pub fn replacement_bus_point(station: &PublicId) -> PublicId {
        station.child(&["platform", "BUS"])
    }

    pub fn line_route(line: &str, responsible_ru: &str, route_type: i32) -> PublicId {
        PublicId::build(
            "czptt:route:line",
            &[line, responsible_ru, &route_type.to_string()],
        )
    }

    /// A route for trains without a catalogued line: one per operator, category, route type
    /// and train number (or category, when there is no number).
    pub fn fallback_route(
        responsible_ru: &str,
        category: &str,
        route_type: i32,
        designation: &str,
    ) -> PublicId {
        PublicId::build(
            "czptt:route:fallback",
            &[
                responsible_ru,
                category,
                &route_type.to_string(),
                designation,
            ],
        )
    }

    /// One part of a path. `part` counts the path's segments from 1, including any that were
    /// dropped, so a part keeps its number.
    pub fn trip(path: &str, part: u16) -> PublicId {
        PublicId::build("czptt:trip", &[path, &part.to_string()])
    }

    /// `mode_block` is set when the path alternates between rail and replacement bus.
    pub fn block(path: &str, mode_block: Option<u16>) -> PublicId {
        match mode_block {
            Some(index) => PublicId::build("czptt:block", &[path, "mode", &index.to_string()]),
            None => PublicId::build("czptt:block", &[path]),
        }
    }

    /// `kind` is `central`, `noncentral` or `calendar`; `index` counts that kind's notes in
    /// the message from 1.
    pub fn note(path: &str, kind: &str, index: usize) -> PublicId {
        PublicId::build("czptt:note", &[path, kind, &index.to_string()])
    }
}

/// The road feed's scheme (`docs/rules/jdf-convert.md`, section 4). Stop and post numbers come
/// from the stop registry; text components are escaped.
pub mod jdf {
    use super::PublicId;
    use crate::time::Date;

    /// `number` is the registered stop number, or a provisional one.
    pub fn stop(number: u64) -> PublicId {
        PublicId::build("jdf:stop", &[&number.to_string()])
    }

    /// The boarding point of calls for which the source names no post.
    pub fn unspecified_point(stop: &PublicId) -> PublicId {
        stop.child(&["unspecified"])
    }

    /// A post the source lists with its own number (`Oznacniky`).
    pub fn authored_post(stop: &PublicId, post: u64) -> PublicId {
        stop.child(&["post", "id", &post.to_string()])
    }

    /// A post a call names only by a text designation.
    pub fn textual_post(stop: &PublicId, designation: &str) -> PublicId {
        stop.child(&["post", designation])
    }

    /// A post the estimator inferred; `ordinal` is pinned by the registry.
    pub fn estimated_post(stop: &PublicId, ordinal: u32) -> PublicId {
        stop.child(&["est", &ordinal.to_string()])
    }

    /// `distinction` tells apart operators' records that share a registration number.
    pub fn agency(registration: &str, distinction: u32) -> PublicId {
        PublicId::build("jdf:agency", &[registration, &distinction.to_string()])
    }

    /// One published route of a line: the regular one, its detour timetables, and (rarely) a
    /// further group whose presentation differs, told apart by `semantics`, eight hex
    /// characters derived from what differs.
    pub fn route(line: &str, detour: bool, semantics: Option<&str>) -> PublicId {
        let mut parts = vec![line];
        if detour {
            parts.push("detour");
        }
        parts.extend(semantics);
        PublicId::build("jdf:route", &parts)
    }

    /// A trip of a published line version: the line, the day the version's validity starts,
    /// whether it is a detour timetable, and the trip number. `hash` tells apart a second
    /// version starting the same day; `part` (from 2) versions published side by side.
    pub fn trip(
        line: &str,
        version_start: Date,
        detour: bool,
        hash: Option<&str>,
        part: Option<u32>,
        trip_number: u64,
    ) -> PublicId {
        let start = format!(
            "{:02}{:02}{:02}",
            version_start.year().rem_euclid(100),
            version_start.month(),
            version_start.day()
        );
        let part = part.map(|part| format!("p{part}"));
        let number = trip_number.to_string();
        let mut parts = vec![line, start.as_str()];
        if detour {
            parts.push("det");
        }
        parts.extend(hash);
        parts.extend(part.as_deref());
        parts.push(&number);
        PublicId::build("jdf:trip", &parts)
    }

    /// A note of a route (`Udaje`), a trip (`Caskody`) or a reservation rule (`Mistenky`), named
    /// after what it belongs to and the source row's own number.
    pub fn note(owner: &PublicId, kind: &str, number: u64) -> PublicId {
        owner.child(&["notice", kind, &number.to_string()])
    }

    /// A further note under the number of `note`: versions of a line published as one route
    /// number their notes independently, so one number can carry several texts. `variant`
    /// counts them from 2, in the order the versions are taken.
    pub fn note_variant(note: &PublicId, variant: usize) -> PublicId {
        note.child(&[&variant.to_string()])
    }
}

/// The regional overlay's scheme (`docs/rules/overlay.md`, section 4, verdict Q5): what a
/// regional feed adds to a national timetable. Posts, stop places, routes and agencies are
/// named after the source's own identifiers, in the form the published IDs and the reviewed
/// registry already use (a hash of the identifier, since source identifiers are long and not
/// ours to print). A date slice of a national trip and a trip a source adds are named after the
/// dates they run on.
pub mod overlay {
    use sha2::{Digest, Sha256};

    use super::PublicId;
    use crate::DateSet;

    /// The separator the scheme has always put between the parts of a hashed text.
    const UNIT: char = '\u{1f}';

    fn hex(digest: &[u8], characters: usize) -> String {
        let text: String = digest.iter().map(|byte| format!("{byte:02x}")).collect();
        text[..characters].to_owned()
    }

    fn sha(text: &str, characters: usize) -> String {
        hex(&Sha256::digest(text.as_bytes()), characters)
    }

    fn named(source: &str, kind: &str, hash: String) -> PublicId {
        PublicId(format!("overlay:{source}:{kind}:{hash}").into_boxed_str())
    }

    /// The part of a national trip that runs on `dates`, where the trip runs differently on
    /// its other days.
    pub fn slice(trip: &PublicId, dates: &DateSet) -> PublicId {
        let hash = hex(&Sha256::digest(dates.to_bytes()), 12);
        PublicId(format!("{trip}:overlay:{hash}").into_boxed_str())
    }

    /// A trip only the source has, running on `dates`.
    pub fn added_trip(source: &str, source_trip: &str, dates: &DateSet) -> PublicId {
        let mut hasher = Sha256::new();
        hasher.update(source.as_bytes());
        hasher.update([0x1f]);
        hasher.update(source_trip.as_bytes());
        hasher.update([0x1f]);
        hasher.update(dates.to_bytes());
        named(source, "trip", hex(&hasher.finalize(), 20))
    }

    /// A boarding point the source describes, named after the source's stop.
    pub fn post(source: &str, source_stop: &str) -> PublicId {
        named(source, "post", sha(source_stop, 16))
    }

    /// One boarding point that several sources describe at one place under one platform code
    /// (`platform` trimmed and upper-cased by the caller).
    pub fn shared_post(place: &PublicId, platform: &str) -> PublicId {
        named(
            "regional-all",
            "post",
            sha(&format!("{place}{UNIT}{platform}"), 16),
        )
    }

    /// A stop place only the source has, named after its stop group's key. The reviewed
    /// registry pins such places by that key and may give them another ID.
    pub fn stop_place(source: &str, group_key: &str) -> PublicId {
        named(source, "stop-place", sha(group_key, 16))
    }

    /// A source's stop place under the ID the reviewed registry pins for it (the registry's
    /// `place_id`, an ID this scheme issued when the place was first seen).
    pub fn registered_place(place_id: &str) -> PublicId {
        PublicId(place_id.into())
    }

    /// A route the source runs in its own mode or on its own: `line` is the national line it
    /// belongs to, or what stands for one when there is none.
    pub fn route(source: &str, source_route: &str, line: &str) -> PublicId {
        named(source, "route", sha(&format!("{source_route}|{line}"), 16))
    }

    pub fn agency(source: &str, source_agency: &str) -> PublicId {
        named(source, "agency", sha(source_agency, 16))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parts_are_escaped_so_separators_cannot_collide() {
        assert_eq!(
            PublicId::build("czptt:stop", &["CZ", "53414"]).as_str(),
            "czptt:stop:CZ:53414"
        );
        assert_eq!(
            PublicId::build("jdf:stop", &["1", "post", "A:1"]).as_str(),
            "jdf:stop:1:post:A%3A1"
        );
        assert_eq!(escape("Ústí n.L. hl.n."), "%C3%9Ast%C3%AD%20n.L.%20hl.n.");
    }

    #[test]
    fn content_ids_depend_on_every_field() {
        let a = PublicId::from_content("czptt:transfer", &[("from", "a"), ("to", "b")]);
        let b = PublicId::from_content("czptt:transfer", &[("from", "a"), ("to", "c")]);
        let c = PublicId::from_content("czptt:transfer", &[("from", "a\nto=b"), ("to", "")]);
        assert_ne!(a, b);
        assert_ne!(a, c);
        assert_eq!(a.as_str().len(), "czptt:transfer:".len() + 32);
    }

    #[test]
    fn the_rail_scheme_matches_the_published_ids() {
        let path = czptt::transport_identifier("Pa", "54", "000000001234", "00", "2026");
        assert_eq!(path, "Pa:54:000000001234:00:2026");
        assert_eq!(
            czptt::trip(&path, 2).as_str(),
            "czptt:trip:Pa%3A54%3A000000001234%3A00%3A2026:2"
        );
        let station = czptt::station("CZ", "54212");
        assert_eq!(
            czptt::track(&station, "1a").as_str(),
            "czptt:stop:CZ:54212:platform:1a"
        );
        assert_eq!(
            czptt::unspecified_point(&station).as_str(),
            "czptt:stop:CZ:54212:unspecified"
        );
        assert_eq!(
            czptt::line_route("5007", "3736", 106).as_str(),
            "czptt:route:line:5007:3736:106"
        );
        assert_eq!(
            czptt::fallback_route("54", "Os", 106, "01234").as_str(),
            "czptt:route:fallback:54:Os:106:01234"
        );
        assert_eq!(
            czptt::block(&path, Some(2)).as_str(),
            "czptt:block:Pa%3A54%3A000000001234%3A00%3A2026:mode:2"
        );
        let route = czptt::line_route("5007", "3736", 106);
        assert_eq!(
            route_stop(&route, None, &station, 1).as_str(),
            "czptt:route:line:5007:3736:106/-/czptt:stop:CZ:54212/1"
        );
    }

    #[test]
    fn the_road_scheme_matches_the_published_ids() {
        use jiff::civil::date;
        let stop = jdf::stop(4711);
        assert_eq!(
            jdf::unspecified_point(&stop).as_str(),
            "jdf:stop:4711:unspecified"
        );
        assert_eq!(
            jdf::authored_post(&stop, 3).as_str(),
            "jdf:stop:4711:post:id:3"
        );
        assert_eq!(
            jdf::textual_post(&stop, "A 1").as_str(),
            "jdf:stop:4711:post:A%201"
        );
        assert_eq!(
            jdf::estimated_post(&stop, 2).as_str(),
            "jdf:stop:4711:est:2"
        );
        assert_eq!(jdf::agency("00005886", 1).as_str(), "jdf:agency:00005886:1");
        assert_eq!(
            jdf::route("199007", true, None).as_str(),
            "jdf:route:199007:detour"
        );
        assert_eq!(
            jdf::route("199007", false, Some("0a1b2c3d")).as_str(),
            "jdf:route:199007:0a1b2c3d"
        );
        let start = date(2026, 11, 13);
        assert_eq!(
            jdf::trip("199004", start, false, None, None, 3042).as_str(),
            "jdf:trip:199004:261113:3042"
        );
        assert_eq!(
            jdf::trip("199007", start, true, None, None, 2010).as_str(),
            "jdf:trip:199007:261113:det:2010"
        );
        assert_eq!(
            jdf::trip("199007", start, false, Some("0a1b2c3d"), Some(2), 7).as_str(),
            "jdf:trip:199007:261113:0a1b2c3d:p2:7"
        );
    }
}
