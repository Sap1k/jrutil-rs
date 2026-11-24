//! Gazetteers (`jrunify-ext-geodata/gazetteer`, `docs/rules/jdf-read.md` 2.8, JDF-R011,
//! R015, R016): okres polygons (`cz_regions.json.gz`), country polygons
//! (`eur_countries.json.gz`), Czech municipality polygons (`cz_towns.json.gz`) and European town
//! names (`eur_towns_countries.csv`).
//!
//! The two GeoJSON files are read as a stream (gzip, then JSON) straight into the returned
//! types. They are stored in different coordinate systems, and the reader returns each in its
//! own: okres and country polygons are WGS84 (`CRS84`, longitude/latitude) and come as [`Coord`]; municipality
//! polygons are ETRS89-LAEA (`EPSG:3035`, metres) and come as [`Laea`]. Projecting between them
//! is not a reader's business. A file declaring another coordinate system is an error.

use std::collections::HashMap;
use std::fmt;
use std::io::{BufReader, Read};
use std::marker::PhantomData;

use flate2::read::MultiGzDecoder;
use jr_model::Coord;
use serde::Deserialize;
use serde::de::{self, Deserializer, IgnoredAny, MapAccess, SeqAccess, Visitor};

/// The `crs` names accepted for WGS84 longitude/latitude. A file without `crs` is WGS84 too
/// (RFC 7946).
const CRS84: [&str; 2] = [
    "urn:ogc:def:crs:OGC:1.3:CRS84",
    "urn:ogc:def:crs:OGC::CRS84",
];
/// The `crs` names accepted for ETRS89-extended / LAEA Europe.
const EPSG_3035: [&str; 2] = ["urn:ogc:def:crs:EPSG::3035", "EPSG:3035"];

/// A position in ETRS89-extended / LAEA Europe (EPSG:3035), in metres, GeoJSON axis order.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Laea {
    pub easting: f64,
    pub northing: f64,
}

/// A coordinate type a GeoJSON position (`[x, y, ...]`) is read into.
pub trait Position: Sized {
    fn from_xy(x: f64, y: f64) -> Self;
}

impl Position for Coord {
    fn from_xy(x: f64, y: f64) -> Self {
        Coord::new(x, y)
    }
}

impl Position for Laea {
    fn from_xy(x: f64, y: f64) -> Self {
        Laea {
            easting: x,
            northing: y,
        }
    }
}

/// One polygon: its exterior ring and holes, rings as written (closed, as GeoJSON requires).
#[derive(Clone, Debug, PartialEq)]
pub struct Polygon<P = Coord> {
    pub exterior: Vec<P>,
    pub holes: Vec<Vec<P>>,
}

/// An area named by a code (an okres, a country): every polygon of every feature with its code,
/// in file order (bug B2: a code may occur on more than one feature; okres `LT` does, and most
/// countries with islands or exclaves).
#[derive(Clone, Debug, PartialEq)]
pub struct CodedArea {
    /// The `code` property.
    pub code: Box<str>,
    /// The `name` property of the code's first feature, as written (mis-encoded in the real
    /// file).
    pub name: Option<Box<str>>,
    pub polygons: Vec<Polygon>,
}

/// An okres of `cz_regions.json.gz`; its code is the JDF okres code.
pub type Okres = CodedArea;

/// A country of `eur_countries.json.gz`; its code is ISO 3166-1 alpha-2.
pub type Country = CodedArea;

/// One municipality feature. Names repeat (`Nová Ves` 14 times); each feature is its own town.
#[derive(Clone, Debug, PartialEq)]
pub struct Town {
    /// The `name` property.
    pub name: Box<str>,
    /// The `region` property: the okres code.
    pub okres: Option<Box<str>>,
    /// The feature's polygons in EPSG:3035.
    pub polygons: Vec<Polygon<Laea>>,
    /// The 0-based index of the feature in the file.
    pub feature: usize,
}

/// One row of `eur_towns_countries.csv` (`name,country,lat,lon`).
#[derive(Clone, Debug, PartialEq)]
pub struct ForeignTown {
    /// Column 1 verbatim.
    pub name: Box<str>,
    /// Column 2 verbatim (lower-case ISO alpha-2 in the real file).
    pub country: Box<str>,
    /// Column 3, when a finite decimal number (blank where the name is ambiguous).
    pub latitude: Option<f64>,
    /// Column 4, when a finite decimal number.
    pub longitude: Option<f64>,
    pub line: u64,
}

#[derive(Debug, thiserror::Error)]
pub enum GazetteerError {
    #[error("gazetteer: cannot decompress: {0}")]
    Gzip(std::io::Error),
    #[error("gazetteer: {0}")]
    Json(serde_json::Error),
    #[error("gazetteer: {0}")]
    Csv(#[from] csv::Error),
    #[error("gazetteer: top-level type is {found:?}, FeatureCollection expected")]
    NotFeatureCollection { found: String },
    #[error("gazetteer: coordinate system {found:?}, {expected} expected")]
    Crs {
        found: Option<String>,
        expected: &'static str,
    },
    #[error("gazetteer: feature {feature} has no {property:?} property")]
    MissingProperty {
        feature: usize,
        property: &'static str,
    },
    #[error("gazetteer line {line}: {columns} column(s), four expected")]
    Columns { line: u64, columns: usize },
}

impl From<serde_json::Error> for GazetteerError {
    fn from(error: serde_json::Error) -> Self {
        if error.is_io() {
            Self::Gzip(error.into())
        } else {
            Self::Json(error)
        }
    }
}

/// Parses `cz_regions.json.gz`: one [`Okres`] per distinct code, in order of first occurrence.
pub fn parse_okresy<R: Read>(gzip_json: R) -> Result<Vec<Okres>, GazetteerError> {
    parse_coded_areas(gzip_json)
}

/// Parses `eur_countries.json.gz`: one [`Country`] per distinct code, in order of first
/// occurrence.
pub fn parse_countries<R: Read>(gzip_json: R) -> Result<Vec<Country>, GazetteerError> {
    parse_coded_areas(gzip_json)
}

/// A WGS84 collection of features with `code` and `name` properties, grouped by code.
fn parse_coded_areas<R: Read>(gzip_json: R) -> Result<Vec<CodedArea>, GazetteerError> {
    #[derive(Deserialize)]
    struct Properties {
        code: Option<String>,
        name: Option<String>,
    }
    let collection = read_collection::<Properties, Coord, R>(gzip_json)?;
    check_crs(collection.crs, &CRS84, true, "CRS84 (WGS84)")?;
    let mut areas: Vec<CodedArea> = Vec::new();
    let mut by_code: HashMap<String, usize> = HashMap::new();
    for (
        feature,
        Feature {
            properties,
            polygons,
        },
    ) in collection.features.into_iter().enumerate()
    {
        let Properties { code, name } = properties.unwrap_or(Properties {
            code: None,
            name: None,
        });
        let code = code.ok_or(GazetteerError::MissingProperty {
            feature,
            property: "code",
        })?;
        let index = *by_code.entry(code).or_insert_with_key(|code| {
            areas.push(CodedArea {
                code: code.as_str().into(),
                name: name.map(Box::from),
                polygons: Vec::new(),
            });
            areas.len() - 1
        });
        areas[index].polygons.extend(polygons);
    }
    Ok(areas)
}

/// Parses `cz_towns.json.gz`: one [`Town`] per feature, in file order.
pub fn parse_towns<R: Read>(gzip_json: R) -> Result<Vec<Town>, GazetteerError> {
    #[derive(Deserialize)]
    struct Properties {
        name: Option<String>,
        region: Option<String>,
    }
    let collection = read_collection::<Properties, Laea, R>(gzip_json)?;
    check_crs(collection.crs, &EPSG_3035, false, "EPSG:3035")?;
    collection
        .features
        .into_iter()
        .enumerate()
        .map(
            |(
                feature,
                Feature {
                    properties,
                    polygons,
                },
            )| {
                let (name, region) = properties.map_or((None, None), |p| (p.name, p.region));
                Ok(Town {
                    name: name
                        .ok_or(GazetteerError::MissingProperty {
                            feature,
                            property: "name",
                        })?
                        .into(),
                    okres: region.map(Box::from),
                    polygons,
                    feature,
                })
            },
        )
        .collect()
}

/// Parses `eur_towns_countries.csv` (headerless, exactly four columns per row).
pub fn parse_foreign_towns(csv: &[u8]) -> Result<Vec<ForeignTown>, GazetteerError> {
    let mut reader = csv::ReaderBuilder::new()
        .has_headers(false)
        .flexible(true)
        .from_reader(csv);
    let mut towns = Vec::new();
    let mut record = csv::StringRecord::new();
    while reader.read_record(&mut record)? {
        let line = super::line_of(&record);
        if record.len() != 4 {
            return Err(GazetteerError::Columns {
                line,
                columns: record.len(),
            });
        }
        towns.push(ForeignTown {
            name: record[0].into(),
            country: record[1].into(),
            latitude: super::finite_number(&record[2]),
            longitude: super::finite_number(&record[3]),
            line,
        });
    }
    Ok(towns)
}

/// `absent_is_accepted`: a file without `crs` is WGS84 (RFC 7946), so only a WGS84 reader
/// accepts it.
fn check_crs(
    crs: Option<Crs>,
    accepted: &[&str],
    absent_is_accepted: bool,
    expected: &'static str,
) -> Result<(), GazetteerError> {
    match crs {
        None if absent_is_accepted => Ok(()),
        Some(Crs { properties }) if accepted.contains(&properties.name.as_str()) => Ok(()),
        other => Err(GazetteerError::Crs {
            found: other.map(|crs| crs.properties.name),
            expected,
        }),
    }
}

fn read_collection<Props, P, R>(gzip_json: R) -> Result<Collection<Props, P>, GazetteerError>
where
    Props: for<'de> Deserialize<'de>,
    P: Position,
    R: Read,
{
    let reader = BufReader::with_capacity(1 << 16, MultiGzDecoder::new(gzip_json));
    let collection: Collection<Props, P> = serde_json::from_reader(reader)?;
    if collection.kind != "FeatureCollection" {
        return Err(GazetteerError::NotFeatureCollection {
            found: collection.kind,
        });
    }
    Ok(collection)
}

#[derive(Deserialize)]
#[serde(bound(deserialize = "Props: Deserialize<'de>, P: Position"))]
struct Collection<Props, P> {
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    crs: Option<Crs>,
    features: Vec<Feature<Props, P>>,
}

#[derive(Deserialize)]
struct Crs {
    properties: CrsProperties,
}

#[derive(Deserialize)]
struct CrsProperties {
    name: String,
}

/// A feature's properties and polygons (none for a `null` geometry).
struct Feature<Props, P> {
    properties: Option<Props>,
    polygons: Vec<Polygon<P>>,
}

impl<'de, Props: Deserialize<'de>, P: Position> Deserialize<'de> for Feature<Props, P> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(bound(deserialize = "Props: Deserialize<'de>, P: Position"))]
        struct Raw<Props, P> {
            #[serde(default)]
            properties: Option<Props>,
            #[serde(default)]
            geometry: Option<Geometry<P>>,
        }
        let raw = Raw::<Props, P>::deserialize(deserializer)?;
        Ok(Feature {
            properties: raw.properties,
            polygons: raw.geometry.map_or_else(Vec::new, |geometry| geometry.0),
        })
    }
}

/// A `Polygon` or `MultiPolygon` geometry. `type` must precede `coordinates`, as GDAL writes
/// it; anything else is an error rather than a buffered second pass.
struct Geometry<P>(Vec<Polygon<P>>);

impl<'de, P: Position> Deserialize<'de> for Geometry<P> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct GeometryVisitor<P>(PhantomData<P>);
        impl<'de, P: Position> Visitor<'de> for GeometryVisitor<P> {
            type Value = Geometry<P>;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a GeoJSON Polygon or MultiPolygon")
            }
            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
                let mut kind: Option<String> = None;
                let mut polygons = None;
                while let Some(key) = map.next_key::<String>()? {
                    match key.as_str() {
                        "type" => kind = Some(map.next_value()?),
                        "coordinates" => {
                            polygons = Some(match kind.as_deref() {
                                Some("Polygon") => vec![map.next_value::<Rings<P>>()?.0],
                                Some("MultiPolygon") => map
                                    .next_value::<Vec<Rings<P>>>()?
                                    .into_iter()
                                    .map(|rings| rings.0)
                                    .collect(),
                                Some(other) => {
                                    return Err(de::Error::custom(format!(
                                        "unsupported geometry type {other}"
                                    )));
                                }
                                None => {
                                    return Err(de::Error::custom(
                                        "geometry coordinates before its type",
                                    ));
                                }
                            });
                        }
                        _ => {
                            map.next_value::<IgnoredAny>()?;
                        }
                    }
                }
                match (kind, polygons) {
                    (_, Some(polygons)) => Ok(Geometry(polygons)),
                    (Some(kind), None) => Err(de::Error::custom(format!(
                        "geometry {kind} without polygon coordinates"
                    ))),
                    (None, None) => Err(de::Error::missing_field("type")),
                }
            }
        }
        deserializer.deserialize_map(GeometryVisitor(PhantomData))
    }
}

/// A polygon's rings: the first is the exterior, the rest are holes.
struct Rings<P>(Polygon<P>);

impl<'de, P: Position> Deserialize<'de> for Rings<P> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct RingsVisitor<P>(PhantomData<P>);
        impl<'de, P: Position> Visitor<'de> for RingsVisitor<P> {
            type Value = Rings<P>;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a list of linear rings")
            }
            fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
                let exterior = seq
                    .next_element::<Ring<P>>()?
                    .map_or_else(Vec::new, |r| r.0);
                let mut holes = Vec::new();
                while let Some(Ring(hole)) = seq.next_element()? {
                    holes.push(hole);
                }
                Ok(Rings(Polygon { exterior, holes }))
            }
        }
        deserializer.deserialize_seq(RingsVisitor(PhantomData))
    }
}

/// A list of positions, read straight into `P`.
struct Ring<P>(Vec<P>);

impl<'de, P: Position> Deserialize<'de> for Ring<P> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct RingVisitor<P>(PhantomData<P>);
        impl<'de, P: Position> Visitor<'de> for RingVisitor<P> {
            type Value = Ring<P>;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a list of positions")
            }
            fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
                let mut positions = Vec::with_capacity(seq.size_hint().unwrap_or(0));
                while let Some(Xy(x, y)) = seq.next_element()? {
                    positions.push(P::from_xy(x, y));
                }
                Ok(Ring(positions))
            }
        }
        deserializer.deserialize_seq(RingVisitor(PhantomData))
    }
}

/// A GeoJSON position: the first two numbers; an altitude or more is skipped.
struct Xy(f64, f64);

impl<'de> Deserialize<'de> for Xy {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct XyVisitor;
        impl<'de> Visitor<'de> for XyVisitor {
            type Value = Xy;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a position of at least two numbers")
            }
            fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
                let x = seq
                    .next_element()?
                    .ok_or_else(|| de::Error::invalid_length(0, &self))?;
                let y = seq
                    .next_element()?
                    .ok_or_else(|| de::Error::invalid_length(1, &self))?;
                while seq.next_element::<IgnoredAny>()?.is_some() {}
                Ok(Xy(x, y))
            }
        }
        deserializer.deserialize_seq(XyVisitor)
    }
}
