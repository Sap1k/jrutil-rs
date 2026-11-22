//! Merged stops: numbers, spellings, positions, posts and the registry candidates
//! (JDF-R213, R223, R224, R226, R227, R229–R234, R236, R237).

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use jr_geo::names::Analyzer;
use jr_model::{Coord, Diagnostic, Diagnostics, Rule, Severity, geo::distance_m};
use sha2::{Digest, Sha256};

use super::reconcile::{Candidate, Found, Locality, Name, Reconciler};
use super::registry::Row;
use super::{CodeSetId, CodeSets, Policy, Post, PostCandidate, ProvisionalReason, Spelling};
use crate::fixed::{FixedBatch, Location};

mod candidates;

pub use candidates::{R232, R233, R234};

/// Pevnykod numbers are local to a batch. The merge resolves every attribute slot to its fixed
/// code once, so a merged entity carries codes, not numbers, and no merged numbering exists.
pub const R213: Rule = Rule {
    id: "JDF-R213",
    summary: "Attribute slots are resolved to fixed codes; merged entities carry codes",
};

/// Several registered stops share a name: an otherwise ambiguous precisely located spelling
/// takes the candidate whose registry reference is nearest, when every candidate has one
/// (verdict Q3: no distance cap; ties: the smaller number).
pub const R223: Rule = Rule {
    id: "JDF-R223",
    summary: "An ambiguous precise spelling takes the nearest registered reference",
};

/// A spelling that matches no stop is a new stop; one that stays ambiguous is quarantined: one
/// provisional stop per ambiguous identity (normalised name and locality), which later
/// occurrences of that identity join (verdict Q2, fixes B1). Both are diagnosed.
pub const R224: Rule = Rule {
    id: "JDF-R224",
    summary: "Unmatched spellings are new stops; each ambiguous identity is one quarantined stop",
};

/// The merged stop's published spelling is the best-ranked of its spellings: fewer repeated
/// adjacent components, then more components, fewer dots, a longer display name, then the
/// ordinally smaller display name (expanded CIS names over abbreviations, clean over
/// redundant, hierarchical over shortened). Okres and country come from it, or else from the
/// other spelling.
pub const R226: Rule = Rule {
    id: "JDF-R226",
    summary: "A merged stop is published under its best-ranked spelling",
};

/// A merged stop's fixed codes are the union of its spellings' codes, sorted. The six-slot
/// limit of `Zastavky.txt` belongs to a JDF projection, which nothing publishes.
pub const R227: Rule = Rule {
    id: "JDF-R227",
    summary: "A merged stop carries the union of its spellings' fixed codes",
};

/// A spelling matching a registered alias takes the registered number; the first such spelling
/// becomes the stop's spelling until a better one comes (R226).
pub const R229: Rule = Rule {
    id: "JDF-R229",
    summary: "A spelling matching the registry takes the registered number",
};

/// A stop without a registered number gets `base + (h + k) mod span`, `h` the first eight bytes
/// (little-endian) of SHA-256 of the creating spelling's identity and locality, `k` the first
/// probe not taken by a stop already merged. The formula is reviewed through the candidates
/// file, so it is ported as published.
pub const R230: Rule = Rule {
    id: "JDF-R230",
    summary: "Unregistered stops get a hashed provisional number, probing past collisions",
};

/// A merged stop's position: the first one, replaced by a precise one over an estimate, or at
/// equal precision by the position of a spelling that became the stop's published spelling in
/// that batch. Two precise positions farther apart than policy are diagnosed.
pub const R231: Rule = Rule {
    id: "JDF-R231",
    summary: "A stop's position prefers precise, then the preferred spelling's; conflicts diagnosed",
};

/// Post observations attach to the merged stop; one observation per stop. A second, different
/// row for one observation is diagnosed and the first kept (verdict, F# aborted).
pub const R236: Rule = Rule {
    id: "JDF-R236",
    summary: "Post observations follow their stop; a conflicting repeat keeps the first",
};

/// One post number per merged stop: the first batch's `Oznacniky` row wins; a differing repeat
/// is diagnosed.
pub const R237: Rule = Rule {
    id: "JDF-R237",
    summary: "Stop posts are deduplicated by stop and post number, the first row winning",
};

/// One merged stop while the batches are added.
#[derive(Clone, Debug)]
pub(crate) struct Draft {
    pub number: u64,
    pub provisional: Option<ProvisionalReason>,
    pub name: Spelling,
    pub codes: BTreeSet<Box<str>>,
    pub location: Option<Location>,
    pub post_candidates: BTreeMap<Box<str>, PostCandidate>,
    pub posts: BTreeMap<u64, Post>,
    /// Provisional stops: their spellings by spelling key (R232).
    spellings: BTreeMap<String, Spelling>,
}

struct RegistryEntry {
    row: Row,
    name: Name,
}

pub(crate) struct Stops<'a> {
    analyzer: &'a Analyzer,
    policy: &'a Policy,
    reconciler: Reconciler,
    registry: Vec<RegistryEntry>,
    registered: HashSet<u64>,
    references: HashMap<u64, Coord>,
    identities: HashSet<(u64, String)>,
    by_town: HashMap<String, Vec<usize>>,
    pub drafts: Vec<Draft>,
    by_number: HashMap<u64, usize>,
    quarantine: HashMap<String, usize>,
    /// R233: spelling key to (number, spelling), the first number winning.
    aliases: BTreeMap<String, (u64, Spelling)>,
}

impl Spelling {
    fn of(stop: &crate::fixed::FixedStop) -> Spelling {
        Spelling {
            town: stop.town.clone(),
            district: stop.district.clone(),
            nearby_place: stop.nearby_place.clone(),
            okres: stop.okres.clone(),
            country: stop.country.clone(),
        }
    }

    fn of_row(row: &Row) -> Spelling {
        Spelling {
            town: row.town.clone(),
            district: row.district.clone(),
            nearby_place: row.nearby_place.clone(),
            okres: row.okres.clone(),
            country: row.country.clone(),
        }
    }

    /// The present name components joined by `,`.
    pub fn display_name(&self) -> String {
        std::iter::once(Some(&*self.town))
            .chain([self.district.as_deref(), self.nearby_place.as_deref()])
            .flatten()
            .collect::<Vec<_>>()
            .join(",")
    }

    fn locality(&self) -> Locality<'_> {
        Locality {
            okres: self.okres.as_deref(),
            country: self.country.as_deref(),
        }
    }

    fn locality_key(&self) -> String {
        let part = |value: &Option<Box<str>>| value.as_deref().unwrap_or("").trim().to_uppercase();
        format!("{}\u{1f}{}", part(&self.okres), part(&self.country))
    }

    fn spelling_key(&self) -> String {
        format!("{}\u{1f}{}", self.display_name(), self.locality_key())
    }

    fn raw(&self) -> [Option<&str>; 3] {
        [
            Some(&*self.town),
            self.district.as_deref(),
            self.nearby_place.as_deref(),
        ]
    }

    fn name(&self, analyzer: &Analyzer) -> Name {
        Name::new(
            analyzer,
            &self.town,
            self.district.as_deref(),
            self.nearby_place.as_deref(),
        )
    }
}

/// R226's rank, compared as a tuple.
fn rank(spelling: &Spelling) -> (i64, usize, i64, usize) {
    let components: Vec<&str> = spelling
        .raw()
        .into_iter()
        .flatten()
        .filter(|c| !c.trim().is_empty())
        .collect();
    let redundant = components
        .windows(2)
        .filter(|pair| {
            let (a, b) = (pair[0].trim(), pair[1].trim());
            a.chars()
                .flat_map(char::to_uppercase)
                .eq(b.chars().flat_map(char::to_uppercase))
        })
        .count();
    let display = spelling.display_name();
    (
        -(redundant as i64),
        components.len(),
        -(display.matches('.').count() as i64),
        display.encode_utf16().count(),
    )
}

/// R226: whether `candidate` replaces `current` as the published spelling.
fn preferred(candidate: &Spelling, current: &Spelling) -> bool {
    let (a, b) = (rank(candidate), rank(current));
    a > b || (a == b && candidate.display_name() < current.display_name())
}

/// R230.
pub fn provisional_number(
    identity: &str,
    base: u64,
    span: u64,
    taken: impl Fn(u64) -> bool,
) -> Option<u64> {
    let digest = Sha256::digest(identity.as_bytes());
    let mut first = [0u8; 8];
    first.copy_from_slice(&digest[..8]);
    let value = u64::from_le_bytes(first) % span;
    (0..span)
        .map(|k| base + (value + k) % span)
        .find(|&candidate| !taken(candidate))
}

enum Decision {
    Join(Candidate),
    Ambiguous(Vec<Candidate>),
    New,
}

impl<'a> Stops<'a> {
    /// R210: the registry rows become aliases before any batch.
    pub fn new(rows: &[Row], analyzer: &'a Analyzer, policy: &'a Policy) -> Stops<'a> {
        let mut stops = Stops {
            analyzer,
            policy,
            reconciler: Reconciler::new(&policy.stops),
            registry: Vec::with_capacity(rows.len()),
            registered: HashSet::new(),
            references: HashMap::new(),
            identities: HashSet::new(),
            by_town: HashMap::new(),
            drafts: Vec::new(),
            by_number: HashMap::new(),
            quarantine: HashMap::new(),
            aliases: BTreeMap::new(),
        };
        for row in rows {
            let spelling = Spelling::of_row(row);
            let name = spelling.name(analyzer);
            stops.registered.insert(row.number);
            stops.identities.insert((row.number, name.key()));
            if let Some(reference) = row.reference {
                stops.references.entry(row.number).or_insert(reference);
            }
            stops.reconciler.add_alias(
                row.number,
                spelling.raw(),
                &name,
                spelling.locality(),
                row.reference.map(|coord| (coord, true)),
            );
            stops
                .by_town
                .entry(row.town.trim().to_lowercase())
                .or_default()
                .push(stops.registry.len());
            stops.registry.push(RegistryEntry {
                row: row.clone(),
                name,
            });
        }
        stops
    }

    pub fn statistics(&self) -> super::Statistics {
        self.reconciler.statistics
    }

    /// Reconciles one batch's stops in file order; returns each stop's draft.
    pub fn add_batch(&mut self, fixed: &FixedBatch, diagnostics: &mut Diagnostics) -> Vec<usize> {
        let batch = &fixed.batch;
        let codes = batch.attribute_codes();
        let mut map = Vec::with_capacity(fixed.stops.len());
        let mut preferred_here = vec![false; fixed.stops.len()];
        for (position, (stop, source)) in fixed.stops.iter().zip(&batch.stops).enumerate() {
            let spelling = Spelling::of(stop);
            let name = spelling.name(self.analyzer);
            let location = stop
                .location
                .as_ref()
                .map(|l| (l.coord, l.origin.is_precise()));
            let precise = location.and_then(|(coord, precise)| precise.then_some(coord));
            let stop_codes: Vec<Box<str>> =
                codes.resolve(&source.attributes).map(Box::from).collect();
            let subject = || format!("{}/{}", batch.name, source.id);

            let decision = match self.reconciler.find(&name, spelling.locality(), precise) {
                Found::Match(candidate) => Decision::Join(candidate),
                Found::Unmatched(candidates) if candidates.is_empty() => Decision::New,
                Found::Unmatched(candidates) => match self.nearest_registered(&candidates, precise)
                {
                    Some(candidate) => Decision::Join(candidate),
                    None => Decision::Ambiguous(candidates),
                },
            };
            let identity = format!("{}\u{1f}{}", name.key(), spelling.locality_key());
            let draft = match decision {
                Decision::Join(candidate) => {
                    let (draft, won) = self.join(candidate.number, &spelling, stop_codes);
                    preferred_here[position] = won;
                    let canonical = self.drafts[draft].name.display_name();
                    let alias = spelling.display_name();
                    if alias != canonical {
                        diagnostics.push(
                            Diagnostic::new(
                                "jdf_stop_merged",
                                Severity::Info,
                                subject(),
                                format!(
                                    "{alias} merged into {canonical} ({:?}, Levenshtein {:.3}, \
                                     Dice {:.3}, distance {})",
                                    candidate.kind,
                                    candidate.levenshtein,
                                    candidate.dice,
                                    candidate
                                        .distance
                                        .map_or("-".to_owned(), |d| format!("{d:.0} m"))
                                ),
                            )
                            .rule(super::reconcile::R222),
                        );
                    }
                    draft
                }
                Decision::Ambiguous(candidates) => {
                    let listed: Vec<String> = candidates
                        .iter()
                        .map(|c| {
                            let name = self
                                .by_number
                                .get(&c.number)
                                .map(|&d| self.drafts[d].name.display_name())
                                .unwrap_or_else(|| format!("registered {}", c.number));
                            format!("{name} [{:?}; distance={:?}]", c.kind, c.distance)
                        })
                        .collect();
                    diagnostics.push(
                        Diagnostic::new(
                            "jdf_stop_ambiguous",
                            Severity::Warning,
                            subject(),
                            format!(
                                "{} is ambiguous between {}",
                                spelling.display_name(),
                                listed.join(", ")
                            ),
                        )
                        .rule(super::reconcile::R222),
                    );
                    match self.quarantine.get(&identity) {
                        Some(&draft) => {
                            let number = self.drafts[draft].number;
                            let (draft, won) = self.join(number, &spelling, stop_codes);
                            preferred_here[position] = won;
                            draft
                        }
                        None => {
                            let draft = self.create(
                                &identity,
                                ProvisionalReason::Ambiguous,
                                &spelling,
                                stop_codes,
                            );
                            self.quarantine.insert(identity.clone(), draft);
                            diagnostics.push(
                                Diagnostic::new(
                                    "jdf_stop_quarantined",
                                    Severity::Warning,
                                    subject(),
                                    format!(
                                        "{} quarantined as provisional stop {}",
                                        spelling.display_name(),
                                        self.drafts[draft].number
                                    ),
                                )
                                .rule(R224),
                            );
                            draft
                        }
                    }
                }
                Decision::New => {
                    let draft =
                        self.create(&identity, ProvisionalReason::New, &spelling, stop_codes);
                    diagnostics.push(
                        Diagnostic::new(
                            "jdf_stop_provisional",
                            Severity::Info,
                            subject(),
                            format!(
                                "{} is new: provisional stop {}",
                                spelling.display_name(),
                                self.drafts[draft].number
                            ),
                        )
                        .rule(R230),
                    );
                    draft
                }
            };
            let number = self.drafts[draft].number;
            self.reconciler
                .add_alias(number, spelling.raw(), &name, spelling.locality(), location);
            self.record(draft, &spelling, &name);
            map.push(draft);
        }

        // R231, in file order after the batch's stops are decided.
        for (position, stop) in fixed.stops.iter().enumerate() {
            let Some(incoming) = &stop.location else {
                continue;
            };
            let draft = &mut self.drafts[map[position]];
            let replace = match &draft.location {
                None => true,
                Some(current) => {
                    let (a, b) = (incoming.origin.is_precise(), current.origin.is_precise());
                    if a && b
                        && incoming.coord != current.coord
                        && distance_m(incoming.coord, current.coord)
                            > self.policy.stops.location_conflict_m
                    {
                        diagnostics.push(
                            Diagnostic::new(
                                "jdf_stop_location_conflict",
                                Severity::Warning,
                                draft.number.to_string(),
                                format!(
                                    "position {:.6},{:.6} of {} is far from {:.6},{:.6}",
                                    incoming.coord.lat,
                                    incoming.coord.lon,
                                    batch.name,
                                    current.coord.lat,
                                    current.coord.lon
                                ),
                            )
                            .rule(R231),
                        );
                    }
                    (a && !b) || (a == b && preferred_here[position])
                }
            };
            if replace {
                draft.location = Some(incoming.clone());
            }
        }

        // R236.
        for candidate in &fixed.post_candidates {
            let Some(&draft) = map.get(candidate.stop as usize) else {
                continue;
            };
            let draft = &mut self.drafts[draft];
            let incoming = PostCandidate {
                coord: candidate.coord,
                observation: candidate.observation.clone(),
                facts: candidate.facts.clone(),
            };
            match draft.post_candidates.get(&candidate.observation) {
                Some(existing) if *existing != incoming => diagnostics.push(
                    Diagnostic::new(
                        "jdf_post_observation_conflict",
                        Severity::Warning,
                        format!("{}/{}", draft.number, candidate.observation),
                        format!("{} observes it differently: the first is kept", batch.name),
                    )
                    .rule(R236),
                ),
                Some(_) => {}
                None => {
                    draft
                        .post_candidates
                        .insert(candidate.observation.clone(), incoming);
                }
            }
        }
        map
    }

    /// Adds the batch's `Oznacniky` rows (R237); `position` maps a stop number to its position.
    pub fn add_posts(
        &mut self,
        fixed: &FixedBatch,
        map: &[usize],
        position: &HashMap<u64, usize>,
        diagnostics: &mut Diagnostics,
    ) -> usize {
        let mut unknown = 0;
        for row in &fixed.batch.stop_posts {
            let Some(&at) = position.get(&row.stop_id) else {
                unknown += 1;
                continue;
            };
            let draft = &mut self.drafts[map[at]];
            let post = Post {
                post_id: row.post_id,
                name: row.name.clone(),
                description: row.description.clone(),
                post_name: row.post_name.clone(),
            };
            match draft.posts.get(&row.post_id) {
                Some(existing) if *existing != post => diagnostics.push(
                    Diagnostic::new(
                        "jdf_post_attribute_conflict",
                        Severity::Warning,
                        format!("{}/{}", draft.number, row.post_id),
                        format!(
                            "{} describes the post differently: the first is kept",
                            fixed.batch.name
                        ),
                    )
                    .rule(R237),
                ),
                Some(_) => {}
                None => {
                    draft.posts.insert(row.post_id, post);
                }
            }
        }
        unknown
    }

    /// R223.
    fn nearest_registered(
        &self,
        candidates: &[Candidate],
        precise: Option<Coord>,
    ) -> Option<Candidate> {
        let point = precise?;
        let mut ranked = Vec::with_capacity(candidates.len());
        for candidate in candidates {
            let reference = self.references.get(&candidate.number)?;
            ranked.push((distance_m(point, *reference), candidate.number, *candidate));
        }
        ranked
            .into_iter()
            .min_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)))
            .map(|(_, _, candidate)| candidate)
    }

    /// Joins `number` (creating its draft on first use, R229); returns the draft and whether
    /// the spelling became the published one (R226).
    fn join(&mut self, number: u64, spelling: &Spelling, codes: Vec<Box<str>>) -> (usize, bool) {
        let Some(&index) = self.by_number.get(&number) else {
            let index = self.push(number, None, spelling, codes);
            return (index, false);
        };
        let draft = &mut self.drafts[index];
        let won = preferred(spelling, &draft.name);
        let (preferred_name, other) = if won {
            (spelling.clone(), draft.name.clone())
        } else {
            (draft.name.clone(), spelling.clone())
        };
        draft.name = Spelling {
            okres: preferred_name.okres.clone().or(other.okres),
            country: preferred_name.country.clone().or(other.country),
            ..preferred_name
        };
        draft.codes.extend(codes);
        (index, won)
    }

    fn create(
        &mut self,
        identity: &str,
        reason: ProvisionalReason,
        spelling: &Spelling,
        codes: Vec<Box<str>>,
    ) -> usize {
        let stops = &self.policy.stops;
        let number = provisional_number(
            identity,
            stops.provisional_base,
            stops.provisional_span,
            |n| self.by_number.contains_key(&n),
        )
        // The span holds a billion numbers; a national merge has tens of thousands of stops.
        .expect("provisional range exhausted");
        self.push(number, Some(reason), spelling, codes)
    }

    fn push(
        &mut self,
        number: u64,
        provisional: Option<ProvisionalReason>,
        spelling: &Spelling,
        codes: Vec<Box<str>>,
    ) -> usize {
        let index = self.drafts.len();
        self.by_number.insert(number, index);
        self.drafts.push(Draft {
            number,
            provisional,
            name: spelling.clone(),
            codes: codes.into_iter().collect(),
            location: None,
            post_candidates: BTreeMap::new(),
            posts: BTreeMap::new(),
            spellings: BTreeMap::new(),
        });
        index
    }

    /// R232, R233 bookkeeping.
    fn record(&mut self, draft: usize, spelling: &Spelling, name: &Name) {
        let number = self.drafts[draft].number;
        if self.drafts[draft].provisional.is_some() {
            self.drafts[draft]
                .spellings
                .entry(spelling.spelling_key())
                .or_insert_with(|| spelling.clone());
        } else if self.registered.contains(&number)
            && !self.identities.contains(&(number, name.key()))
        {
            self.aliases
                .entry(spelling.spelling_key())
                .or_insert_with(|| (number, spelling.clone()));
        }
    }

    pub fn code_set(draft: &Draft, code_sets: &mut CodeSets) -> CodeSetId {
        code_sets.intern(draft.codes.iter().map(|c| &**c))
    }
}
