//! One decoded `PrimitiveBlock`: flat element tables over the block's string table.

use jr_model::Coord;

use super::protobuf::{Field, Message, Varints};
use super::{Elements, OsmError};

/// The kind of an OSM element.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ElementKind {
    Node,
    Way,
    Relation,
}

/// The type of a relation member (`Relation.MemberType`).
pub type MemberKind = ElementKind;

/// The elements of one `PrimitiveBlock`, in flat tables.
#[derive(Debug, Default)]
pub struct Block {
    /// The valid UTF-8 entries of the string table, concatenated; `strings[i]` is the byte range
    /// of entry `i`, `None` when the entry is not UTF-8 (an error only if an element uses it).
    text: String,
    strings: Vec<Option<(u32, u32)>>,
    /// `(key, value)` string indexes of every element's tags.
    tags: Vec<(u32, u32)>,
    node_ids: Vec<i64>,
    node_coords: Vec<Coord>,
    node_tags: Vec<(u32, u32)>,
    way_ids: Vec<i64>,
    way_refs: Vec<(u32, u32)>,
    refs: Vec<i64>,
    way_tags: Vec<(u32, u32)>,
    relation_ids: Vec<i64>,
    relation_members: Vec<(u32, u32)>,
    members: Vec<(MemberKind, i64, u32)>,
    relation_tags: Vec<(u32, u32)>,
    /// The groups in file order: their kind and the range of element indexes they hold.
    groups: Vec<(ElementKind, u32, u32)>,
}

/// A node.
#[derive(Clone, Copy, Debug)]
pub struct NodeView<'a> {
    pub id: i64,
    pub coord: Coord,
    pub tags: Tags<'a>,
}

/// A way: its node references in order.
#[derive(Clone, Copy, Debug)]
pub struct WayView<'a> {
    pub id: i64,
    pub refs: &'a [i64],
    pub tags: Tags<'a>,
}

/// A relation.
#[derive(Clone, Copy, Debug)]
pub struct RelationView<'a> {
    pub id: i64,
    pub members: Members<'a>,
    pub tags: Tags<'a>,
}

/// An element of a block.
#[derive(Clone, Copy, Debug)]
pub enum Element<'a> {
    Node(NodeView<'a>),
    Way(WayView<'a>),
    Relation(RelationView<'a>),
}

/// An element's tags in file order.
#[derive(Clone, Copy, Debug)]
pub struct Tags<'a> {
    block: &'a Block,
    pairs: &'a [(u32, u32)],
}

impl<'a> Tags<'a> {
    pub fn len(&self) -> usize {
        self.pairs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.pairs.is_empty()
    }

    pub fn iter(&self) -> impl ExactSizeIterator<Item = (&'a str, &'a str)> + 'a {
        let block = self.block;
        self.pairs
            .iter()
            .map(move |&(key, value)| (block.string(key), block.string(value)))
    }

    /// The value of the first tag with this key.
    pub fn get(&self, key: &str) -> Option<&'a str> {
        self.iter().find(|(k, _)| *k == key).map(|(_, value)| value)
    }
}

/// One relation member.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Member<'a> {
    pub kind: MemberKind,
    pub id: i64,
    pub role: &'a str,
}

/// A relation's members in file order.
#[derive(Clone, Copy, Debug)]
pub struct Members<'a> {
    block: &'a Block,
    members: &'a [(MemberKind, i64, u32)],
}

impl<'a> Members<'a> {
    pub fn len(&self) -> usize {
        self.members.len()
    }

    pub fn is_empty(&self) -> bool {
        self.members.is_empty()
    }

    pub fn iter(&self) -> impl ExactSizeIterator<Item = Member<'a>> + 'a {
        let block = self.block;
        self.members.iter().map(move |&(kind, id, role)| Member {
            kind,
            id,
            role: block.string(role),
        })
    }
}

fn slice<T>(items: &[T], (start, end): (u32, u32)) -> &[T] {
    &items[start as usize..end as usize]
}

impl Block {
    /// The string table entry; only called for indexes checked while decoding.
    fn string(&self, index: u32) -> &str {
        self.strings[index as usize]
            .map_or("", |(start, end)| &self.text[start as usize..end as usize])
    }

    fn tags_of(&self, range: (u32, u32)) -> Tags<'_> {
        Tags {
            block: self,
            pairs: slice(&self.tags, range),
        }
    }

    pub fn node(&self, index: usize) -> NodeView<'_> {
        NodeView {
            id: self.node_ids[index],
            coord: self.node_coords[index],
            tags: self.tags_of(self.node_tags[index]),
        }
    }

    pub fn way(&self, index: usize) -> WayView<'_> {
        WayView {
            id: self.way_ids[index],
            refs: slice(&self.refs, self.way_refs[index]),
            tags: self.tags_of(self.way_tags[index]),
        }
    }

    pub fn relation(&self, index: usize) -> RelationView<'_> {
        RelationView {
            id: self.relation_ids[index],
            members: Members {
                block: self,
                members: slice(&self.members, self.relation_members[index]),
            },
            tags: self.tags_of(self.relation_tags[index]),
        }
    }

    pub fn node_count(&self) -> usize {
        self.node_ids.len()
    }

    pub fn way_count(&self) -> usize {
        self.way_ids.len()
    }

    pub fn relation_count(&self) -> usize {
        self.relation_ids.len()
    }

    /// The nodes in file order.
    pub fn nodes(&self) -> impl ExactSizeIterator<Item = NodeView<'_>> {
        (0..self.node_count()).map(|index| self.node(index))
    }

    /// The ways in file order.
    pub fn ways(&self) -> impl ExactSizeIterator<Item = WayView<'_>> {
        (0..self.way_count()).map(|index| self.way(index))
    }

    /// The relations in file order.
    pub fn relations(&self) -> impl ExactSizeIterator<Item = RelationView<'_>> {
        (0..self.relation_count()).map(|index| self.relation(index))
    }

    /// Every element in file order.
    pub fn elements(&self) -> impl Iterator<Item = Element<'_>> {
        self.groups.iter().flat_map(move |&(kind, start, end)| {
            (start as usize..end as usize).map(move |index| match kind {
                ElementKind::Node => Element::Node(self.node(index)),
                ElementKind::Way => Element::Way(self.way(index)),
                ElementKind::Relation => Element::Relation(self.relation(index)),
            })
        })
    }
}

/// The coordinate encoding of a block.
struct Encoding {
    granularity: i64,
    lat_offset: i64,
    lon_offset: i64,
}

impl Encoding {
    fn coord(&self, lat: i64, lon: i64) -> Coord {
        let degrees = |offset: i64, value: i64| (offset + self.granularity * value) as f64 / 1e9;
        Coord::new(degrees(self.lon_offset, lon), degrees(self.lat_offset, lat))
    }
}

fn index(value: u64) -> Result<u32, OsmError> {
    u32::try_from(value).map_err(|_| OsmError::StringIndex(value))
}

fn count(length: usize) -> u32 {
    // A block is at most 32 MiB, so it holds fewer than 2^32 of anything.
    u32::try_from(length).expect("a block holds fewer than 2^32 items")
}

/// Decodes a `PrimitiveBlock`.
pub(super) fn decode(bytes: &[u8], elements: Elements) -> Result<Block, OsmError> {
    let mut block = Block::default();
    // The string table and the coordinate encoding may follow the groups: read them first.
    let mut encoding = Encoding {
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
                        let entry = std::str::from_utf8(text).ok().map(|text| {
                            let start = count(block.text.len());
                            block.text.push_str(text);
                            (start, count(block.text.len()))
                        });
                        block.strings.push(entry);
                    }
                }
            }
            (17, Field::Varint(value)) => encoding.granularity = value as i32 as i64,
            (19, Field::Varint(value)) => encoding.lat_offset = value as i64,
            (20, Field::Varint(value)) => encoding.lon_offset = value as i64,
            _ => {}
        }
    }
    let mut message = Message::new(bytes, "PrimitiveBlock");
    while let Some((number, field)) = message.next()? {
        if let (2, Field::Bytes(group)) = (number, field) {
            let mut group = Message::new(group, "PrimitiveGroup");
            while let Some((number, field)) = group.next()? {
                match (number, field) {
                    (1, Field::Bytes(node)) => block.group(ElementKind::Node, |block| {
                        block.plain_node(&encoding, node, elements)
                    })?,
                    (2, Field::Bytes(dense)) => block.group(ElementKind::Node, |block| {
                        block.dense_nodes(&encoding, dense, elements)
                    })?,
                    (3, Field::Bytes(way)) if elements.ways => {
                        block.group(ElementKind::Way, |block| block.way_message(way))?
                    }
                    (4, Field::Bytes(relation)) if elements.relations => block
                        .group(ElementKind::Relation, |block| {
                            block.relation_message(relation)
                        })?,
                    _ => {}
                }
            }
        }
    }
    Ok(block)
}

impl Block {
    fn element_count(&self, kind: ElementKind) -> u32 {
        count(match kind {
            ElementKind::Node => self.node_ids.len(),
            ElementKind::Way => self.way_ids.len(),
            ElementKind::Relation => self.relation_ids.len(),
        })
    }

    /// Runs `decode`, which appends elements of `kind`, and records them as (part of) a group.
    fn group(
        &mut self,
        kind: ElementKind,
        decode: impl FnOnce(&mut Block) -> Result<(), OsmError>,
    ) -> Result<(), OsmError> {
        let start = self.element_count(kind);
        decode(self)?;
        let end = self.element_count(kind);
        if end > start {
            match self.groups.last_mut() {
                Some((last, _, last_end)) if *last == kind && *last_end == start => *last_end = end,
                _ => self.groups.push((kind, start, end)),
            }
        }
        Ok(())
    }

    /// Checks a string index and returns it.
    fn checked(&self, value: u64) -> Result<u32, OsmError> {
        let index = index(value)?;
        match self.strings.get(index as usize) {
            Some(Some(_)) => Ok(index),
            Some(None) => Err(OsmError::Utf8),
            None => Err(OsmError::StringIndex(value)),
        }
    }

    /// Appends key/value pairs and returns their range.
    fn push_tags(&mut self, keys: &[u64], values: &[u64]) -> Result<(u32, u32), OsmError> {
        let start = count(self.tags.len());
        for (&key, &value) in keys.iter().zip(values) {
            let pair = (self.checked(key)?, self.checked(value)?);
            self.tags.push(pair);
        }
        Ok((start, count(self.tags.len())))
    }

    /// `Node`: `id` (1, sint64), `keys` (2) and `vals` (3) as packed string indexes, `lat` (8)
    /// and `lon` (9) as sint64.
    fn plain_node(
        &mut self,
        encoding: &Encoding,
        bytes: &[u8],
        elements: Elements,
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
        if keys.is_empty() && !elements.untagged_nodes {
            return Ok(());
        }
        if keys.len() != values.len() {
            return Err(OsmError::Malformed("Node"));
        }
        let tags = self.push_tags(&keys, &values)?;
        self.node_ids.push(id);
        self.node_coords.push(encoding.coord(lat, lon));
        self.node_tags.push(tags);
        Ok(())
    }

    /// `DenseNodes`: delta-coded `id` (1), `lat` (8), `lon` (9), and `keys_vals` (10): per
    /// node, key/value string indexes ended by `0`.
    fn dense_nodes(
        &mut self,
        encoding: &Encoding,
        bytes: &[u8],
        elements: Elements,
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
        let mut ids = Varints::new(ids.unwrap_or_default(), "DenseNodes");
        let mut lats = Varints::new(lats.unwrap_or_default(), "DenseNodes");
        let mut lons = Varints::new(lons.unwrap_or_default(), "DenseNodes");
        let mut keys_vals = Varints::new(keys_vals.unwrap_or_default(), "DenseNodes");
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
            let start = count(self.tags.len());
            if has_tags {
                loop {
                    let key = keys_vals.next()?.ok_or(OsmError::Malformed("DenseNodes"))?;
                    if key == 0 {
                        break;
                    }
                    let value = keys_vals.next()?.ok_or(OsmError::Malformed("DenseNodes"))?;
                    let pair = (self.checked(key)?, self.checked(value)?);
                    self.tags.push(pair);
                }
            }
            let end = count(self.tags.len());
            if end == start && !elements.untagged_nodes {
                continue;
            }
            self.node_ids.push(id);
            self.node_coords.push(encoding.coord(lat, lon));
            self.node_tags.push((start, end));
        }
        Ok(())
    }

    /// `Way`: `id` (1, int64), `keys` (2), `vals` (3), `refs` (8, packed delta-coded sint64).
    fn way_message(&mut self, bytes: &[u8]) -> Result<(), OsmError> {
        let mut id = 0;
        let (mut keys, mut values) = (Vec::new(), Vec::new());
        let start = count(self.refs.len());
        let mut message = Message::new(bytes, "Way");
        while let Some((number, field)) = message.next()? {
            match (number, field) {
                (1, Field::Varint(value)) => id = value as i64,
                (2, field) => field.each_varint("Way", |key| keys.push(key))?,
                (3, field) => field.each_varint("Way", |value| values.push(value))?,
                (8, field) => {
                    let mut node = 0i64;
                    let refs = &mut self.refs;
                    field.each_varint("Way", |delta| {
                        node += super::protobuf::zigzag(delta);
                        refs.push(node);
                    })?;
                }
                (1, _) => return Err(OsmError::Malformed("Way")),
                _ => {}
            }
        }
        if keys.len() != values.len() {
            return Err(OsmError::Malformed("Way"));
        }
        let refs = (start, count(self.refs.len()));
        let tags = self.push_tags(&keys, &values)?;
        self.way_ids.push(id);
        self.way_refs.push(refs);
        self.way_tags.push(tags);
        Ok(())
    }

    /// `Relation`: `id` (1, int64), `keys` (2), `vals` (3), `roles_sid` (8, packed int32),
    /// `memids` (9, packed delta-coded sint64), `types` (10, packed enum: node, way, relation).
    fn relation_message(&mut self, bytes: &[u8]) -> Result<(), OsmError> {
        let mut id = 0;
        let (mut keys, mut values) = (Vec::new(), Vec::new());
        let (mut roles, mut ids, mut types) = (Vec::new(), Vec::new(), Vec::new());
        let mut message = Message::new(bytes, "Relation");
        while let Some((number, field)) = message.next()? {
            match (number, field) {
                (1, Field::Varint(value)) => id = value as i64,
                (2, field) => field.each_varint("Relation", |key| keys.push(key))?,
                (3, field) => field.each_varint("Relation", |value| values.push(value))?,
                (8, field) => field.each_varint("Relation", |role| roles.push(role))?,
                (9, field) => {
                    let mut member = 0i64;
                    field.each_varint("Relation", |delta| {
                        member += super::protobuf::zigzag(delta);
                        ids.push(member);
                    })?;
                }
                (10, field) => field.each_varint("Relation", |kind| types.push(kind))?,
                (1, _) => return Err(OsmError::Malformed("Relation")),
                _ => {}
            }
        }
        if keys.len() != values.len() || roles.len() != ids.len() || ids.len() != types.len() {
            return Err(OsmError::Malformed("Relation"));
        }
        let start = count(self.members.len());
        for ((&role, &member), &kind) in roles.iter().zip(&ids).zip(&types) {
            let kind = match kind {
                0 => MemberKind::Node,
                1 => MemberKind::Way,
                2 => MemberKind::Relation,
                other => return Err(OsmError::MemberType(other)),
            };
            // `roles_sid` is an int32 written as a varint.
            let role = self.checked(u64::from(role as u32))?;
            self.members.push((kind, member, role));
        }
        let members = (start, count(self.members.len()));
        let tags = self.push_tags(&keys, &values)?;
        self.relation_ids.push(id);
        self.relation_members.push(members);
        self.relation_tags.push(tags);
        Ok(())
    }
}
