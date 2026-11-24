//! Stop reconciliation across batches (JDF-R216–R222, R225): the incremental, indexed matcher.
//!
//! Every spelling seen so far (registry rows first, then each batch's stops in batch-name order)
//! is an alias of a stop number. An incoming spelling is compared with the aliases its name or
//! position retrieves; the strongest kind of match decides. Later spellings see earlier ones, so
//! suffix and fuzzy matches chain (verdict Q1: ported in batch-name order).

use std::collections::HashMap;

use jr_geo::names::Analyzer;
use jr_model::{Coord, Rule, geo::distance_m};

use super::policy::StopsPolicy;

/// Same names in different okresy or countries are different stops: an alias whose okres or
/// country differs (both present, compared ignoring case) is never a candidate.
pub const R216: Rule = Rule {
    id: "JDF-R216",
    summary: "Spellings with different okres or country never match",
};

/// Candidates are the aliases whose identity equals the incoming one or one of its proper
/// suffixes, whose proper suffixes equal it, and, for a precisely located stop, the aliases in
/// the adjacent cells of a grid as fine as the match distance. Retrieval is symmetric: a
/// one-component spelling finds a longer one ending in it, as the longer finds the shorter
/// (B3 fixed, `docs/roadmap.md`).
pub const R217: Rule = Rule {
    id: "JDF-R217",
    summary: "Candidates come from the identity and suffix indexes and the adjacent grid cells",
};

/// Stop identity in CIS is the spelling: equal normalised components (JDF-R214) match.
pub const R218: Rule = Rule {
    id: "JDF-R218",
    summary: "Equal normalised name components are an exact match",
};

/// A hierarchical CIS name and its shortened urban form (`Horní Jiřetín,Dolní Jiřetín,rozcestí`
/// and `Dolní Jiřetín,rozc.`) are one stop: one component list being a proper suffix of the
/// other is a suffix match.
pub const R219: Rule = Rule {
    id: "JDF-R219",
    summary: "One component list ending the other is a suffix match",
};

/// Same-named stops farther apart than the match distance are different stops: a precisely
/// located spelling matches a stop by exact or suffix name only within the match distance of
/// the nearest precise position the stop's aliases have, whichever alias the name retrieved (B2
/// fixed: an alias without a precise position no longer carries the match to any distance).
/// Without precise positions on both sides an exact or suffix match holds at any distance. A
/// registry row's position is its reference.
pub const R220: Rule = Rule {
    id: "JDF-R220",
    summary: "Exact and suffix matches need the match distance to the stop's nearest precise position",
};

/// Minor spelling differences (`stare`/`staré`) of two precisely located stops within the
/// match distance: at least two tokens each, Levenshtein similarity and token Dice over policy.
pub const R221: Rule = Rule {
    id: "JDF-R221",
    summary: "Close precise stops with nearly equal names are a fuzzy match",
};

/// Per stop number the best alias (exact, suffix, fuzzy; then nearer) counts; across numbers
/// only the strongest kind present. One number left is a match; several are ambiguous (and
/// diagnosed where the ambiguity is settled).
pub const R222: Rule = Rule {
    id: "JDF-R222",
    summary: "The strongest kind decides; several stops of that kind are ambiguous",
};

/// After a stop is decided, its spelling joins the number as an alias (one per number and raw
/// spelling: a repeat only fills a missing okres, country or a better position), so later
/// batches match it too.
pub const R225: Rule = Rule {
    id: "JDF-R225",
    summary: "Every decided spelling becomes an alias of its stop for later batches",
};

/// One name, normalised for identity (JDF-R214 through `jr-geo`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Name {
    pub components: Vec<String>,
    pub tokens: Vec<String>,
    pub flattened: String,
}

impl Name {
    pub fn new(
        analyzer: &Analyzer,
        town: &str,
        district: Option<&str>,
        nearby: Option<&str>,
    ) -> Name {
        let components: Vec<String> = std::iter::once(Some(town))
            .chain([district, nearby])
            .flatten()
            .map(|component| analyzer.tokens(component).join(" "))
            .filter(|component| !component.is_empty())
            .collect();
        let tokens: Vec<String> = components
            .iter()
            .flat_map(|component| component.split(' ').map(str::to_owned))
            .collect();
        let flattened = tokens.join(" ");
        Name {
            components,
            tokens,
            flattened,
        }
    }

    /// The exact-match identity.
    pub fn key(&self) -> String {
        key(&self.components)
    }
}

fn key(components: &[String]) -> String {
    components.join("\u{1f}")
}

/// The kinds of match, strongest first.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum MatchKind {
    Exact,
    Suffix,
    Fuzzy,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Candidate {
    pub number: u64,
    pub kind: MatchKind,
    pub distance: Option<f64>,
    pub levenshtein: f64,
    pub dice: f64,
}

pub(crate) enum Found {
    Match(Candidate),
    /// Several stops, or none (empty).
    Unmatched(Vec<Candidate>),
}

/// An incoming or stored spelling's locality.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Locality<'a> {
    pub okres: Option<&'a str>,
    pub country: Option<&'a str>,
}

struct Alias {
    number: u64,
    okres: Option<Box<str>>,
    country: Option<Box<str>>,
    name: Name,
    /// The position and whether it is precise.
    location: Option<(Coord, bool)>,
    cell: Option<(i64, i64)>,
}

/// Counts of decided reconciliations (R235).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Statistics {
    pub exact: u64,
    pub suffix: u64,
    pub fuzzy: u64,
    pub ambiguous: u64,
}

pub(crate) struct Reconciler {
    aliases: Vec<Alias>,
    by_key: HashMap<(u64, String), usize>,
    exact: HashMap<String, Vec<usize>>,
    suffix: HashMap<String, Vec<usize>>,
    grid: HashMap<(i64, i64), Vec<usize>>,
    /// R220: per stop number, the precise positions of its aliases.
    precise: HashMap<u64, Vec<Coord>>,
    cell_lat: f64,
    cell_lon: f64,
    policy: StopsPolicy,
    pub statistics: Statistics,
}

/// Metres per degree of latitude on the sphere of `jr_model::geo`.
const METRES_PER_DEGREE: f64 = 6_371_008.8 * std::f64::consts::PI / 180.0;

impl Reconciler {
    pub fn new(policy: &StopsPolicy) -> Reconciler {
        let cell_lat = policy.match_distance_m / METRES_PER_DEGREE;
        let cell_lon = cell_lat / policy.grid_max_latitude.to_radians().cos();
        Reconciler {
            aliases: Vec::new(),
            by_key: HashMap::new(),
            exact: HashMap::new(),
            suffix: HashMap::new(),
            grid: HashMap::new(),
            precise: HashMap::new(),
            cell_lat,
            cell_lon,
            policy: policy.clone(),
            statistics: Statistics::default(),
        }
    }

    fn cell(&self, coord: Coord) -> (i64, i64) {
        (
            (coord.lat / self.cell_lat).floor() as i64,
            (coord.lon / self.cell_lon).floor() as i64,
        )
    }

    /// R216–R222 for one incoming spelling; `precise` is its position if precise.
    pub fn find(&mut self, name: &Name, locality: Locality<'_>, precise: Option<Coord>) -> Found {
        let mut ids: Vec<usize> = Vec::new();
        let full = name.key();
        ids.extend(self.exact.get(&full).into_iter().flatten());
        ids.extend(self.suffix.get(&full).into_iter().flatten());
        for offset in 1..name.components.len() {
            ids.extend(
                self.exact
                    .get(&key(&name.components[offset..]))
                    .into_iter()
                    .flatten(),
            );
        }
        if let Some(coord) = precise {
            let (y, x) = self.cell(coord);
            for dy in -1..=1 {
                for dx in -1..=1 {
                    ids.extend(self.grid.get(&(y + dy, x + dx)).into_iter().flatten());
                }
            }
        }
        ids.sort_unstable();
        ids.dedup();

        // Best alias per number, in ascending alias order (stable).
        let mut best: Vec<Candidate> = Vec::new();
        for id in ids {
            let Some(candidate) = self.compare(&self.aliases[id], name, locality, precise) else {
                continue;
            };
            let order = |c: &Candidate| (c.kind, c.distance.unwrap_or(0.0));
            match best.iter_mut().find(|b| b.number == candidate.number) {
                Some(current) => {
                    let (a, b) = (order(&candidate), order(current));
                    if a.0 < b.0 || (a.0 == b.0 && a.1 < b.1) {
                        *current = candidate;
                    }
                }
                None => best.push(candidate),
            }
        }
        // R220: a precise spelling is measured against the stop, not only the alias it found.
        if let Some(coord) = precise {
            best.retain_mut(|candidate| {
                if candidate.kind == MatchKind::Fuzzy {
                    return true;
                }
                let Some(nearest) = self.precise.get(&candidate.number).and_then(|positions| {
                    positions
                        .iter()
                        .map(|&at| distance_m(coord, at))
                        .min_by(f64::total_cmp)
                }) else {
                    return true;
                };
                candidate.distance = Some(nearest);
                nearest <= self.policy.match_distance_m
            });
        }
        let Some(strongest) = best.iter().map(|c| c.kind).min() else {
            return Found::Unmatched(Vec::new());
        };
        best.retain(|c| c.kind == strongest);
        if let [only] = best[..] {
            match only.kind {
                MatchKind::Exact => self.statistics.exact += 1,
                MatchKind::Suffix => self.statistics.suffix += 1,
                MatchKind::Fuzzy => self.statistics.fuzzy += 1,
            }
            Found::Match(only)
        } else {
            self.statistics.ambiguous += 1;
            Found::Unmatched(best)
        }
    }

    fn compare(
        &self,
        alias: &Alias,
        name: &Name,
        locality: Locality<'_>,
        precise: Option<Coord>,
    ) -> Option<Candidate> {
        // R216.
        if !compatible(locality.okres, alias.okres.as_deref())
            || !compatible(locality.country, alias.country.as_deref())
        {
            return None;
        }
        let alias_precise = alias
            .location
            .and_then(|(coord, precise)| precise.then_some(coord));
        let distance = precise.zip(alias_precise).map(|(a, b)| distance_m(a, b));
        let limit = self.policy.match_distance_m;
        let kind = if name.components == alias.name.components {
            Some(MatchKind::Exact)
        } else if is_suffix(&name.components, &alias.name.components)
            || is_suffix(&alias.name.components, &name.components)
        {
            Some(MatchKind::Suffix)
        } else {
            None
        };
        match kind {
            // R220.
            Some(_) if distance.is_some_and(|d| d > limit) => None,
            Some(kind) => Some(Candidate {
                number: alias.number,
                kind,
                distance,
                levenshtein: levenshtein_similarity(&name.flattened, &alias.name.flattened),
                dice: token_dice(&name.tokens, &alias.name.tokens),
            }),
            // R221.
            None => {
                let d = distance.filter(|&d| d <= limit)?;
                let minimum = self.policy.fuzzy_min_tokens;
                if name.tokens.len() < minimum || alias.name.tokens.len() < minimum {
                    return None;
                }
                let levenshtein = levenshtein_similarity(&name.flattened, &alias.name.flattened);
                let dice = token_dice(&name.tokens, &alias.name.tokens);
                (levenshtein >= self.policy.fuzzy_min_levenshtein
                    && dice >= self.policy.fuzzy_min_token_dice)
                    .then_some(Candidate {
                        number: alias.number,
                        kind: MatchKind::Fuzzy,
                        distance: Some(d),
                        levenshtein,
                        dice,
                    })
            }
        }
    }

    /// R225: the spelling (as written: town, district, nearby place) becomes or updates an alias
    /// of `number`.
    pub fn add_alias(
        &mut self,
        number: u64,
        raw: [Option<&str>; 3],
        name: &Name,
        locality: Locality<'_>,
        location: Option<(Coord, bool)>,
    ) {
        let raw_key = raw
            .iter()
            .map(|part| part.unwrap_or("").trim().to_lowercase())
            .collect::<Vec<_>>()
            .join("\u{1f}");
        if let Some(&id) = self.by_key.get(&(number, raw_key.clone())) {
            let existing = &self.aliases[id];
            let updated = match (existing.location, location) {
                (None, Some(incoming)) => Some(incoming),
                (Some((_, false)), Some(incoming @ (_, true))) => Some(incoming),
                (current, _) => current,
            };
            let cell = updated.and_then(|(coord, precise)| precise.then(|| self.cell(coord)));
            if cell != existing.cell {
                if let Some(old) = existing.cell
                    && let Some(ids) = self.grid.get_mut(&old)
                {
                    ids.retain(|&other| other != id);
                }
                if let Some(new) = cell {
                    insert(&mut self.grid, new, id);
                }
            }
            if let Some((coord, true)) = updated
                && existing.location != updated
            {
                self.precise.entry(number).or_default().push(coord);
            }
            let alias = &mut self.aliases[id];
            if alias.okres.is_none() {
                alias.okres = locality.okres.map(Box::from);
            }
            if alias.country.is_none() {
                alias.country = locality.country.map(Box::from);
            }
            alias.location = updated;
            alias.cell = cell;
            return;
        }
        let id = self.aliases.len();
        let cell = location.and_then(|(coord, precise)| precise.then(|| self.cell(coord)));
        insert(&mut self.exact, name.key(), id);
        for offset in 1..name.components.len() {
            insert(&mut self.suffix, key(&name.components[offset..]), id);
        }
        if let Some((coord, true)) = location {
            self.precise.entry(number).or_default().push(coord);
        }
        if let Some(cell) = cell {
            insert(&mut self.grid, cell, id);
        }
        self.by_key.insert((number, raw_key), id);
        self.aliases.push(Alias {
            number,
            okres: locality.okres.map(Box::from),
            country: locality.country.map(Box::from),
            name: name.clone(),
            location,
            cell,
        });
    }
}

fn insert<K: std::hash::Hash + Eq>(index: &mut HashMap<K, Vec<usize>>, key: K, id: usize) {
    let ids = index.entry(key).or_default();
    if !ids.contains(&id) {
        ids.push(id);
    }
}

/// R216: absent on either side is compatible; otherwise equal ignoring case.
pub(crate) fn compatible(a: Option<&str>, b: Option<&str>) -> bool {
    match (a, b) {
        (Some(a), Some(b)) => a
            .chars()
            .flat_map(char::to_uppercase)
            .eq(b.chars().flat_map(char::to_uppercase)),
        _ => true,
    }
}

fn is_suffix(shorter: &[String], longer: &[String]) -> bool {
    shorter.len() < longer.len() && longer[longer.len() - shorter.len()..] == *shorter
}

fn token_dice(a: &[String], b: &[String]) -> f64 {
    let a: std::collections::BTreeSet<&str> = a.iter().map(String::as_str).collect();
    let b: std::collections::BTreeSet<&str> = b.iter().map(String::as_str).collect();
    if a.is_empty() && b.is_empty() {
        return 1.0;
    }
    2.0 * a.intersection(&b).count() as f64 / (a.len() + b.len()) as f64
}

pub(crate) use jr_geo::names::levenshtein_similarity;
