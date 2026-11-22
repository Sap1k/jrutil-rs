//! The protobuf wire format, as much as OSM PBF needs.

use super::OsmError;

/// One field's payload.
pub(super) enum Field<'a> {
    Varint(u64),
    Bytes(&'a [u8]),
    /// A fixed 32- or 64-bit value; not used by OSM PBF.
    Fixed,
}

impl Field<'_> {
    pub fn sint64(&self, what: &'static str) -> Result<i64, OsmError> {
        match self {
            Field::Varint(value) => Ok(zigzag(*value)),
            _ => Err(OsmError::Malformed(what)),
        }
    }

    /// A repeated varint field, packed or not.
    pub fn each_varint(&self, what: &'static str, mut f: impl FnMut(u64)) -> Result<(), OsmError> {
        match self {
            Field::Varint(value) => f(*value),
            Field::Bytes(packed) => {
                let mut values = Varints::new(packed, what);
                while let Some(value) = values.next()? {
                    f(value);
                }
            }
            Field::Fixed => return Err(OsmError::Malformed(what)),
        }
        Ok(())
    }
}

fn zigzag(value: u64) -> i64 {
    ((value >> 1) as i64) ^ -((value & 1) as i64)
}

fn varint(bytes: &mut &[u8], what: &'static str) -> Result<u64, OsmError> {
    let mut value = 0u64;
    for shift in (0..64).step_by(7) {
        let (&byte, rest) = bytes.split_first().ok_or(OsmError::Malformed(what))?;
        *bytes = rest;
        value |= u64::from(byte & 0x7f) << shift;
        if byte & 0x80 == 0 {
            return Ok(value);
        }
    }
    Err(OsmError::Malformed(what))
}

/// The fields of one message, in order.
pub(super) struct Message<'a> {
    bytes: &'a [u8],
    what: &'static str,
}

impl<'a> Message<'a> {
    pub fn new(bytes: &'a [u8], what: &'static str) -> Self {
        Self { bytes, what }
    }

    pub fn next(&mut self) -> Result<Option<(u32, Field<'a>)>, OsmError> {
        if self.bytes.is_empty() {
            return Ok(None);
        }
        let key = varint(&mut self.bytes, self.what)?;
        let number = u32::try_from(key >> 3).map_err(|_| OsmError::Malformed(self.what))?;
        let field = match key & 7 {
            0 => Field::Varint(varint(&mut self.bytes, self.what)?),
            1 => Field::Fixed.skip(&mut self.bytes, 8, self.what)?,
            2 => {
                let length = varint(&mut self.bytes, self.what)?;
                let length = usize::try_from(length).map_err(|_| OsmError::Malformed(self.what))?;
                if length > self.bytes.len() {
                    return Err(OsmError::Malformed(self.what));
                }
                let (value, rest) = self.bytes.split_at(length);
                self.bytes = rest;
                Field::Bytes(value)
            }
            5 => Field::Fixed.skip(&mut self.bytes, 4, self.what)?,
            _ => return Err(OsmError::Malformed(self.what)),
        };
        Ok(Some((number, field)))
    }
}

impl<'a> Field<'a> {
    fn skip(self, bytes: &mut &[u8], width: usize, what: &'static str) -> Result<Self, OsmError> {
        if bytes.len() < width {
            return Err(OsmError::Malformed(what));
        }
        *bytes = &bytes[width..];
        Ok(self)
    }
}

/// A packed run of varints.
pub(super) struct Varints<'a> {
    bytes: &'a [u8],
    what: &'static str,
}

impl<'a> Varints<'a> {
    pub fn new(bytes: &'a [u8], what: &'static str) -> Self {
        Self { bytes, what }
    }

    pub fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }

    pub fn next(&mut self) -> Result<Option<u64>, OsmError> {
        if self.bytes.is_empty() {
            return Ok(None);
        }
        varint(&mut self.bytes, self.what).map(Some)
    }

    pub fn next_sint64(&mut self) -> Result<Option<i64>, OsmError> {
        Ok(self.next()?.map(zigzag))
    }
}
