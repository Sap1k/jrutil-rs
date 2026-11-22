//! `CZPTTLocation`.

use super::Parser;
use crate::czptt::cursor::Element;
use crate::czptt::{CzpttError, Location, Subsidiary, Timing, datetime};

const COUNTRY: &str = "Location/CountryCodeISO";
const PRIMARY_CODE: &str = "Location/LocationPrimaryCode";
const PRIMARY_NAME: &str = "Location/PrimaryLocationName";
const SUBSIDIARY: &str = "Location/LocationSubsidiaryIdentification";
const SUBSIDIARY_CODE: &str = "LocationSubsidiaryIdentification/LocationSubsidiaryCode";
const SUBSIDIARY_NAME: &str = "LocationSubsidiaryIdentification/LocationSubsidiaryName";
const ALLOCATION: &str = "LocationSubsidiaryIdentification/AllocationCompany";
const QUALIFIER: &str = "Timing/@TimingQualifierCode";
const TIME: &str = "Timing/Time";
const OFFSET: &str = "Timing/Offset";

impl Parser<'_> {
    pub(super) fn location(&mut self, element: &Element<'_>) -> Result<Location, CzpttError> {
        let mut place = None;
        let mut timings = Vec::new();
        let (mut ru, mut im, mut train_type, mut traffic, mut commercial, mut number) =
            (None, None, None, None, None, None);
        let mut activities = Vec::new();
        let mut parameters = Vec::new();
        while let Some(child) = self.cursor.child(element)? {
            match child.name() {
                "Location" => {
                    let value = self.place(&child)?;
                    self.once(&mut place, value, "CZPTTLocation/Location")?;
                }
                "TimingAtLocation" => {
                    while let Some(item) = self.cursor.child(&child)? {
                        match item.name() {
                            "Timing" => timings.push(self.timing(&item)?),
                            _ => self.cursor.skip(&item)?,
                        }
                    }
                }
                "ResponsibleRU" => self.text_once(&mut ru, &child, "ResponsibleRU")?,
                "ResponsibleIM" => self.text_once(&mut im, &child, "ResponsibleIM")?,
                "TrainType" => self.text_once(&mut train_type, &child, "TrainType")?,
                "TrafficType" => self.text_once(&mut traffic, &child, "TrafficType")?,
                "CommercialTrafficType" => {
                    self.text_once(&mut commercial, &child, "CommercialTrafficType")?;
                }
                "OperationalTrainNumber" => {
                    self.text_once(&mut number, &child, "OperationalTrainNumber")?;
                }
                "TrainActivity" => {
                    while let Some(item) = self.cursor.child(&child)? {
                        match item.name() {
                            "TrainActivityType" => activities.extend(self.text(&item)?),
                            _ => self.cursor.skip(&item)?,
                        }
                    }
                }
                "NetworkSpecificParameter" => self.parameters(&child, &mut parameters)?,
                _ => self.cursor.skip(&child)?,
            }
        }
        let place = place.ok_or_else(|| self.missing(COUNTRY))?;
        Ok(Location {
            country: self.required(place.country, COUNTRY)?,
            primary_code: self.required(place.primary_code, PRIMARY_CODE)?,
            primary_name: place.primary_name.flatten(),
            subsidiary: place.subsidiary,
            timings,
            responsible_ru: ru.flatten(),
            responsible_im: im.flatten(),
            train_type: train_type.flatten(),
            traffic_type: traffic.flatten(),
            commercial_traffic_type: commercial.flatten(),
            operational_train_number: number.flatten(),
            activities,
            parameters,
        })
    }

    fn place(&mut self, element: &Element<'_>) -> Result<Place, CzpttError> {
        let mut place = Place::default();
        while let Some(child) = self.cursor.child(element)? {
            match child.name() {
                "CountryCodeISO" => self.text_once(&mut place.country, &child, COUNTRY)?,
                "LocationPrimaryCode" => {
                    self.text_once(&mut place.primary_code, &child, PRIMARY_CODE)?;
                }
                "PrimaryLocationName" => {
                    self.text_once(&mut place.primary_name, &child, PRIMARY_NAME)?;
                }
                "LocationSubsidiaryIdentification" => {
                    let value = self.subsidiary(&child)?;
                    if place.subsidiary.is_some() {
                        return Err(CzpttError::Repeated {
                            element: SUBSIDIARY,
                            location: self.location,
                        });
                    }
                    place.subsidiary = Some(value);
                }
                _ => self.cursor.skip(&child)?,
            }
        }
        Ok(place)
    }

    fn subsidiary(&mut self, element: &Element<'_>) -> Result<Subsidiary, CzpttError> {
        let (mut code, mut type_code, mut name, mut allocation) = (None, None, None, None);
        while let Some(child) = self.cursor.child(element)? {
            match child.name() {
                "LocationSubsidiaryCode" => {
                    let value = child.attribute("LocationSubsidiaryTypeCode")?;
                    let value = value.as_deref().map(str::trim).filter(|v| !v.is_empty());
                    self.once(&mut type_code, value.map(Box::from), SUBSIDIARY_CODE)?;
                    self.text_once(&mut code, &child, SUBSIDIARY_CODE)?;
                }
                "LocationSubsidiaryName" => self.text_once(&mut name, &child, SUBSIDIARY_NAME)?,
                "AllocationCompany" => self.text_once(&mut allocation, &child, ALLOCATION)?,
                _ => self.cursor.skip(&child)?,
            }
        }
        Ok(Subsidiary {
            code: self.required(code, SUBSIDIARY_CODE)?,
            type_code: type_code.flatten(),
            name: name.flatten(),
            allocation_company: allocation.flatten(),
        })
    }

    fn timing(&mut self, element: &Element<'_>) -> Result<Timing, CzpttError> {
        let qualifier = element.attribute("TimingQualifierCode")?;
        let qualifier = qualifier
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(Box::from)
            .ok_or_else(|| self.missing(QUALIFIER))?;
        let (mut time, mut day_offset) = (None, None);
        while let Some(child) = self.cursor.child(element)? {
            match child.name() {
                "Time" => {
                    let text = self.cursor.text(&child)?.ok_or(CzpttError::Missing {
                        element: TIME,
                        location: self.location,
                    })?;
                    let value = datetime::time(text).ok_or_else(|| CzpttError::Invalid {
                        element: TIME,
                        location: self.location,
                        value: text.into(),
                    })?;
                    self.once(&mut time, value, TIME)?;
                }
                "Offset" => {
                    let text = self.cursor.text(&child)?.ok_or(CzpttError::Missing {
                        element: OFFSET,
                        location: self.location,
                    })?;
                    let value = text.parse::<i32>().map_err(|_| CzpttError::Invalid {
                        element: OFFSET,
                        location: self.location,
                        value: text.into(),
                    })?;
                    self.once(&mut day_offset, value, OFFSET)?;
                }
                _ => self.cursor.skip(&child)?,
            }
        }
        let (time, offset_seconds) = time.ok_or_else(|| self.missing(TIME))?;
        Ok(Timing {
            qualifier,
            time,
            offset_seconds,
            day_offset: day_offset.ok_or_else(|| self.missing(OFFSET))?,
        })
    }
}

/// `Location`, before its mandatory parts are checked.
#[derive(Default)]
struct Place {
    country: Option<Option<Box<str>>>,
    primary_code: Option<Option<Box<str>>>,
    primary_name: Option<Option<Box<str>>>,
    subsidiary: Option<Subsidiary>,
}
