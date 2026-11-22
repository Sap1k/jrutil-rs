//! JDF batches: the Czech bus and urban timetable exchange format, versions 1.8 to 1.11.
//!
//! A batch is a ZIP or a directory of CP1250 text files. [`read_zip`] and [`read_files`] return
//! every relation in file order with every field as written (JDF-R017–R024,
//! `docs/rules/jdf-read.md`): one record type per relation, the superset of all versions, where a
//! field the batch's version does not have is `None`. Nothing is defaulted, repaired or resolved:
//! the version defaults, fixed-code meanings and validity bounds are the compiler's rules.
//!
//! What stops a batch from being read at all is a [`JdfError`]: no `VerzeJDF.txt`, an unknown
//! version, more than one batch in the container, a missing required file, two entries for one
//! file. Everything else is a [`Problem`] next to the batch: a record that does not follow the
//! grammar or has too few fields, or whose required field does not decode, is skipped; an optional
//! field that does not decode is `None`; tolerated deviations (LF line ends, a final record
//! without terminator) are read. Nothing panics on input.

mod codes;
mod files;
mod grammar;
mod records;
mod relations;

use std::fmt;
use std::io::{Read as _, Seek};

pub use codes::{CompanyType, Km, NoteType, RouteType, TimeCell, TransportMode};
pub use records::{
    Agency, AgencyAlternation, AlternateRouteName, AttributeRef, Header, Reservation, Route,
    RouteInfo, RouteIntegration, RouteStop, ServiceNote, Stop, StopPost, Transfer, Trip, TripGroup,
    TripStop,
};

use files::File;
use grammar::{Row, Skip, problem};
use relations::relation;

/// The format version a batch declares (`VerzeJDF.txt`, JDF-R018).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Version {
    V1_8,
    V1_9,
    V1_10,
    V1_11,
}

impl Version {
    /// As `VerzeJDF.txt` writes it.
    pub fn as_str(self) -> &'static str {
        match self {
            Version::V1_8 => "1.8",
            Version::V1_9 => "1.9",
            Version::V1_10 => "1.10",
            Version::V1_11 => "1.11",
        }
    }

    fn parse(text: &str) -> Option<Version> {
        match text {
            "1.8" => Some(Version::V1_8),
            "1.9" => Some(Version::V1_9),
            "1.10" => Some(Version::V1_10),
            "1.11" => Some(Version::V1_11),
            _ => None,
        }
    }

    /// The field layout: 0 = 1.8/1.9 (one layout), 1 = 1.10, 2 = 1.11.
    fn layout(self) -> usize {
        match self {
            Version::V1_8 | Version::V1_9 => 0,
            Version::V1_10 => 1,
            Version::V1_11 => 2,
        }
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One batch as it says itself: every relation in file order, every field as written. A relation
/// whose file is absent, or which the batch's version does not have, is empty.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Batch {
    pub version: Version,
    /// `VerzeJDF.txt`.
    pub header: Header,
    /// `Zastavky.txt`.
    pub stops: Vec<Stop>,
    /// `Oznacniky.txt` (1.10 and later).
    pub stop_posts: Vec<StopPost>,
    /// `Dopravci.txt`.
    pub agencies: Vec<Agency>,
    /// `Linky.txt`.
    pub routes: Vec<Route>,
    /// `LinExt.txt` (1.10 and later).
    pub route_integrations: Vec<RouteIntegration>,
    /// `Zaslinky.txt`.
    pub route_stops: Vec<RouteStop>,
    /// `Spoje.txt`.
    pub trips: Vec<Trip>,
    /// `SpojSkup.txt` (1.10 and later).
    pub trip_groups: Vec<TripGroup>,
    /// `Zasspoje.txt`.
    pub trip_stops: Vec<TripStop>,
    /// `Udaje.txt`.
    pub route_info: Vec<RouteInfo>,
    /// `Pevnykod.txt`.
    pub attribute_refs: Vec<AttributeRef>,
    /// `Caskody.txt`.
    pub service_notes: Vec<ServiceNote>,
    /// `Navaznosti.txt` (1.10 and later).
    pub transfers: Vec<Transfer>,
    /// `Altdop.txt`.
    pub agency_alternations: Vec<AgencyAlternation>,
    /// `Altlinky.txt`.
    pub alternate_route_names: Vec<AlternateRouteName>,
    /// `Mistenky.txt`.
    pub reservations: Vec<Reservation>,
}

/// Something that did not stop the batch from being read: a record without its terminator, a
/// record with the wrong number of fields, a value that does not parse.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Problem {
    /// The file's name as the specification spells it (`Zasspoje.txt`).
    pub file: Box<str>,
    /// The record's 1-based number in the file; 0 for the file as a whole.
    pub record: u32,
    /// The record field (as named in this module's types), where one applies.
    pub field: Option<&'static str>,
    pub message: Box<str>,
}

impl fmt::Display for Problem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.file)?;
        if self.record > 0 {
            write!(f, " record {}", self.record)?;
        }
        if let Some(field) = self.field {
            write!(f, " {field}")?;
        }
        write!(f, ": {}", self.message)
    }
}

/// A batch and the problems met reading it, in file order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Read {
    pub batch: Batch,
    pub problems: Vec<Problem>,
}

#[derive(Debug, thiserror::Error)]
pub enum JdfError {
    #[error("ZIP: {0}")]
    Zip(#[from] zip::result::ZipError),
    #[error("ZIP member {name}: {source}")]
    Member {
        name: String,
        #[source]
        source: std::io::Error,
    },
    #[error("no VerzeJDF.txt")]
    NoVersionFile,
    #[error("more than one batch: VerzeJDF.txt in {0:?}")]
    SeveralBatches(Vec<Box<str>>),
    #[error("more than one {0} (names compared ignoring case)")]
    DuplicateFile(&'static str),
    #[error("unknown JDF version: \"{0}\"")]
    UnknownVersion(Box<str>),
    #[error("JDF {version} batch without {file}")]
    MissingFile {
        version: Version,
        file: &'static str,
    },
}

/// Reads a batch from the files of a ZIP archive. The batch may sit at the root or in a
/// directory; file names are matched ignoring ASCII case.
pub fn read_zip<R: std::io::Read + Seek>(zip: R) -> Result<Read, JdfError> {
    let mut archive = zip::ZipArchive::new(zip)?;
    let mut names = Vec::with_capacity(archive.len());
    for index in 0..archive.len() {
        let name = archive
            .name_for_index(index)
            .transpose()?
            .map_or_else(String::new, |name| name.into_owned());
        names.push(name);
    }
    let found = files::locate(names.iter().map(|name| {
        if name.ends_with('/') {
            ""
        } else {
            name.as_str()
        }
    }))?;
    let mut contents: [Option<Vec<u8>>; 17] = Default::default();
    for (slot, index) in contents.iter_mut().zip(found) {
        let Some(index) = index else { continue };
        let mut file = archive.by_index(index)?;
        let mut bytes = Vec::with_capacity(usize::try_from(file.size()).unwrap_or(0));
        if let Err(source) = file.read_to_end(&mut bytes) {
            return Err(JdfError::Member {
                name: names[index].clone(),
                source,
            });
        }
        *slot = Some(bytes);
    }
    read_batch(contents.each_ref().map(Option::as_deref))
}

/// Reads a batch from named file contents (name, bytes), for a directory the caller has read.
/// Names may carry a directory part (`/` or `\`); the batch is the directory holding
/// `VerzeJDF.txt`, and file names are matched ignoring ASCII case.
pub fn read_files<'a>(
    files: impl IntoIterator<Item = (&'a str, &'a [u8])>,
) -> Result<Read, JdfError> {
    let files: Vec<(&str, &[u8])> = files.into_iter().collect();
    let found = files::locate(files.iter().map(|&(name, _)| name))?;
    read_batch(found.map(|index| index.map(|index| files[index].1)))
}

fn read_batch(contents: [Option<&[u8]>; 17]) -> Result<Read, JdfError> {
    let mut problems = Vec::new();
    let verze_name = File::Verze.name();
    let verze = contents[File::Verze as usize].ok_or(JdfError::NoVersionFile)?;
    let verze = grammar::decode(verze, verze_name, &mut problems);
    let header = header(&verze, &mut problems)?;
    let version = Version::parse(&header.version)
        .ok_or_else(|| JdfError::UnknownVersion(header.version.clone()))?;
    let layout = version.layout();

    let mut texts: [Option<std::borrow::Cow<'_, str>>; 17] = Default::default();
    for file in File::ALL.into_iter().skip(1) {
        let name = file.name();
        match contents[file as usize] {
            None if file.required() => {
                return Err(JdfError::MissingFile {
                    version,
                    file: name,
                });
            }
            None => {}
            Some(_) if layout == 0 && file.since_1_10() => problems.push(problem(
                name,
                0,
                None,
                format!("not part of JDF {version}: not read"),
            )),
            Some(bytes) => texts[file as usize] = Some(grammar::decode(bytes, name, &mut problems)),
        }
    }

    macro_rules! read {
        ($file:expr, $counts:expr, $decode:expr $(,)?) => {
            match &texts[$file as usize] {
                Some(text) => relation(text, $file.name(), $counts, layout, &mut problems, $decode),
                None => Vec::new(),
            }
        };
    }
    use relations as r;
    let batch = Batch {
        version,
        header,
        stops: read!(File::Zastavky, r::STOP, r::stop),
        stop_posts: read!(File::Oznacniky, r::STOP_POST, r::stop_post),
        agencies: read!(File::Dopravci, r::AGENCY, r::agency),
        routes: read!(File::Linky, r::ROUTE, r::route),
        route_integrations: read!(File::LinExt, r::ROUTE_INTEGRATION, r::route_integration),
        route_stops: read!(File::Zaslinky, r::ROUTE_STOP, r::route_stop),
        trips: read!(File::Spoje, r::TRIP, r::trip),
        trip_groups: read!(File::SpojSkup, r::TRIP_GROUP, r::trip_group),
        trip_stops: read!(File::Zasspoje, r::TRIP_STOP, r::trip_stop),
        route_info: read!(File::Udaje, r::ROUTE_INFO, r::route_info),
        attribute_refs: read!(File::Pevnykod, r::ATTRIBUTE_REF, r::attribute_ref),
        service_notes: read!(File::Caskody, r::SERVICE_NOTE, r::service_note),
        transfers: read!(File::Navaznosti, r::TRANSFER, r::transfer),
        agency_alternations: read!(File::Altdop, r::AGENCY_ALTERNATION, r::agency_alternation),
        alternate_route_names: read!(
            File::Altlinky,
            r::ALTERNATE_ROUTE_NAME,
            r::alternate_route_name,
        ),
        reservations: read!(File::Mistenky, r::RESERVATION, r::reservation),
    };
    Ok(Read { batch, problems })
}

/// `VerzeJDF.txt`: its first record. The version field is required; fields 2–6 are read where
/// present (real 1.8 batches write only the version).
fn header(text: &str, problems: &mut Vec<Problem>) -> Result<Header, JdfError> {
    let file = File::Verze.name();
    let mut header = None;
    grammar::records(text, file, problems, |record, fields, problems| {
        if header.is_some() {
            if record == 2 {
                problems.push(problem(
                    file,
                    record,
                    None,
                    "more than one record: not read",
                ));
            }
            return;
        }
        if fields.len() > 6 {
            problems.push(problem(
                file,
                record,
                None,
                format!(
                    "{} fields, at most 6 expected: the extra fields are not read",
                    fields.len()
                ),
            ));
        }
        let mut row = Row {
            fields,
            layout: 0,
            file,
            record,
            problems,
        };
        header = Some(header_fields(&mut row));
    });
    match header {
        Some(Ok(header)) => Ok(header),
        _ => Err(JdfError::UnknownVersion("".into())),
    }
}

fn header_fields(r: &mut Row<'_, '_>) -> Result<Header, Skip> {
    use grammar::all;
    Ok(Header {
        version: r.text(all(1), "version")?,
        du_number: r.opt(all(2), "du_number"),
        region: r.opt_text(all(3)),
        batch_id: r.opt_text(all(4)),
        created: r.opt(all(5), "created"),
        generator: r.opt_text(all(6)),
    })
}
