//! The validator on a tiny package written here: clean when valid, and one stable code per
//! kind of break.

mod support;

use jr_publish::validate::code;
use jr_publish::{Package, validate};
use support::{Fixture, edit_manifest, s};

fn codes(root: &std::path::Path) -> Vec<&'static str> {
    let package = Package::open(root).expect("open");
    let findings = validate(&package).expect("readable");
    let mut codes: Vec<_> = findings.iter().map(|finding| finding.code).collect();
    codes.dedup();
    codes
}

#[test]
fn a_valid_package_has_no_findings_and_reads_back() {
    let fixture = Fixture::valid();
    let root = fixture.write("valid");
    let package = Package::open(&root).expect("open");
    assert_eq!(validate(&package).expect("readable"), []);

    let calls = package.relation("trip_call").expect("trip_call");
    assert_eq!(
        calls,
        support::table(
            jr_publish::contract().relation("trip_call").unwrap(),
            &fixture.relations["trip_call"]
        )
    );
    assert_eq!(
        package.gtfs_files().expect("zip"),
        [
            "calendar.txt",
            "calendar_dates.txt",
            "stop_times.txt",
            "trips.txt"
        ]
    );
    assert_eq!(
        package.gtfs("stop_times.txt").expect("member").rows[1][1],
        "25:00:00"
    );
}

#[test]
fn a_duplicate_primary_key_is_found() {
    let mut fixture = Fixture::valid();
    let first = fixture.rows_mut("trip_call")[0].clone();
    fixture.rows_mut("trip_call").push(first);
    assert_eq!(
        codes(&fixture.write("duplicate-key")),
        [code::DUPLICATE_PRIMARY_KEY]
    );
}

#[test]
fn a_dangling_foreign_key_is_found() {
    let mut fixture = Fixture::valid();
    fixture.rows_mut("trip")[0].insert("route_id".into(), s("jdf:route:missing"));
    assert_eq!(
        codes(&fixture.write("dangling-key")),
        [code::FOREIGN_KEY_DANGLING]
    );
}

#[test]
fn a_value_outside_its_enumeration_is_found() {
    let mut fixture = Fixture::valid();
    fixture.rows_mut("location")[2].insert("coordinate_precision".into(), s("guessed"));
    assert_eq!(
        codes(&fixture.write("bad-enum")),
        [code::ENUM_VALUE_INVALID]
    );
}

#[test]
fn a_wrong_digest_is_found() {
    let root = Fixture::valid().write("wrong-hash");
    edit_manifest(&root, |manifest| {
        manifest["files"][0]["sha256"] = "0".repeat(64).into();
    });
    assert_eq!(codes(&root), [code::FILE_HASH_MISMATCH]);
}

#[test]
fn a_missing_relation_is_found() {
    let root = Fixture::valid().write("missing-relation");
    std::fs::remove_file(root.join("serving/shape.parquet")).expect("remove");
    assert_eq!(codes(&root), [code::MISSING_FILE]);
}

#[test]
fn gtfs_that_is_not_the_projection_is_found() {
    let fixture = Fixture::valid();
    let mut gtfs = fixture.gtfs();
    gtfs.get_mut("stop_times.txt").expect("stop_times").rows[1][2] = "25:01:00".into();
    assert_eq!(
        codes(&fixture.write_with_gtfs("gtfs-projection", &gtfs)),
        [code::GTFS_PROJECTION_MISMATCH]
    );
}
