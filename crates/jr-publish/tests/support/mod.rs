//! A tiny production package built in a test: rows per relation -> `Table` -> Parquet, the
//! GTFS projection of the relations, `diagnostics.json` and a `manifest.json` inventorying it
//! all. Relation files are written by the package writer's `write_relation`.

use jiff::civil::{Date, date};
use jr_publish::{Column, CsvTable, FieldType, RelationSpec, StrColumn, Table, contract};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq)]
pub enum Cell {
    Str(String),
    Int(i64),
    Double(f64),
    Bool(bool),
    Date(Date),
}

pub fn s(value: &str) -> Cell {
    Cell::Str(value.to_owned())
}

pub fn i(value: i64) -> Cell {
    Cell::Int(value)
}

pub fn b(value: bool) -> Cell {
    Cell::Bool(value)
}

pub fn d(value: Date) -> Cell {
    Cell::Date(value)
}

/// One row; fields not named are null.
pub type Row = BTreeMap<String, Cell>;

pub fn row(cells: &[(&str, Cell)]) -> Row {
    cells
        .iter()
        .map(|(name, cell)| ((*name).to_owned(), cell.clone()))
        .collect()
}

/// Rows of every contract relation.
#[derive(Debug, Clone)]
pub struct Fixture {
    pub relations: BTreeMap<String, Vec<Row>>,
}

impl Fixture {
    /// A valid road package: one trip with two calls, its route, calendar and stops.
    pub fn valid() -> Fixture {
        let mut relations: BTreeMap<String, Vec<Row>> = contract()
            .relations
            .iter()
            .map(|relation| (relation.name.clone(), Vec::new()))
            .collect();
        let mut add = |name: &str, rows: Vec<Row>| {
            relations.insert(name.to_owned(), rows);
        };
        add(
            "agency",
            vec![row(&[
                ("agency_id", s("jdf:agency:1")),
                ("name", s("Dopravce")),
                ("timezone", s("Europe/Prague")),
            ])],
        );
        let stop = |id: &str, kind: &str, parent: Option<&str>| {
            let mut cells = row(&[
                ("location_id", s(id)),
                ("kind", s(kind)),
                ("domain", s("surface")),
                ("name", s("Horní Dolní")),
                ("coordinate_precision", s("exact")),
                ("coordinate_source", s("catalogue")),
                ("longitude", Cell::Double(14.42)),
                ("latitude", Cell::Double(50.08)),
            ]);
            if let Some(parent) = parent {
                cells.insert("parent_location_id".into(), s(parent));
            }
            cells
        };
        add(
            "location",
            vec![
                stop("jdf:stop:1", "stop_place", None),
                stop("jdf:stop:1:1", "boarding_point", Some("jdf:stop:1")),
                stop("jdf:stop:2", "stop_place", None),
            ],
        );
        add(
            "route",
            vec![row(&[
                ("route_id", s("jdf:route:1")),
                ("agency_id", s("jdf:agency:1")),
                ("mode", s("bus")),
                ("gtfs_route_type", i(3)),
                ("short_name", s("100")),
                ("timetable_kind", s("regular")),
            ])],
        );
        add(
            "service_calendar",
            vec![row(&[
                ("service_id", s("jdf:service:1")),
                ("valid_from", d(date(2026, 1, 5))),
                ("valid_to", d(date(2026, 6, 26))),
                ("weekday_mask", i(31)),
            ])],
        );
        add(
            "service_exception",
            vec![row(&[
                ("service_id", s("jdf:service:1")),
                ("service_date", d(date(2026, 4, 6))),
                ("added", b(false)),
            ])],
        );
        add(
            "trip",
            vec![row(&[
                ("trip_id", s("jdf:trip:1")),
                ("route_id", s("jdf:route:1")),
                ("service_id", s("jdf:service:1")),
                ("direction", i(0)),
            ])],
        );
        let route_stop = |id: &str, sequence: i64, location: &str| {
            row(&[
                ("route_stop_id", s(id)),
                ("route_id", s("jdf:route:1")),
                ("direction", i(0)),
                ("sequence", i(sequence)),
                ("location_id", s(location)),
            ])
        };
        add(
            "route_stop",
            vec![
                route_stop("jdf:rs:1", 1, "jdf:stop:1"),
                route_stop("jdf:rs:2", 2, "jdf:stop:2"),
            ],
        );
        let call = |sequence: i64, location: &str, boarding: Option<&str>, time: i64| {
            let mut cells = row(&[
                ("trip_id", s("jdf:trip:1")),
                ("sequence", i(sequence)),
                ("location_id", s(location)),
                ("passenger_service", b(true)),
                ("route_stop_id", s(&format!("jdf:rs:{sequence}"))),
                ("scheduled_arrival", i(time)),
                ("scheduled_departure", i(time)),
                ("pickup_type", i(0)),
                ("dropoff_type", i(0)),
                ("timepoint", b(true)),
                ("source_kilometre", Cell::Double(sequence as f64 - 1.0)),
            ]);
            if let Some(boarding) = boarding {
                cells.insert("boarding_point_id".into(), s(boarding));
            }
            cells
        };
        add(
            "trip_call",
            vec![
                call(1, "jdf:stop:1", Some("jdf:stop:1:1"), 25_200),
                call(2, "jdf:stop:2", None, 90_000),
            ],
        );
        add(
            "source_key",
            vec![row(&[
                ("entity_kind", s("trip")),
                ("namespace", s("cis:line_trip")),
                ("identifier", s("100001:1")),
                ("public_id", s("jdf:trip:1")),
                ("valid_from", d(date(2026, 1, 5))),
                ("valid_to", d(date(2026, 6, 26))),
                ("binding_method", s("identity")),
            ])],
        );
        Fixture { relations }
    }

    pub fn rows_mut(&mut self, relation: &str) -> &mut Vec<Row> {
        self.relations.get_mut(relation).expect("contract relation")
    }

    pub fn tables(&self) -> Vec<Table> {
        contract()
            .relations
            .iter()
            .map(|spec| table(spec, &self.relations[&spec.name]))
            .collect()
    }

    /// The projected GTFS members the validator compares, by file name.
    pub fn gtfs(&self) -> BTreeMap<String, CsvTable> {
        let rows = |name: &str| &self.relations[name];
        let text = |row: &Row, field: &str| match row.get(field) {
            None => String::new(),
            Some(Cell::Str(value)) => value.clone(),
            Some(Cell::Int(value)) => value.to_string(),
            Some(Cell::Date(value)) => value.strftime("%Y%m%d").to_string(),
            Some(other) => panic!("{field}: no GTFS text for {other:?}"),
        };
        let time = |row: &Row, field: &str| match row.get(field) {
            Some(Cell::Int(seconds)) => {
                format!(
                    "{:02}:{:02}:{:02}",
                    seconds / 3600,
                    seconds / 60 % 60,
                    seconds % 60
                )
            }
            _ => String::new(),
        };
        let table = |header: &[&str], rows: Vec<Vec<String>>| CsvTable {
            header: header.iter().map(|h| (*h).to_owned()).collect(),
            rows,
        };
        let mut gtfs = BTreeMap::new();
        gtfs.insert(
            "calendar.txt".to_owned(),
            table(
                &[
                    "service_id",
                    "monday",
                    "tuesday",
                    "wednesday",
                    "thursday",
                    "friday",
                    "saturday",
                    "sunday",
                    "start_date",
                    "end_date",
                ],
                rows("service_calendar")
                    .iter()
                    .map(|row| {
                        let Some(Cell::Int(mask)) = row.get("weekday_mask") else {
                            panic!("mask")
                        };
                        let mut cells = vec![text(row, "service_id")];
                        cells.extend((0..7).map(|day| ((mask >> day) & 1).to_string()));
                        cells.extend([text(row, "valid_from"), text(row, "valid_to")]);
                        cells
                    })
                    .collect(),
            ),
        );
        gtfs.insert(
            "calendar_dates.txt".to_owned(),
            table(
                &["service_id", "date", "exception_type"],
                rows("service_exception")
                    .iter()
                    .map(|row| {
                        let added = row.get("added") == Some(&b(true));
                        vec![
                            text(row, "service_id"),
                            text(row, "service_date"),
                            if added { "1" } else { "2" }.into(),
                        ]
                    })
                    .collect(),
            ),
        );
        gtfs.insert(
            "stop_times.txt".to_owned(),
            table(
                &[
                    "trip_id",
                    "arrival_time",
                    "departure_time",
                    "stop_id",
                    "stop_sequence",
                    "pickup_type",
                    "drop_off_type",
                ],
                rows("trip_call")
                    .iter()
                    .filter(|row| row.get("passenger_service") == Some(&b(true)))
                    .map(|row| {
                        let stop = if row.contains_key("boarding_point_id") {
                            "boarding_point_id"
                        } else {
                            "location_id"
                        };
                        vec![
                            text(row, "trip_id"),
                            time(row, "scheduled_arrival"),
                            time(row, "scheduled_departure"),
                            text(row, stop),
                            text(row, "sequence"),
                            text(row, "pickup_type"),
                            text(row, "dropoff_type"),
                        ]
                    })
                    .collect(),
            ),
        );
        gtfs.insert(
            "trips.txt".to_owned(),
            table(
                &["route_id", "service_id", "trip_id"],
                rows("trip")
                    .iter()
                    .map(|row| {
                        vec![
                            text(row, "route_id"),
                            text(row, "service_id"),
                            text(row, "trip_id"),
                        ]
                    })
                    .collect(),
            ),
        );
        gtfs
    }

    /// Writes the package with the projected GTFS.
    pub fn write(&self, name: &str) -> PathBuf {
        self.write_with_gtfs(name, &self.gtfs())
    }

    pub fn write_with_gtfs(&self, name: &str, gtfs: &BTreeMap<String, CsvTable>) -> PathBuf {
        let root = Path::new(env!("CARGO_TARGET_TMPDIR"))
            .join("jr-publish")
            .join(name);
        if root.exists() {
            fs::remove_dir_all(&root).expect("clear fixture");
        }
        fs::create_dir_all(root.join("serving")).expect("fixture dir");
        let tables = self.tables();
        for (spec, table) in contract().relations.iter().zip(&tables) {
            write_parquet(
                &root.join("serving").join(format!("{}.parquet", spec.name)),
                spec,
                table,
            );
        }
        write_zip(&root.join("gtfs.zip"), gtfs);
        fs::write(
            root.join("diagnostics.json"),
            "{\"schema_version\":1,\"counts_by_code\":[]}\n",
        )
        .expect("diagnostics");
        let manifest = manifest(&root, &tables);
        write_json(&root.join("manifest.json"), &manifest);
        root
    }
}

/// Rewrites `manifest.json` (it is not inventoried, so no digest changes).
pub fn edit_manifest(root: &Path, edit: impl FnOnce(&mut Value)) {
    let path = root.join("manifest.json");
    let mut manifest: Value =
        serde_json::from_slice(&fs::read(&path).expect("read")).expect("json");
    edit(&mut manifest);
    write_json(&path, &manifest);
}

fn write_json(path: &Path, value: &Value) {
    let mut text = serde_json::to_string_pretty(value).expect("json");
    text.push('\n');
    fs::write(path, text).expect("write json");
}

/// A relation's rows as a `Table` of the contract's types.
pub fn table(spec: &RelationSpec, rows: &[Row]) -> Table {
    let columns = spec
        .fields
        .iter()
        .map(|field| {
            let cells = rows.iter().map(|row| row.get(&field.name));
            let column = match field.ty {
                FieldType::String => Column::Str(
                    cells
                        .map(|cell| match cell {
                            None => None,
                            Some(Cell::Str(value)) => Some(value.as_str()),
                            Some(other) => panic!("{other:?} is not a string"),
                        })
                        .collect::<StrColumn>(),
                ),
                FieldType::Int16 => Column::Int16(
                    cells
                        .map(|cell| cell.map(|c| int(c).try_into().expect("int16")))
                        .collect(),
                ),
                FieldType::Int32 => Column::Int32(
                    cells
                        .map(|cell| cell.map(|c| int(c).try_into().expect("int32")))
                        .collect(),
                ),
                FieldType::Double => Column::Double(cells.map(|cell| cell.map(double)).collect()),
                FieldType::Bool => Column::Bool(cells.map(|cell| cell.map(boolean)).collect()),
                FieldType::Date32 => Column::Date(cells.map(|cell| cell.map(civil)).collect()),
            };
            (field.name.clone(), column)
        })
        .collect();
    Table {
        name: spec.name.clone(),
        columns,
    }
}

fn int(cell: &Cell) -> i64 {
    match cell {
        Cell::Int(value) => *value,
        other => panic!("{other:?} is not an integer"),
    }
}

fn double(cell: &Cell) -> f64 {
    match cell {
        Cell::Double(value) => *value,
        other => panic!("{other:?} is not a double"),
    }
}

fn boolean(cell: &Cell) -> bool {
    match cell {
        Cell::Bool(value) => *value,
        other => panic!("{other:?} is not a bool"),
    }
}

fn civil(cell: &Cell) -> Date {
    match cell {
        Cell::Date(value) => *value,
        other => panic!("{other:?} is not a date"),
    }
}

/// One relation file with the contract's schema and footer metadata, as the writer writes it.
pub fn write_parquet(path: &Path, spec: &RelationSpec, table: &Table) {
    assert_eq!(spec.name, table.name);
    jr_publish::write_relation(path, table).expect("write relation");
}

fn write_zip(path: &Path, members: &BTreeMap<String, CsvTable>) {
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Stored)
        .last_modified_time(zip::DateTime::default());
    let mut archive = zip::ZipWriter::new(File::create(path).expect("create zip"));
    for (name, table) in members {
        let mut csv = csv::WriterBuilder::new()
            .terminator(csv::Terminator::Any(b'\n'))
            .from_writer(Vec::new());
        csv.write_record(&table.header).expect("header");
        for row in &table.rows {
            csv.write_record(row).expect("row");
        }
        archive.start_file(name.as_str(), options).expect("member");
        archive
            .write_all(&csv.into_inner().expect("csv"))
            .expect("write member");
    }
    archive.finish().expect("finish zip");
}

fn sha256(path: &Path) -> String {
    let digest = Sha256::digest(fs::read(path).expect("read payload"));
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// The manifest of a written package: versions, sources, namespaces, relations, inventory.
fn manifest(root: &Path, tables: &[Table]) -> Value {
    let contract = contract();
    let relations: Vec<Value> = contract
        .relations
        .iter()
        .zip(tables)
        .map(|(spec, table)| {
            let schema: Vec<Value> = spec
                .fields
                .iter()
                .map(|field| {
                    let mut value = json!({ "name": field.name, "type": field.ty.as_str(), "nullable": field.nullable });
                    if let Some(enumeration) = &field.enumeration {
                        value["enum"] = json!(enumeration);
                    }
                    value
                })
                .collect();
            let foreign_keys: Vec<Value> = spec
                .foreign_keys
                .iter()
                .map(|key| json!({ "fields": key.fields, "relation": key.relation, "target_fields": key.target_fields }))
                .collect();
            json!({
                "name": spec.name,
                "path": format!("serving/{}.parquet", spec.name),
                "schema": schema,
                "primary_key": spec.primary_key,
                "foreign_keys": foreign_keys,
                "row_count": table.rows(),
            })
        })
        .collect();
    let mut payloads = vec!["diagnostics.json".to_owned(), "gtfs.zip".to_owned()];
    payloads.extend(
        contract
            .relations
            .iter()
            .map(|spec| format!("serving/{}.parquet", spec.name)),
    );
    payloads.sort();
    let files: Vec<Value> = payloads
        .iter()
        .map(|path| {
            let file = root.join(path);
            let size = fs::metadata(&file).expect("payload").len();
            json!({ "path": path, "size_bytes": size, "sha256": sha256(&file) })
        })
        .collect();
    let namespaces: Vec<Value> = contract
        .namespaces
        .iter()
        .map(|(name, spec)| json!({ "name": name, "entity_kind": spec.entity_kind }))
        .collect();
    json!({
        "bundle_format": "jrutil-production",
        "bundle_version": 3,
        "serving_schema_version": contract.version(),
        "diagnostics_schema_version": 1,
        "identity_contract": "jrutil-identity-v1",
        "feed_version": "sha256:0000000000000000000000000000000000000000000000000000000000000001",
        "build_spec_sha256": "0000000000000000000000000000000000000000000000000000000000000002",
        "compiler": { "tool": "jrutil-rs", "version": "test" },
        "contract_valid": true,
        "publication_eligible": true,
        "calibration": false,
        "sources": [{
            "source_id": "fixture",
            "payload_kind": "file",
            "payload_sha256": "1111111111111111111111111111111111111111111111111111111111111111",
        }],
        "namespaces": namespaces,
        "relations": relations,
        "files": files,
    })
}
