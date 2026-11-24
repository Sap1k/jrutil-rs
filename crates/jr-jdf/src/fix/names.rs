//! Stop names as matching sees them (JDF-R036–R038) and the comma splits the rules use.

use crate::batch::Stop;

/// A stop's name triple, okres and country while it is being matched.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct WorkStop {
    pub id: u64,
    pub town: Box<str>,
    pub district: Option<Box<str>>,
    pub nearby: Option<Box<str>>,
    pub okres: Option<Box<str>>,
    pub country: Option<Box<str>>,
}

impl WorkStop {
    pub(crate) fn from_source(stop: &Stop) -> WorkStop {
        WorkStop {
            id: stop.id,
            town: stop.town.clone(),
            district: stop.district.clone(),
            nearby: stop.nearby_place.clone(),
            okres: stop.okres.clone(),
            country: stop.country.clone(),
        }
    }

    /// The query of R039: `town,district,nearby`, absent parts empty.
    pub(crate) fn name_string(&self) -> String {
        format!(
            "{},{},{}",
            self.town,
            self.district.as_deref().unwrap_or(""),
            self.nearby.as_deref().unwrap_or("")
        )
    }

    /// Only the town column is filled (R050, R051, R056, R059).
    pub(crate) fn town_only(&self) -> bool {
        self.district.is_none() && self.nearby.is_none()
    }

    pub(crate) fn locality(&self) -> super::select::Locality<'_> {
        super::select::Locality {
            okres: self.okres.as_deref(),
            country: self.country.as_deref(),
        }
    }
}

/// R036: `Praha [AB]` is the town `Praha` in okres `AB` of the home country. The F# pattern is
/// `(.*) \[(..)\]$`: any two characters.
pub(crate) fn okres_from_name(stop: &mut WorkStop, home_country: &str) -> bool {
    let chars: Vec<char> = stop.town.chars().collect();
    let n = chars.len();
    if n < 5 || chars[n - 1] != ']' || chars[n - 4] != '[' || chars[n - 5] != ' ' {
        return false;
    }
    if chars[n - 3] == '\n' || chars[n - 2] == '\n' || chars[..n - 5].contains(&'\n') {
        return false;
    }
    let okres: String = chars[n - 3..n - 1].iter().collect();
    stop.town = chars[..n - 5].iter().collect::<String>().into();
    stop.okres = Some(okres.into());
    stop.country = Some(home_country.into());
    true
}

/// R037. Returns which branch applied (1 = the whole name in the town column, 2 = the nearby
/// place moved to the district).
pub(crate) fn normalise_name(stop: &mut WorkStop) -> Option<u8> {
    if stop.town_only()
        && let Some((town, district, nearby)) = double_comma(&stop.town)
    {
        let (town, district, nearby): (Box<str>, Option<Box<str>>, Option<Box<str>>) = (
            town.into(),
            (!district.is_empty()).then(|| district.into()),
            (!nearby.is_empty()).then(|| nearby.into()),
        );
        stop.town = town;
        stop.district = district;
        stop.nearby = nearby;
        return Some(1);
    }
    district_from_nearby(stop).then_some(2)
}

/// R037's second branch: a name without a district has its nearby place as the district, on
/// the plain and the MHD path alike, so one stop has one structure whatever its batch.
pub(crate) fn district_from_nearby(stop: &mut WorkStop) -> bool {
    if stop.district.is_some() || stop.nearby.is_none() {
        return false;
    }
    stop.district = stop.nearby.take();
    true
}

/// R038: on the non-MHD path a town-only name with one comma is town and district.
pub(crate) fn split_single_comma(stop: &mut WorkStop) -> bool {
    if !stop.town_only() {
        return false;
    }
    let Some((town, district)) = single_comma(&stop.town) else {
        return false;
    };
    let (town, district): (Box<str>, Box<str>) = (town.into(), district.into());
    stop.town = town;
    stop.district = Some(district);
    true
}

/// The first match of `([^,]+),([^,]*),([^,]*)`: the first non-empty part followed by two more
/// parts (text after a third comma is lost, B6).
fn double_comma(text: &str) -> Option<(&str, &str, &str)> {
    let parts: Vec<&str> = text.split(',').collect();
    (0..parts.len().saturating_sub(2))
        .find(|&i| !parts[i].is_empty())
        .map(|i| (parts[i], parts[i + 1], parts[i + 2]))
}

/// The first match of `([^,]+), *([^,]+)`: a non-empty part and the next non-empty part
/// without its leading spaces (one space when it is nothing but spaces).
fn single_comma(text: &str) -> Option<(&str, &str)> {
    let parts: Vec<&str> = text.split(',').collect();
    (0..parts.len().saturating_sub(1))
        .find(|&i| !parts[i].is_empty() && !parts[i + 1].is_empty())
        .map(|i| {
            let next = parts[i + 1];
            let stripped = next.trim_start_matches(' ');
            (
                parts[i],
                if stripped.is_empty() {
                    &next[next.len() - 1..]
                } else {
                    stripped
                },
            )
        })
}

/// The first comma part (`Split(",").[0]`).
pub(crate) fn first_part(text: &str) -> &str {
    text.split(',').next().unwrap_or(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn comma_splits_follow_the_reference_patterns() {
        assert_eq!(
            double_comma("Praha,,Florenc"),
            Some(("Praha", "", "Florenc"))
        );
        assert_eq!(double_comma(",A,B,C"), Some(("A", "B", "C")));
        assert_eq!(double_comma("A,B"), None);
        assert_eq!(single_comma("Hevlín, škola"), Some(("Hevlín", "škola")));
        assert_eq!(single_comma("A,,B"), None);
        assert_eq!(single_comma("A,  "), Some(("A", " ")));
    }
}
