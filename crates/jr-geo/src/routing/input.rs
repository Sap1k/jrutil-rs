//! What the graph is built from: OSM nodes, ways and relations as plain views. The command line
//! feeds them from the PBF reader; tests build them by hand.

use jr_model::Coord;

/// A tag list in source order.
pub type OsmTags<'a> = &'a [(&'a str, &'a str)];

/// An OSM node; `tags` is empty for an untagged node.
#[derive(Clone, Copy, Debug)]
pub struct OsmNode<'a> {
    pub id: i64,
    pub coord: Coord,
    pub tags: OsmTags<'a>,
}

/// An OSM way: its node references in order.
#[derive(Clone, Copy, Debug)]
pub struct OsmWay<'a> {
    pub id: i64,
    pub nodes: &'a [i64],
    pub tags: OsmTags<'a>,
}

/// The type of a relation member.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OsmMemberType {
    Node,
    Way,
    Relation,
}

/// A relation member.
#[derive(Clone, Copy, Debug)]
pub struct OsmMember<'a> {
    pub kind: OsmMemberType,
    pub id: i64,
    pub role: &'a str,
}

/// An OSM relation.
#[derive(Clone, Copy, Debug)]
pub struct OsmRelation<'a> {
    pub id: i64,
    pub tags: OsmTags<'a>,
    pub members: &'a [OsmMember<'a>],
}

/// The value of the first tag with the key.
pub(super) fn tag<'a>(tags: OsmTags<'a>, key: &str) -> Option<&'a str> {
    tags.iter()
        .find(|(k, _)| *k == key)
        .map(|&(_, value)| value)
}

/// The value trimmed and lower-cased.
pub(super) fn normalized(value: &str) -> std::borrow::Cow<'_, str> {
    let value = value.trim();
    if value
        .bytes()
        .any(|byte| byte.is_ascii_uppercase() || !byte.is_ascii())
    {
        value.to_lowercase().into()
    } else {
        value.into()
    }
}

/// Whether the tag's value, trimmed and lower-cased, is one of `values`.
pub(super) fn tag_in(tags: OsmTags<'_>, key: &str, values: &[Box<str>]) -> bool {
    tag(tags, key).is_some_and(|value| {
        let value = normalized(value);
        values.iter().any(|candidate| **candidate == *value)
    })
}

/// Whether the tag's value, trimmed and lower-cased, is `value`.
pub(super) fn tag_is(tags: OsmTags<'_>, key: &str, value: &str) -> bool {
    tag(tags, key).is_some_and(|found| normalized(found) == value)
}
