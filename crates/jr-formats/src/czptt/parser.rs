//! The message grammar: which elements make up a message and which are mandatory.

mod location;

use super::cursor::{Cursor, Element};
use super::{
    Calendar, CancellationMessage, CzpttError, Identifier, ObjectType, Parameter, Timestamp,
    TimetableMessage, datetime,
};

/// Element paths of one identifier kind, for errors.
struct IdentifierPaths {
    object_type: &'static str,
    company: &'static str,
    core: &'static str,
    variant: &'static str,
    timetable_year: &'static str,
}

const PLANNED: IdentifierPaths = IdentifierPaths {
    object_type: "PlannedTransportIdentifiers/ObjectType",
    company: "PlannedTransportIdentifiers/Company",
    core: "PlannedTransportIdentifiers/Core",
    variant: "PlannedTransportIdentifiers/Variant",
    timetable_year: "PlannedTransportIdentifiers/TimetableYear",
};

const RELATED: IdentifierPaths = IdentifierPaths {
    object_type: "RelatedPlannedTransportIdentifiers/ObjectType",
    company: "RelatedPlannedTransportIdentifiers/Company",
    core: "RelatedPlannedTransportIdentifiers/Core",
    variant: "RelatedPlannedTransportIdentifiers/Variant",
    timetable_year: "RelatedPlannedTransportIdentifiers/TimetableYear",
};

pub(super) struct Parser<'a> {
    cursor: Cursor<'a>,
    /// The `CZPTTLocation` being read, for errors.
    location: Option<usize>,
    /// Scratch for pairing parameter names with values.
    names: Vec<Box<str>>,
    values: Vec<Box<str>>,
}

/// A value slot that may be filled once: the outer `Option` says whether the element occurred.
type Once<T> = Option<T>;

impl<'a> Parser<'a> {
    pub(super) fn new(cursor: Cursor<'a>) -> Self {
        Self {
            cursor,
            location: None,
            names: Vec::new(),
            values: Vec::new(),
        }
    }

    fn missing(&self, element: &'static str) -> CzpttError {
        CzpttError::Missing {
            element,
            location: self.location,
        }
    }

    fn invalid(&self, element: &'static str, value: &str) -> CzpttError {
        CzpttError::Invalid {
            element,
            location: self.location,
            value: value.into(),
        }
    }

    fn once<T>(
        &self,
        slot: &mut Once<T>,
        value: T,
        element: &'static str,
    ) -> Result<(), CzpttError> {
        if slot.is_some() {
            return Err(CzpttError::Repeated {
                element,
                location: self.location,
            });
        }
        *slot = Some(value);
        Ok(())
    }

    fn text(&mut self, element: &Element<'_>) -> Result<Option<Box<str>>, CzpttError> {
        Ok(self.cursor.text(element)?.map(Box::from))
    }

    /// Reads an optional text element into a once-slot.
    fn text_once(
        &mut self,
        slot: &mut Once<Option<Box<str>>>,
        element: &Element<'_>,
        path: &'static str,
    ) -> Result<(), CzpttError> {
        let text = self.text(element)?;
        self.once(slot, text, path)
    }

    /// A mandatory text from a once-slot.
    fn required(
        &self,
        slot: Once<Option<Box<str>>>,
        path: &'static str,
    ) -> Result<Box<str>, CzpttError> {
        slot.flatten().ok_or_else(|| self.missing(path))
    }

    fn timestamp(
        &mut self,
        element: &Element<'_>,
        path: &'static str,
    ) -> Result<Timestamp, CzpttError> {
        let text = self.cursor.text(element)?.ok_or(CzpttError::Missing {
            element: path,
            location: self.location,
        })?;
        match datetime::date_time(text) {
            Some((local, offset_seconds)) => Ok(Timestamp {
                local,
                offset_seconds,
            }),
            None => Err(CzpttError::Invalid {
                element: path,
                location: self.location,
                value: text.into(),
            }),
        }
    }

    pub(super) fn timetable(mut self, root: &Element<'_>) -> Result<TimetableMessage, CzpttError> {
        let mut identifiers = Vec::new();
        let mut related = Vec::new();
        let mut creation = None;
        let mut calendar = None;
        let mut locations = Vec::new();
        let mut parameters = Vec::new();
        while let Some(child) = self.cursor.child(root)? {
            match child.name() {
                "Identifiers" => {
                    while let Some(item) = self.cursor.child(&child)? {
                        match item.name() {
                            "PlannedTransportIdentifiers" => {
                                identifiers.push(self.identifier(&item, &PLANNED)?);
                            }
                            "RelatedPlannedTransportIdentifiers" => {
                                related.push(self.identifier(&item, &RELATED)?);
                            }
                            _ => self.cursor.skip(&item)?,
                        }
                    }
                }
                "RelatedPlannedTransportIdentifiers" => {
                    related.push(self.identifier(&child, &RELATED)?);
                }
                "CZPTTCreation" => {
                    let value = self.timestamp(&child, "CZPTTCreation")?;
                    self.once(&mut creation, value, "CZPTTCreation")?;
                }
                "CZPTTInformation" => {
                    while let Some(item) = self.cursor.child(&child)? {
                        match item.name() {
                            "CZPTTLocation" => {
                                self.location = Some(locations.len());
                                let location = self.location(&item)?;
                                self.location = None;
                                locations.push(location);
                            }
                            "PlannedCalendar" => {
                                let value = self.calendar(&item)?;
                                self.once(
                                    &mut calendar,
                                    value,
                                    "CZPTTInformation/PlannedCalendar",
                                )?;
                            }
                            _ => self.cursor.skip(&item)?,
                        }
                    }
                }
                "NetworkSpecificParameter" => self.parameters(&child, &mut parameters)?,
                _ => self.cursor.skip(&child)?,
            }
        }
        if identifiers.is_empty() {
            return Err(self.missing("Identifiers/PlannedTransportIdentifiers"));
        }
        Ok(TimetableMessage {
            identifiers,
            related,
            creation: creation.ok_or_else(|| self.missing("CZPTTCreation"))?,
            calendar: calendar.ok_or_else(|| self.missing("CZPTTInformation/PlannedCalendar"))?,
            locations,
            parameters,
        })
    }

    pub(super) fn cancellation(
        mut self,
        root: &Element<'_>,
    ) -> Result<CancellationMessage, CzpttError> {
        let mut identifiers = Vec::new();
        let mut cancellation = None;
        let mut calendar = None;
        while let Some(child) = self.cursor.child(root)? {
            match child.name() {
                "PlannedTransportIdentifiers" => {
                    identifiers.push(self.identifier(&child, &PLANNED)?);
                }
                "CZPTTCancelation" => {
                    let value = self.timestamp(&child, "CZPTTCancelation")?;
                    self.once(&mut cancellation, value, "CZPTTCancelation")?;
                }
                "PlannedCalendar" => {
                    let value = self.calendar(&child)?;
                    self.once(&mut calendar, value, "PlannedCalendar")?;
                }
                _ => self.cursor.skip(&child)?,
            }
        }
        if identifiers.is_empty() {
            return Err(self.missing("PlannedTransportIdentifiers"));
        }
        Ok(CancellationMessage {
            identifiers,
            cancellation: cancellation.ok_or_else(|| self.missing("CZPTTCancelation"))?,
            calendar: calendar.ok_or_else(|| self.missing("PlannedCalendar"))?,
        })
    }

    fn identifier(
        &mut self,
        element: &Element<'_>,
        paths: &IdentifierPaths,
    ) -> Result<Identifier, CzpttError> {
        let (mut object_type, mut company, mut core, mut variant, mut year) =
            (None, None, None, None, None);
        while let Some(child) = self.cursor.child(element)? {
            match child.name() {
                "ObjectType" => self.text_once(&mut object_type, &child, paths.object_type)?,
                "Company" => self.text_once(&mut company, &child, paths.company)?,
                "Core" => self.text_once(&mut core, &child, paths.core)?,
                "Variant" => self.text_once(&mut variant, &child, paths.variant)?,
                "TimetableYear" => self.text_once(&mut year, &child, paths.timetable_year)?,
                _ => self.cursor.skip(&child)?,
            }
        }
        let object_type = match &*self.required(object_type, paths.object_type)? {
            "PA" => ObjectType::Pa,
            "TR" => ObjectType::Tr,
            other => return Err(self.invalid(paths.object_type, other)),
        };
        Ok(Identifier {
            object_type,
            company: self.required(company, paths.company)?,
            core: self.required(core, paths.core)?,
            variant: self.required(variant, paths.variant)?,
            timetable_year: self.required(year, paths.timetable_year)?,
        })
    }

    fn calendar(&mut self, element: &Element<'_>) -> Result<Calendar, CzpttError> {
        const BITMAP: &str = "PlannedCalendar/BitmapDays";
        const START: &str = "PlannedCalendar/ValidityPeriod/StartDateTime";
        const END: &str = "PlannedCalendar/ValidityPeriod/EndDateTime";
        const PERIOD: &str = "PlannedCalendar/ValidityPeriod";
        let (mut bitmap, mut start, mut end, mut period) = (None, None, None, None);
        while let Some(child) = self.cursor.child(element)? {
            match child.name() {
                "BitmapDays" => self.text_once(&mut bitmap, &child, BITMAP)?,
                "ValidityPeriod" => {
                    self.once(&mut period, (), PERIOD)?;
                    while let Some(item) = self.cursor.child(&child)? {
                        match item.name() {
                            "StartDateTime" => {
                                let value = self.timestamp(&item, START)?;
                                self.once(&mut start, value, START)?;
                            }
                            "EndDateTime" => {
                                let value = self.timestamp(&item, END)?;
                                self.once(&mut end, value, END)?;
                            }
                            _ => self.cursor.skip(&item)?,
                        }
                    }
                }
                _ => self.cursor.skip(&child)?,
            }
        }
        Ok(Calendar {
            start: start.ok_or_else(|| self.missing(START))?,
            end,
            bitmap: self.required(bitmap, BITMAP)?,
        })
    }

    /// One `NetworkSpecificParameter`: `Name[i]` with `Value[i]` up to the shorter count (R031).
    fn parameters(
        &mut self,
        element: &Element<'_>,
        out: &mut Vec<Parameter>,
    ) -> Result<(), CzpttError> {
        self.names.clear();
        self.values.clear();
        while let Some(child) = self.cursor.child(element)? {
            match child.name() {
                "Name" => {
                    let text = self.cursor.text(&child)?.unwrap_or("").into();
                    self.names.push(text);
                }
                "Value" => {
                    let text = self.cursor.text(&child)?.unwrap_or("").into();
                    self.values.push(text);
                }
                _ => self.cursor.skip(&child)?,
            }
        }
        out.extend(
            self.names
                .drain(..)
                .zip(self.values.drain(..))
                .map(|(name, value)| Parameter { name, value }),
        );
        Ok(())
    }
}
