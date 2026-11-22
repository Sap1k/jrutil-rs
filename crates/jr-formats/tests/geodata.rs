//! Curated geodata readers: fixtures cut from `jrunify-ext-geodata` and `jrutil/data`, and tiny
//! synthetic gazetteers built here.

use std::io::Write;

use flate2::Compression;
use flate2::write::GzEncoder;
use jr_formats::geodata::gazetteer::{self, GazetteerError, Laea};
use jr_formats::geodata::registry::{self, PostKey, RegistryError, Status};
use jr_formats::geodata::route_rules::{self, LicencePattern, PublicLines, RouteRulesError};
use jr_formats::geodata::{catalogue, synonyms};
use jr_model::Coord;

#[test]
fn jdf_r007_catalogue_rows_keep_comma_names_and_report_unparsed_coordinates() {
    let rows = catalogue::parse(include_bytes!("data/geodata/catalogue.csv")).unwrap();
    assert_eq!(rows.len(), 12);

    let americka = &rows[0];
    assert_eq!(&*americka.name, "Liberec,,Americká");
    assert_eq!(americka.latitude, Some(50.75791549682617));
    assert_eq!(americka.longitude, Some(15.033920288085938));
    assert_eq!(americka.okres.as_deref(), Some("LI"));
    assert_eq!(americka.country.as_deref(), Some("CZ"));
    assert_eq!(americka.line, 1);

    assert_eq!(&*rows[5].name, "Hrabětice");
    assert_eq!(rows[8].okres, None);
    assert_eq!(rows[8].country.as_deref(), Some("D"));

    // Out of the Czech Republic (B19) but a number: kept; the range is the compiler's (R006).
    let estonia = &rows[10];
    assert_eq!(estonia.latitude, Some(59.7560959));
    assert_eq!(
        (estonia.okres.as_deref(), estonia.country.as_deref()),
        (None, None)
    );

    let unparsed = &rows[11];
    assert_eq!(&*unparsed.name, "Hrádek n.N.,,nádr.");
    assert_eq!(unparsed.latitude, None);
    assert_eq!(unparsed.longitude, Some(14.8467));
    assert_eq!(unparsed.line, 12);
}

#[test]
fn jdf_r006_catalogue_row_with_six_columns_is_an_error() {
    let six = b"Adamov,49.3,16.6,BK,CZ\nAdamov,49.3,16.6,BK,CZ,town\n";
    assert!(matches!(
        catalogue::parse(six),
        Err(catalogue::CatalogueError::Columns {
            line: 2,
            columns: 6
        })
    ));
}

#[test]
fn jdf_r207_registry_stops_parse_with_aliases_and_merges() {
    let stops = registry::parse_stops(include_bytes!("data/geodata/registry_stops.csv")).unwrap();
    assert_eq!(stops.len(), 8);

    let first = &stops[0];
    assert_eq!((first.id, &*first.town), (1, "Předotice"));
    assert_eq!(first.district.as_deref(), Some("Nová Hospoda"));
    assert_eq!(first.nearby_place.as_deref(), Some("sil.č.20"));
    assert_eq!(first.reference, Some(Coord::new(14.050396, 49.348988)));
    assert_eq!(
        (first.status, first.merged_into, first.note.as_deref()),
        (Status::Active, None, None)
    );
    assert_eq!(first.line, 2);

    // Two spellings of number 42.
    assert_eq!((stops[3].id, &*stops[3].town), (42, "Zádub"));
    assert_eq!((stops[4].id, &*stops[4].town), (42, "Zádub-Závišín"));

    let no_reference = &stops[5];
    assert_eq!((no_reference.id, no_reference.reference), (123, None));
    assert_eq!(no_reference.okres, None);
    assert_eq!(no_reference.country.as_deref(), Some("UA"));

    assert_eq!(&*stops[6].town, "Petrovice,Krásný Les");
}

#[test]
fn jdf_r208_merged_into_note_names_the_surviving_number() {
    let stops = registry::parse_stops(include_bytes!("data/geodata/registry_stops.csv")).unwrap();
    let merged = &stops[7];
    assert_eq!((merged.id, merged.merged_into), (44, Some(42)));
    assert_eq!(merged.status, Status::Retired);
    assert_eq!(
        merged.note.as_deref(),
        Some("duplicate of 42, merged_into:42 (review 2026-03)")
    );

    let header = "id,town,district,nearby_place,okres,country,lat,lon,status,note\n";
    for note in ["merged_into:", "merged_into:7", "see merged_into:x"] {
        let csv = format!("{header}7,Adamov,,,BK,CZ,,,active,{note}\n");
        assert!(
            matches!(
                registry::parse_stops(csv.as_bytes()),
                Err(RegistryError::Row { line: 2, .. })
            ),
            "{note}"
        );
    }
}

#[test]
fn jdf_r207_registry_header_must_be_exact_and_references_whole() {
    let reordered = b"id,town,district,nearby_place,okres,country,lon,lat,status,note\n";
    assert!(matches!(
        registry::parse_stops(reordered),
        Err(RegistryError::Header { .. })
    ));

    let header = "id,town,district,nearby_place,okres,country,lat,lon,status,note\n";
    let half = format!("{header}1,Adamov,,,BK,CZ,49.3,,active,\n");
    assert!(matches!(
        registry::parse_stops(half.as_bytes()),
        Err(RegistryError::Row { line: 2, .. })
    ));
    let missing_column = format!("{header}1,Adamov,,,BK,CZ,49.3,16.6,active\n");
    assert!(matches!(
        registry::parse_stops(missing_column.as_bytes()),
        Err(RegistryError::Csv(_))
    ));

    let bom = format!("\u{feff}{header}1,Adamov,,,BK,CZ,,,active,\n");
    assert_eq!(registry::parse_stops(bom.as_bytes()).unwrap().len(), 1);
}

#[test]
fn jdf_r211_registry_posts_and_overlay_places() {
    let posts = registry::parse_posts(include_bytes!("data/geodata/registry_posts.csv")).unwrap();
    assert_eq!(posts.len(), 4);
    assert_eq!(
        (posts[2].stop_id, &posts[2].post_key),
        (3, &PostKey::Estimated(2))
    );
    assert_eq!(posts[2].reference, Some(Coord::new(14.452431, 48.984927)));
    let designated = &posts[3];
    assert_eq!(designated.post_key, PostKey::Post("2".into()));
    assert_eq!(designated.post_key.to_string(), "post:2");
    assert_eq!((designated.reference, designated.line), (None, 5));

    let header = "stop_id,post_key,lat,lon,status,note\n";
    for row in [
        "1,est:1,,,active,",
        "1,est:01,49.3,16.6,active,",
        "1,post:a b,,,active,",
    ] {
        let csv = format!("{header}{row}\n");
        assert!(
            matches!(
                registry::parse_posts(csv.as_bytes()),
                Err(RegistryError::Row { line: 2, .. })
            ),
            "{row}"
        );
    }

    let places =
        registry::parse_overlay_places(include_bytes!("data/geodata/registry_overlay_places.csv"))
            .unwrap();
    assert_eq!(places.len(), 2);
    assert_eq!(&*places[0].group_key, "ids-jmk-gtfs:U1051N3249");
    assert_eq!(
        &*places[0].place_id,
        "overlay:ids-jmk-gtfs:stop-place:8d413e7d92a6065d"
    );
    assert_eq!(places[0].note.as_deref(), Some("Bystrc, přístav"));
}

fn gzip(json: &str) -> Vec<u8> {
    let mut encoder = GzEncoder::new(Vec::new(), Compression::fast());
    encoder.write_all(json.as_bytes()).unwrap();
    encoder.finish().unwrap()
}

const SQUARE: &str = "[[[14.0,50.0],[14.1,50.0],[14.1,50.1],[14.0,50.0]]]";

#[test]
fn jdf_r015_okres_with_two_features_keeps_both_polygons() {
    let json = format!(
        r#"{{"type":"FeatureCollection","name":"regions_simplified",
        "crs":{{"type":"name","properties":{{"name":"urn:ogc:def:crs:OGC:1.3:CRS84"}}}},
        "features":[
        {{"type":"Feature","properties":{{"cat":40,"name":"Liberec","code":"LT"}},
          "geometry":{{"type":"Polygon","coordinates":{SQUARE}}}}},
        {{"type":"Feature","properties":{{"cat":1,"name":"Praha","code":"AB"}},
          "geometry":{{"type":"MultiPolygon","coordinates":[{SQUARE},{SQUARE}]}}}},
        {{"type":"Feature","properties":{{"cat":41,"name":"Liberec","code":"LT"}},
          "geometry":{{"type":"Polygon","coordinates":
            [[[15.0,50.7],[15.2,50.7],[15.2,50.9,300.0],[15.0,50.7]],
             [[15.05,50.75],[15.1,50.75],[15.1,50.8],[15.05,50.75]]]}}}}]}}"#
    );
    let okresy = gazetteer::parse_okresy(gzip(&json).as_slice()).unwrap();
    let codes: Vec<&str> = okresy.iter().map(|okres| &*okres.code).collect();
    assert_eq!(codes, ["LT", "AB"]);

    let liberec = &okresy[0];
    assert_eq!(liberec.name.as_deref(), Some("Liberec"));
    assert_eq!(liberec.polygons.len(), 2);
    assert_eq!(liberec.polygons[0].exterior[0], Coord::new(14.0, 50.0));
    assert_eq!(liberec.polygons[1].exterior[2], Coord::new(15.2, 50.9));
    assert_eq!(liberec.polygons[1].holes.len(), 1);
    assert_eq!(okresy[1].polygons.len(), 2);
}

#[test]
fn jdf_r016_town_keeps_name_okres_and_projected_polygon() {
    let json = r#"{"type":"FeatureCollection","name":"obce_etrs89ex",
        "crs":{"type":"name","properties":{"name":"urn:ogc:def:crs:EPSG::3035"}},
        "features":[
        {"type":"Feature","properties":{"name":"Bohuňovice","region":"OC"},
         "geometry":{"type":"Polygon","coordinates":
           [[[4844438.7,2972911.4],[4844438.1,2972912.2],[4844413.5,2972917.7],[4844438.7,2972911.4]]]}},
        {"type":"Feature","properties":{"name":"Nová Ves","region":null},"geometry":null}]}"#;
    let towns = gazetteer::parse_towns(gzip(json).as_slice()).unwrap();
    assert_eq!(towns.len(), 2);
    let town = &towns[0];
    assert_eq!(
        (&*town.name, town.okres.as_deref()),
        ("Bohuňovice", Some("OC"))
    );
    assert_eq!(
        town.polygons[0].exterior[0],
        Laea {
            easting: 4844438.7,
            northing: 2972911.4
        }
    );
    assert_eq!(town.polygons[0].exterior.len(), 4);
    assert_eq!(
        (
            towns[1].okres.as_deref(),
            towns[1].polygons.len(),
            towns[1].feature
        ),
        (None, 0, 1)
    );

    // The town file is not WGS84, and an okres file must be: the wrong system is an error.
    assert!(matches!(
        gazetteer::parse_okresy(gzip(json).as_slice()),
        Err(GazetteerError::Crs { .. })
    ));
    assert!(matches!(
        gazetteer::parse_towns(json.as_bytes()),
        Err(GazetteerError::Gzip(_))
    ));
}

#[test]
fn jdf_r016_foreign_towns_with_and_without_coordinates() {
    let towns =
        gazetteer::parse_foreign_towns(include_bytes!("data/geodata/eur_towns.csv")).unwrap();
    assert_eq!(towns.len(), 6);
    let beiersdorf = &towns[3];
    assert_eq!(
        (&*beiersdorf.name, &*beiersdorf.country),
        ("Beiersdorf, Saxony", "de")
    );
    assert_eq!(beiersdorf.latitude, Some(51.0695107));
    let dresden = &towns[4];
    assert_eq!(
        (&*dresden.name, dresden.latitude, dresden.longitude),
        ("Dresden", None, None)
    );
    assert_eq!(dresden.line, 5);
}

#[test]
fn jdf_r215_synonym_lines_as_written() {
    let lines = synonyms::parse(include_str!("data/geodata/synonyms.txt"));
    assert_eq!(
        lines[0].alternatives,
        ["nám.".into(), "náměstí".into()] as [Box<str>; 2]
    );
    assert_eq!(lines[0].line, 9);
    let station = lines
        .iter()
        .find(|line| &*line.alternatives[0] == "nádr.")
        .unwrap();
    assert_eq!(station.alternatives.len(), 5);
    assert_eq!(station.line, 17);
    let last = lines.last().unwrap();
    assert_eq!(
        last.alternatives,
        ["ul.".into(), "ulice".into()] as [Box<str>; 2]
    );
    assert_eq!(last.line, 21);
}

#[test]
fn jdf_r406_licence_pattern_forms() {
    let modes =
        route_rules::parse_transport_modes(include_bytes!("data/geodata/transport-modes.csv"))
            .unwrap();
    assert_eq!(modes.len(), 7);
    let trams = &modes[0];
    assert_eq!(trams.agency_ico.as_deref(), Some("61974757"));
    assert_eq!(&*trams.licence, "915001-915019");
    assert_eq!(
        trams.licence_pattern,
        LicencePattern::Range {
            low: "915001".into(),
            high: "915019".into()
        }
    );
    assert_eq!(trams.public_lines, Some(PublicLines { low: 1, high: 19 }));
    assert_eq!((&*trams.expected_mode, &*trams.effective_mode), ("A", "E"));
    assert_eq!(
        modes[2].licence_pattern,
        LicencePattern::Licence("545002".into())
    );
    assert_eq!(modes[2].public_lines, Some(PublicLines { low: 2, high: 2 }));

    let presentation =
        route_rules::parse_presentation(include_bytes!("data/geodata/presentation.csv")).unwrap();
    assert_eq!(presentation.len(), 6);
    let teplice = &presentation[3];
    assert_eq!(teplice.licence_pattern, LicencePattern::Prefix("".into()));
    assert_eq!(
        (
            teplice.route_short_name.as_deref(),
            teplice.route_color.as_deref()
        ),
        (None, Some("283286"))
    );
    assert_eq!(presentation[4].agency_ico, None);
    assert_eq!(
        presentation[4].licence_pattern,
        LicencePattern::Licence("001401".into())
    );
    assert_eq!(presentation[4].line, 6);
}

#[test]
fn jdf_r405_malformed_route_rule_rows_are_errors() {
    let header = "agency_id,licence,public_line,expected_mode,effective_mode,reason\n";
    for row in [
        "61974757,915019-915001,,A,E,reversed range",
        "61974757,91501-915019,,A,E,unequal range",
        "61974757,9150a*,,A,E,bad prefix",
        "61974757,915001,19-1,A,E,reversed lines",
        "61974757,915001,,A,X,unknown mode",
        "ABC,915001,,A,E,agency not an IČO",
        "61974757,915001,,A,E,",
    ] {
        let csv = format!("{header}{row}\n");
        assert!(
            matches!(
                route_rules::parse_transport_modes(csv.as_bytes()),
                Err(RouteRulesError::Row { line: 2, .. })
            ),
            "{row}"
        );
    }
    let header = "agency_id,licence,route_short_name,route_color,route_text_color,reason\n";
    for row in [
        ",001401,,00843g,,bad colour",
        ",001401,,,,overrides nothing",
    ] {
        let csv = format!("{header}{row}\n");
        assert!(
            matches!(
                route_rules::parse_presentation(csv.as_bytes()),
                Err(RouteRulesError::Row { line: 2, .. })
            ),
            "{row}"
        );
    }
}
