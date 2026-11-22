use super::*;
use crate::test_data::{matching, town_synonyms};

fn entries(index: &StopIndex, name: &str) -> Vec<(u32, u32, u32)> {
    index
        .candidates(name)
        .into_iter()
        .map(|candidate| {
            (
                candidate.entry,
                candidate.quality.matched,
                candidate.quality.tokens,
            )
        })
        .collect()
}

/// `StopMatcherTests.Basic stop search`.
#[test]
fn jdf_r041_every_query_token_is_required() {
    let index = StopIndex::new(
        matching(),
        &["Hostivice", "Hostivice-Sadová", "Praha-Dejvice"],
    );
    assert_eq!(entries(&index, "Hostivice"), [(0, 1, 1), (1, 1, 2)]);
    assert_eq!(index.exact("Hostivice"), [0]);
    assert_eq!(entries(&index, "praha dejvice"), [(2, 2, 2)]);
    assert!(index.candidates("Praha Smíchov").is_empty());
    assert!(index.candidates(" , ").is_empty());
}

#[test]
fn jdf_r041_query_tokens_are_found_through_the_candidates_synonyms() {
    let index = StopIndex::new(
        matching(),
        &[
            "Ústí nad Labem,Hlavní nádraží",
            "Ústí n.L.,hl.nádr.",
            "Bílina,lázně",
            "Lhota,obú",
            "Obecní úřad",
        ],
    );
    // Both spellings find each other exactly.
    assert_eq!(index.exact("Ústí n.L.,hl.nádr."), [0, 1]);
    assert_eq!(index.exact("Ústí nad Labem,Hlavní nádraží"), [0, 1]);
    // The query token itself must be among the candidate's words: `labem` is not a word of
    // `lázně` (no transitivity through `l.`), while `l.` is a word of both.
    assert!(index.candidates("Bílina,labem").is_empty());
    assert_eq!(index.exact("Bílina,l."), [2]);
    // A phrase alternative: `obú` finds `Obecní úřad` and `obecní úřad` finds `obú`.
    assert_eq!(index.exact("obú"), [4]);
    assert_eq!(entries(&index, "obecní úřad"), [(3, 1, 2), (4, 2, 2)]);
}

#[test]
fn jdf_r054_town_names_resolve_before_the_municipality_lookup() {
    let synonyms = town_synonyms();
    let municipalities = StopIndex::new(matching(), &["Malá Úpa", "Horní Maršov", "Úpa"]);
    assert_eq!(
        municipalities.exact(synonyms.resolve("Dolní Malá Úpa")),
        [0]
    );
    assert!(municipalities.exact("Dolní Malá Úpa").is_empty());
    assert_eq!(synonyms.resolve("Horní Maršov"), "Horní Maršov");
}
