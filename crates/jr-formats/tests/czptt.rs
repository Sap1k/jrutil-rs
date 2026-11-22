//! CZPTT messages: real fixtures from the golden subset and small hand-made variants.

use jiff::civil::{date, time};
use jr_formats::archive::gunzip;
use jr_formats::czptt::{self, CzpttError, Message, ObjectType, TimetableMessage};

fn timetable(xml: &[u8]) -> TimetableMessage {
    match czptt::parse(xml).expect("parses") {
        Message::Timetable(message) => message,
        Message::Cancellation(_) => panic!("expected a timetable message"),
    }
}

fn names(parameters: &[czptt::Parameter]) -> Vec<(&str, &str)> {
    parameters.iter().map(|p| (&*p.name, &*p.value)).collect()
}

#[test]
fn timetable_with_subsidiaries_and_day_offset() {
    let message = timetable(include_bytes!("data/timetable_day_offset.xml"));

    let ids: Vec<_> = message
        .identifiers
        .iter()
        .map(|i| {
            (
                i.object_type,
                &*i.company,
                &*i.core,
                &*i.variant,
                &*i.timetable_year,
            )
        })
        .collect();
    assert_eq!(
        ids,
        [
            (ObjectType::Pa, "0054", "--KADR249109", "01", "2026"),
            (ObjectType::Tr, "3825", "--KADR249109", "00", "2026"),
        ]
    );
    assert!(message.related.is_empty());
    assert_eq!(message.creation.local, date(2026, 6, 24).at(7, 29, 8, 0));
    assert_eq!(message.creation.offset_seconds, None);
    assert_eq!(
        message.calendar.start.local,
        date(2026, 6, 26).at(0, 0, 0, 0)
    );
    assert_eq!(
        message.calendar.end.map(|end| end.local),
        Some(date(2026, 6, 26).at(0, 0, 0, 0))
    );
    assert_eq!(&*message.calendar.bitmap, "1");
    assert!(message.parameters.is_empty());

    // CZPTTHeader carries location-like elements; they are not locations.
    assert_eq!(message.locations.len(), 7);
    let first = &message.locations[0];
    assert_eq!((&*first.country, &*first.primary_code), ("CZ", "74372"));
    assert_eq!(first.primary_name.as_deref(), Some("Jindřiš"));
    let subsidiary = first.subsidiary.as_ref().expect("subsidiary");
    assert_eq!(&*subsidiary.code, "1");
    assert_eq!(subsidiary.type_code.as_deref(), Some("1"));
    assert_eq!(subsidiary.allocation_company.as_deref(), Some("0054"));
    assert_eq!(subsidiary.name, None);
    assert_eq!(first.responsible_ru.as_deref(), Some("3825"));
    assert_eq!(first.operational_train_number.as_deref(), Some("91503"));
    assert_eq!(first.activities, ["0001".into()]);
    assert_eq!(
        names(&first.parameters),
        [
            ("CZAlternativeTransport", "0"),
            ("CZPassengerDwellTime", "1")
        ]
    );
    assert_eq!(first.timings.len(), 1);
    assert_eq!(&*first.timings[0].qualifier, "ALD");
    assert_eq!(first.timings[0].time, time(23, 56, 0, 0));
    assert_eq!(first.timings[0].offset_seconds, Some(7200));
    assert_eq!(first.timings[0].day_offset, 0);

    let second = &message.locations[1];
    assert_eq!(second.subsidiary, None);
    assert!(second.activities.is_empty());
    let timings: Vec<_> = second
        .timings
        .iter()
        .map(|t| (&*t.qualifier, t.time, t.day_offset))
        .collect();
    assert_eq!(
        timings,
        [("ALA", time(0, 1, 0, 0), 1), ("ALD", time(0, 1, 0, 0), 1)]
    );

    let last = &message.locations[6];
    assert_eq!(last.subsidiary.as_ref().map(|s| &*s.code), Some("7m"));
}

#[test]
fn timetable_with_related_identifiers_and_root_parameters() {
    let message = timetable(include_bytes!("data/timetable_related.xml"));

    assert_eq!(message.related.len(), 1);
    assert_eq!(message.related[0].object_type, ObjectType::Pa);
    assert_eq!(&*message.related[0].core, "KT----47351A");
    let root: Vec<_> = message.parameters.iter().map(|p| &*p.name).collect();
    assert_eq!(
        root,
        [
            "CZIPTS",
            "CZCentralPTTNote",
            "CZCentralPTTNote",
            "CZReroute",
            "CZOriginalCalendarBitmap",
            "CZOriginalCalendarStartDate",
            "CZOriginalCalendarEndDate",
        ]
    );
    assert_eq!(&*message.parameters[1].value, "35|CZ33754||CZ33594||0|");

    let first = &message.locations[0];
    assert_eq!(first.responsible_im.as_deref(), Some("54"));
    assert_eq!(first.train_type.as_deref(), Some("1"));
    assert_eq!(first.traffic_type.as_deref(), Some("11"));
    assert_eq!(first.commercial_traffic_type.as_deref(), Some("84"));
    // DwellTime beside the timings is not a timing.
    let second = &message.locations[1];
    assert_eq!(second.timings.len(), 2);
    assert_eq!(second.timings[1].time, time(12, 6, 30, 0));
}

#[test]
fn cancellation_message() {
    let xml = gunzip(include_bytes!("data/cancellation.xml.gz")).expect("gzip");
    let Message::Cancellation(message) = czptt::parse(&xml).expect("parses") else {
        panic!("expected a cancellation");
    };
    assert_eq!(message.identifiers.len(), 2);
    assert_eq!(message.identifiers[0].object_type, ObjectType::Pa);
    assert_eq!(&*message.identifiers[0].core, "KT----23746A");
    assert_eq!(
        message.cancellation.local,
        date(2025, 11, 28).at(13, 30, 41, 0)
    );
    assert_eq!(message.calendar.start.local.date(), date(2025, 12, 14));
    assert_eq!(
        message.calendar.end.map(|end| end.local.date()),
        Some(date(2026, 12, 12))
    );
    assert_eq!(message.calendar.bitmap.len(), 364);
}

/// A minimal timetable message around the given locations and root-level extras.
fn message(creation: &str, locations: &str, root: &str) -> String {
    format!(
        "<CZPTTCISMessage><Identifiers><PlannedTransportIdentifiers><ObjectType>PA</ObjectType>\
         <Company>0054</Company><Core>X</Core><Variant>00</Variant>\
         <TimetableYear>2026</TimetableYear></PlannedTransportIdentifiers></Identifiers>\
         {creation}<CZPTTInformation>{locations}<PlannedCalendar><BitmapDays>1</BitmapDays>\
         <ValidityPeriod><StartDateTime>2026-01-01T00:00:00</StartDateTime></ValidityPeriod>\
         </PlannedCalendar></CZPTTInformation>{root}</CZPTTCISMessage>"
    )
}

const CREATION: &str = "<CZPTTCreation>2026-01-01T00:00:00</CZPTTCreation>";

fn location(code: &str, extra: &str) -> String {
    format!(
        "<CZPTTLocation><Location><CountryCodeISO>CZ</CountryCodeISO>{code}</Location>{extra}\
         </CZPTTLocation>"
    )
}

#[test]
fn czptt_r031_parameters_pair_names_with_values_up_to_the_shorter_count() {
    let root = "<NetworkSpecificParameter><Name>a</Name><Name>b</Name><Value>1</Value>\
                </NetworkSpecificParameter>\
                <NetworkSpecificParameter><Name>c</Name><Value>2</Value><Value>3</Value>\
                </NetworkSpecificParameter>\
                <NetworkSpecificParameter><Name>d</Name><Value/></NetworkSpecificParameter>";
    let message = timetable(message(CREATION, "", root).as_bytes());
    assert_eq!(
        names(&message.parameters),
        [("a", "1"), ("c", "2"), ("d", "")]
    );
}

#[test]
fn czptt_r030_activity_codes_stay_text() {
    let extra = "<TrainActivity><TrainActivityType>0001</TrainActivityType></TrainActivity>\
                 <TrainActivity><TrainActivityType> ZZ99 </TrainActivityType></TrainActivity>";
    let locations = location("<LocationPrimaryCode>1</LocationPrimaryCode>", extra);
    let message = timetable(message(CREATION, &locations, "").as_bytes());
    assert_eq!(
        message.locations[0].activities,
        ["0001".into(), "ZZ99".into()]
    );
}

#[test]
fn czptt_r010_dispatch_on_the_root_element() {
    let prefixed = message(CREATION, "", "")
        .replace("<CZPTTCISMessage>", "<c:CZPTTCISMessage xmlns:c=\"urn:x\">")
        .replace("</CZPTTCISMessage>", "</c:CZPTTCISMessage>");
    assert_eq!(timetable(prefixed.as_bytes()).identifiers.len(), 1);
    assert!(matches!(
        czptt::parse(b"<?xml version=\"1.0\"?><Other/>"),
        Err(CzpttError::UnknownRoot(name)) if &*name == "Other"
    ));
}

#[test]
fn missing_and_unreadable_mandatory_elements_are_named() {
    let error = czptt::parse(message("", "", "").as_bytes()).unwrap_err();
    assert!(matches!(
        error,
        CzpttError::Missing {
            element: "CZPTTCreation",
            location: None
        }
    ));
    assert_eq!(error.to_string(), "CZPTTCreation is missing");

    let locations = location("<LocationPrimaryCode>1</LocationPrimaryCode>", "")
        + &location("<PrimaryLocationName>n</PrimaryLocationName>", "");
    let error = czptt::parse(message(CREATION, &locations, "").as_bytes()).unwrap_err();
    assert_eq!(
        error.to_string(),
        "Location/LocationPrimaryCode is missing (CZPTTLocation 1)"
    );

    let timing = "<TimingAtLocation><Timing TimingQualifierCode=\"ALA\"><Time>24:01:00</Time>\
                  <Offset>0</Offset></Timing></TimingAtLocation>";
    let locations = location("<LocationPrimaryCode>1</LocationPrimaryCode>", timing);
    let error = czptt::parse(message(CREATION, &locations, "").as_bytes()).unwrap_err();
    assert!(matches!(
        error,
        CzpttError::Invalid { element: "Timing/Time", location: Some(0), ref value } if &**value == "24:01:00"
    ));

    let bad_type = message(CREATION, "", "").replace(">PA<", ">XX<");
    assert!(matches!(
        czptt::parse(bad_type.as_bytes()),
        Err(CzpttError::Invalid {
            element: "PlannedTransportIdentifiers/ObjectType",
            ..
        })
    ));

    let twice = message(&(CREATION.to_owned() + CREATION), "", "");
    assert!(matches!(
        czptt::parse(twice.as_bytes()),
        Err(CzpttError::Repeated {
            element: "CZPTTCreation",
            ..
        })
    ));
}
