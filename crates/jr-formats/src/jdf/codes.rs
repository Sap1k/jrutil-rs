//! The scalar value types of JDF fields: call time cells, kilometres and the one-letter codes.
//!
//! A code enum has one variant per code the JDF specification lists and `Other` for anything
//! else, which is kept as written: an unknown code is a fact, not an error.

/// A call time cell of `Zasspoje.txt` (arrival, departure, and in 1.11 the earliest arrival and
/// latest departure), exactly as written; nothing is interpreted. A time is a clock reading of the
/// service day without a day offset: a trip past midnight simply shows smaller times.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TimeCell {
    /// `""`: no time given.
    Empty,
    /// `HHMM` (`0000`–`2359`), as minutes after midnight.
    At { minutes: u16 },
    /// `|`: the trip passes the stop without stopping.
    Passes,
    /// `<`: the trip does not run via this stop (it takes another way).
    OtherRoute,
}

impl TimeCell {
    pub(super) fn parse(text: &str) -> Option<TimeCell> {
        match text.as_bytes() {
            [] => Some(TimeCell::Empty),
            b"|" => Some(TimeCell::Passes),
            b"<" => Some(TimeCell::OtherRoute),
            &[h1, h2, m1, m2] if [h1, h2, m1, m2].iter().all(u8::is_ascii_digit) => {
                let hours = u16::from(h1 - b'0') * 10 + u16::from(h2 - b'0');
                let minutes = u16::from(m1 - b'0') * 10 + u16::from(m2 - b'0');
                (hours < 24 && minutes < 60).then_some(TimeCell::At {
                    minutes: hours * 60 + minutes,
                })
            }
            _ => None,
        }
    }
}

/// A kilometre value as written: `units / 10^scale`, so `12` is `{12, 0}` and `12.50` is
/// `{1250, 2}`. The source's precision is kept; real batches write whole kilometres.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Km {
    pub units: u64,
    /// Digits after the decimal point.
    pub scale: u8,
}

impl Km {
    /// The value in kilometres.
    pub fn value(self) -> f64 {
        self.units as f64 / 10f64.powi(i32::from(self.scale))
    }

    /// Digits with an optional `.` and fraction digits.
    pub(super) fn parse(text: &str) -> Option<Km> {
        let (whole, fraction) = match text.split_once('.') {
            Some((whole, fraction)) if !fraction.is_empty() => (whole, fraction),
            Some(_) => return None,
            None => (text, ""),
        };
        if whole.is_empty() {
            return None;
        }
        let mut units: u64 = 0;
        for byte in whole.bytes().chain(fraction.bytes()) {
            if !byte.is_ascii_digit() {
                return None;
            }
            units = units.checked_mul(10)?.checked_add(u64::from(byte - b'0'))?;
        }
        Some(Km {
            units,
            scale: u8::try_from(fraction.len()).ok()?,
        })
    }
}

macro_rules! codes {
    ($(#[$meta:meta])* $name:ident { $($(#[$vmeta:meta])* $variant:ident = $code:literal,)* }) => {
        $(#[$meta])*
        #[derive(Clone, Debug, PartialEq, Eq, Hash)]
        pub enum $name {
            $($(#[$vmeta])* $variant,)*
            /// A code the specification does not list, as written.
            Other(Box<str>),
        }

        impl $name {
            /// The code as written in the batch.
            pub fn code(&self) -> &str {
                match self {
                    $($name::$variant => $code,)*
                    $name::Other(code) => code,
                }
            }

            pub(super) fn from_code(code: &str) -> $name {
                match code {
                    $($code => $name::$variant,)*
                    _ => $name::Other(code.into()),
                }
            }
        }
    };
}

codes! {
    /// `Dopravci.txt` field 4, druh firmy.
    CompanyType {
        /// `1`: a legal person.
        Corporation = "1",
        /// `2`: a natural person.
        NaturalPerson = "2",
    }
}

codes! {
    /// `Linky.txt` field 4, typ linky.
    RouteType {
        /// `A`: city.
        City = "A",
        /// `B`: city and adjacent.
        CityAndAdjacent = "B",
        /// `M`: international (JDF 1.8).
        International = "M",
        /// `N`: international without national transport.
        InternationalNoNational = "N",
        /// `P`: international with national transport.
        InternationalOrNational = "P",
        /// `V`: regional (vnitrostátní příměstská).
        Regional = "V",
        /// `W`: extra-district (JDF 1.8).
        ExtraDistrict = "W",
        /// `Z`: extra-regional (dálková).
        ExtraRegional = "Z",
        /// `D`: long-distance national.
        LongDistanceNational = "D",
    }
}

codes! {
    /// `Linky.txt` field 5, dopravní prostředek (JDF 1.10 and later).
    TransportMode {
        /// `A`.
        Bus = "A",
        /// `E`.
        Tram = "E",
        /// `L`.
        CableCar = "L",
        /// `M`.
        Metro = "M",
        /// `P`.
        Ferry = "P",
        /// `T`.
        Trolleybus = "T",
    }
}

codes! {
    /// `Caskody.txt` field 5, typ časového kódu.
    NoteType {
        /// `1`: runs.
        Service = "1",
        /// `2`: also runs.
        ServiceAlso = "2",
        /// `3`: runs only.
        ServiceOnly = "3",
        /// `4`: does not run.
        NoService = "4",
        /// `5`: runs in odd weeks.
        OddWeeks = "5",
        /// `6`: runs in even weeks.
        EvenWeeks = "6",
        /// `7`: runs in odd weeks from–to.
        OddWeeksFromTo = "7",
        /// `8`: runs in even weeks from–to.
        EvenWeeksFromTo = "8",
    }
}
