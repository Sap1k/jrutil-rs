//! The registry candidates (JDF-R232–R234): what the build offers for review.

use std::collections::BTreeMap;

use jr_model::Rule;

use super::super::reconcile::{compatible, levenshtein_similarity};
use super::super::{ProvisionalReason, RegistryCandidate, Spelling};
use super::{Draft, Stops};

/// Every spelling of a provisional stop is offered for review: per number ascending, per
/// spelling (display name and locality) in ordinal order, with the stop's precise position and
/// the reason. Only stops a surviving version uses are offered (Q20).
pub const R232: Rule = Rule {
    id: "JDF-R232",
    summary: "Spellings of provisional stops are registry candidates",
};

/// A spelling that joined a registered number although the registry has no row of that
/// identity for it (a suffix, fuzzy or locality match) is offered as an alias of the number.
pub const R233: Rule = Rule {
    id: "JDF-R233",
    summary: "New spellings of registered stops are offered as aliases",
};

/// A provisional spelling suggests registered stops of the same town, compatible locality and
/// similar normalised name, most similar first, then the smaller number (B16 fixed), at most
/// the policy's count.
pub const R234: Rule = Rule {
    id: "JDF-R234",
    summary: "Provisional spellings suggest the most similar registered stops of their town",
};

impl Stops<'_> {
    /// R232–R234 for the drafts `kept` says survive.
    pub fn candidates(&self, kept: &[bool]) -> Vec<RegistryCandidate> {
        let policy = &self.policy.candidates;
        let precise = |draft: &Draft| {
            draft
                .location
                .as_ref()
                .filter(|l| l.origin.is_precise())
                .map(|l| l.coord)
        };
        let mut provisional: Vec<&Draft> = self
            .drafts
            .iter()
            .enumerate()
            .filter(|&(index, draft)| kept[index] && draft.provisional.is_some())
            .map(|(_, draft)| draft)
            .collect();
        provisional.sort_by_key(|draft| draft.number);
        let mut rows = Vec::new();
        for draft in provisional {
            let reason = match draft.provisional {
                Some(ProvisionalReason::Ambiguous) => &policy.reason_ambiguous,
                _ => &policy.reason_new,
            };
            for spelling in draft.spellings.values() {
                rows.push(RegistryCandidate {
                    provisional_number: Some(draft.number),
                    spelling: spelling.clone(),
                    location: precise(draft),
                    reason: reason.clone(),
                    suggestions: self.suggestions(spelling),
                });
            }
        }
        for (number, spelling) in self.aliases.values() {
            let Some(&index) = self.by_number.get(number) else {
                continue;
            };
            if !kept[index] {
                continue;
            }
            rows.push(RegistryCandidate {
                provisional_number: None,
                spelling: spelling.clone(),
                location: precise(&self.drafts[index]),
                reason: policy.reason_alias.clone(),
                suggestions: vec![*number],
            });
        }
        rows
    }

    /// R234.
    fn suggestions(&self, spelling: &Spelling) -> Vec<u64> {
        let policy = &self.policy.candidates;
        let Some(rows) = self.by_town.get(&spelling.town.trim().to_lowercase()) else {
            return Vec::new();
        };
        let flattened = spelling.name(self.analyzer).flattened;
        let mut best: BTreeMap<u64, f64> = BTreeMap::new();
        for &row in rows {
            let entry = &self.registry[row];
            if !compatible(spelling.okres.as_deref(), entry.row.okres.as_deref())
                || !compatible(spelling.country.as_deref(), entry.row.country.as_deref())
            {
                continue;
            }
            let similarity = levenshtein_similarity(&flattened, &entry.name.flattened);
            if similarity >= policy.suggestion_min_similarity {
                let slot = best.entry(entry.row.number).or_insert(similarity);
                *slot = slot.max(similarity);
            }
        }
        let mut ranked: Vec<(u64, f64)> = best.into_iter().collect();
        ranked.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
        ranked
            .into_iter()
            .take(policy.suggestion_max)
            .map(|(number, _)| number)
            .collect()
    }
}
