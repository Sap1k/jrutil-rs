//! Containers, the snapshot manifest, the KADR catalogue and SR70.

use std::io::Cursor;

use jiff::civil::date;
use jr_formats::archive::{gunzip, xml_members};
use jr_formats::kadr::{Catalog, Problem, Rejected};
use jr_formats::snapshot::{Manifest, ObjectKind};
use jr_formats::sr70::{self, Sr70Error};

#[test]
fn xml_members_are_filtered_and_sorted_by_name() {
    let members = xml_members(Cursor::new(include_bytes!("data/members.zip"))).expect("zip");
    let listed: Vec<_> = members
        .iter()
        .map(|(name, bytes)| (name.as_str(), bytes.as_slice()))
        .collect();
    assert_eq!(
        listed,
        [
            ("A.XML", b"<a/>".as_slice()),
            ("b.xml", b"<b/>".as_slice()),
            ("sub/a.xml", b"<s/>".as_slice()),
        ]
    );
}

#[test]
fn gunzip_decodes_a_change_object() {
    let xml = gunzip(include_bytes!("data/cancellation.xml.gz")).expect("gzip");
    assert!(xml.starts_with(b"<?xml version=\"1.0\"?>"));
    assert!(gunzip(b"<?xml").is_err());
}

#[test]
fn manifest_lists_objects_and_auxiliary_files() {
    let manifest = Manifest::parse(
        r#"{"schema_version": 1, "timetable_year": 2026, "source_base_url": "https://x",
            "objects": [
              {"relative_path": "annual/JR2026.zip", "url": null, "bytes": 10, "sha256": "aa", "kind": "annual_zip"},
              {"relative_path": "changes/2025-12/PA_1.xml.zip", "url": null, "bytes": 2, "sha256": "bb", "kind": "monthly_gzip"}
            ],
            "auxiliary": [{"relative_path": "sr70/SR70.csv", "bytes": 3, "sha256": "cc"}]}"#,
    )
    .expect("manifest");
    assert_eq!(manifest.timetable_year, 2026);
    let kinds: Vec<_> = manifest.objects.iter().map(|o| o.kind).collect();
    assert_eq!(kinds, [ObjectKind::AnnualZip, ObjectKind::MonthlyGzip]);
    assert_eq!(manifest.auxiliary[0].relative_path, "sr70/SR70.csv");
    assert!(Manifest::parse(r#"{"timetable_year": 2026, "objects": [{"relative_path": "x", "bytes": 1, "sha256": "", "kind": "tarball"}], "auxiliary": []}"#).is_err());
}

#[test]
fn catalog_reports_what_the_file_holds() {
    let catalog = Catalog::parse(include_str!("data/catalog.json")).expect("catalog");
    assert_eq!(catalog.lines.len(), 2);
    let line = &catalog.lines[1];
    assert_eq!(
        (
            &*line.code,
            line.mark.as_deref(),
            line.abbreviation.as_deref()
        ),
        ("10", Some("R10"), Some("R10"))
    );
    assert_eq!(line.valid_to, Some(date(2026, 12, 12)));
    assert_eq!(catalog.companies[0].url, None);
    assert_eq!(
        catalog.ids[0].name.as_deref(),
        Some("Pražská integrovaná doprava")
    );
    assert_eq!(catalog.central_notes[1].name.as_deref(), Some("jednička"));
    assert_eq!(catalog.central_notes[1].text, None);
    assert_eq!(catalog.central_notes[1].valid_from, Some(date(2019, 1, 1)));
    assert_eq!(catalog.train_types[0].abbreviation.as_deref(), Some("Os"));
    assert_eq!(catalog.commercial_train_types.len(), 2);
    assert!(catalog.rejected.is_empty());
}

#[test]
fn czptt_r017_catalog_reports_entries_without_code_and_bad_dates() {
    let catalog = Catalog::parse(
        r#"{"lines": [
              {"name": "no code"},
              {"code": "1", "valid_from": "2020-09-01", "valid_to": "2100-13-01"},
              {"code": "2", "valid_from": ""},
              {"code": "3"}
            ],
            "companies": [{"code": null, "name": "x"}]}"#,
    )
    .expect("catalog");
    assert_eq!(catalog.lines.len(), 1);
    let line = &catalog.lines[0];
    assert_eq!((&*line.code, &line.name, &line.mark), ("3", &None, &None));
    assert_eq!((line.valid_from, line.valid_to), (None, None));
    assert!(catalog.central_notes.is_empty());
    let bad_date = |field, value: &str| Problem::BadDate {
        field,
        value: value.into(),
    };
    assert_eq!(
        catalog.rejected,
        [
            Rejected {
                array: "lines",
                index: 0,
                problem: Problem::MissingCode
            },
            Rejected {
                array: "lines",
                index: 1,
                problem: bad_date("valid_to", "2100-13-01")
            },
            Rejected {
                array: "lines",
                index: 2,
                problem: bad_date("valid_from", "")
            },
            Rejected {
                array: "companies",
                index: 0,
                problem: Problem::MissingCode
            },
        ]
    );
}

#[test]
fn czptt_r018_sr70_rows_as_written() {
    let rows = sr70::parse(include_bytes!("data/sr70.csv")).expect("SR70");
    assert_eq!(rows.len(), 12);
    let first = &rows[0];
    assert_eq!(&*first.code, "003012");
    assert_eq!(first.name.as_deref(), Some("Milotice nad Opavou-AWT"));
    assert_eq!(
        (first.latitude, first.longitude),
        (Some(50.014327), Some(17.557445))
    );
    assert_eq!(first.line, 1);
    // Placeholders are facts; deciding they mean "no name" or "no position" is a rule.
    let last = &rows[11];
    assert_eq!((&*last.code, last.name.as_deref()), ("541516", Some("-")));
    assert_eq!((last.latitude, last.longitude), (Some(0.0), Some(0.0)));

    let rows =
        sr70::parse("\u{feff}533398,Praha,abc,14.4\n533399,,50.1\n".as_bytes()).expect("SR70");
    assert_eq!(&*rows[0].code, "533398");
    assert_eq!((rows[0].latitude, rows[0].longitude), (None, Some(14.4)));
    assert_eq!(
        (rows[1].name.as_deref(), rows[1].latitude, rows[1].longitude),
        (None, Some(50.1), None)
    );
    assert_eq!(rows[1].line, 2);

    assert!(matches!(
        sr70::parse(b"533398,Praha,50,14\n533399\n"),
        Err(Sr70Error::TooFewColumns {
            line: 2,
            columns: 1
        })
    ));
}
