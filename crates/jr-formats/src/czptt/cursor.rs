//! A pull cursor over one XML document: elements by local name, their text, no DOM.
//!
//! Element names borrow the input; the only buffer is the text of the element being read, which
//! is reused.

use std::borrow::Cow;

use quick_xml::Reader;
use quick_xml::escape::resolve_predefined_entity;
use quick_xml::events::{BytesStart, Event};

use super::CzpttError;

pub(super) struct Cursor<'a> {
    reader: Reader<&'a [u8]>,
    text: String,
}

/// An element whose start tag has been read.
pub(super) struct Element<'a> {
    start: BytesStart<'a>,
    empty: bool,
}

impl Element<'_> {
    /// The local name (any namespace prefix removed).
    pub fn name(&self) -> &str {
        local(self.start.name().into_inner())
    }

    /// An attribute's value by local name.
    pub fn attribute(&self, name: &str) -> Result<Option<Cow<'_, str>>, CzpttError> {
        for attribute in self.start.attributes().with_checks(false) {
            let attribute = attribute.map_err(|error| CzpttError::Xml {
                position: 0,
                source: error.into(),
            })?;
            if local(attribute.key.into_inner()) == name {
                let value = attribute
                    .normalized_value(quick_xml::XmlVersion::Implicit1_0)
                    .map_err(|source| CzpttError::Xml {
                        position: 0,
                        source,
                    })?;
                return Ok(Some(value));
            }
        }
        Ok(None)
    }
}

fn local(qualified: &str) -> &str {
    qualified
        .rsplit_once(':')
        .map_or(qualified, |(_, name)| name)
}

impl<'a> Cursor<'a> {
    pub fn new(xml: &'a str) -> Self {
        Self {
            reader: Reader::from_str(xml.strip_prefix('\u{feff}').unwrap_or(xml)),
            text: String::new(),
        }
    }

    fn event(&mut self) -> Result<Event<'a>, CzpttError> {
        self.reader.read_event().map_err(|source| CzpttError::Xml {
            position: self.reader.error_position(),
            source,
        })
    }

    /// The root element, or `None` for a document without one.
    pub fn root(&mut self) -> Result<Option<Element<'a>>, CzpttError> {
        loop {
            match self.event()? {
                Event::Start(start) => {
                    return Ok(Some(Element {
                        start,
                        empty: false,
                    }));
                }
                Event::Empty(start) => return Ok(Some(Element { start, empty: true })),
                Event::Eof => return Ok(None),
                _ => {}
            }
        }
    }

    /// The next child of `parent`, or `None` once its end tag has been read. Text between
    /// children is ignored. Every child returned must be consumed (`text`, `skip` or by reading
    /// its children) before the next call.
    pub fn child(&mut self, parent: &Element<'_>) -> Result<Option<Element<'a>>, CzpttError> {
        if parent.empty {
            return Ok(None);
        }
        loop {
            match self.event()? {
                Event::Start(start) => {
                    return Ok(Some(Element {
                        start,
                        empty: false,
                    }));
                }
                Event::Empty(start) => return Ok(Some(Element { start, empty: true })),
                Event::End(_) => return Ok(None),
                Event::Eof => return Err(self.eof()),
                _ => {}
            }
        }
    }

    /// Skips an element and everything in it.
    pub fn skip(&mut self, element: &Element<'_>) -> Result<(), CzpttError> {
        if element.empty {
            return Ok(());
        }
        self.reader
            .read_to_end(element.start.name())
            .map(|_| ())
            .map_err(|source| CzpttError::Xml {
                position: self.reader.error_position(),
                source,
            })
    }

    /// The element's text with surrounding whitespace trimmed, `None` if that is empty. Text of
    /// nested elements is not included.
    pub fn text(&mut self, element: &Element<'_>) -> Result<Option<&str>, CzpttError> {
        self.text.clear();
        if element.empty {
            return Ok(None);
        }
        let mut depth = 0u32;
        loop {
            match self.event()? {
                Event::Text(text) if depth == 0 => self.text.push_str(&text.xml10_content()),
                Event::CData(text) if depth == 0 => self.text.push_str(&text.xml10_content()),
                Event::GeneralRef(reference) if depth == 0 => {
                    let resolved =
                        reference
                            .resolve_char_ref()
                            .map_err(|source| CzpttError::Xml {
                                position: self.reader.buffer_position(),
                                source,
                            })?;
                    match resolved {
                        Some(char) => self.text.push(char),
                        None => match resolve_predefined_entity(&reference) {
                            Some(text) => self.text.push_str(text),
                            None => {
                                return Err(CzpttError::Entity(
                                    reference.into_inner().into_owned().into(),
                                ));
                            }
                        },
                    }
                }
                Event::Start(_) => depth += 1,
                Event::End(_) if depth == 0 => break,
                Event::End(_) => depth -= 1,
                Event::Eof => return Err(self.eof()),
                _ => {}
            }
        }
        let text = self.text.trim();
        Ok((!text.is_empty()).then_some(text))
    }

    fn eof(&self) -> CzpttError {
        CzpttError::Xml {
            position: self.reader.buffer_position(),
            source: quick_xml::Error::Syntax(quick_xml::errors::SyntaxError::UnclosedTag),
        }
    }
}
