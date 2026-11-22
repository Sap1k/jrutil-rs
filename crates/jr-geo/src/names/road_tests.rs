use crate::test_data::{identity, matching};

#[test]
fn jdf_r039_tokens_cut_at_dots_commas_hyphens_and_keep_diacritics() {
    let analyzer = matching();
    let cases: &[(&str, &[&str])] = &[
        ("Praha-Dejvice", &["praha", "dejvice"]),
        ("Ústí n.L.,hl.nádr.", &["ústí", "n.", "l.", "hl.", "nádr."]),
        ("Praha,,Florenc", &["praha", "florenc"]),
        ("Brno,(Hlavní nádraží)", &["brno", "(hlavní", "nádraží)"]),
        ("Lhota\u{a0}u Kamenice", &["lhota", "u", "kamenice"]),
        ("...", &[".", ".", "."]),
        (" , ", &[]),
    ];
    for (name, expected) in cases {
        assert_eq!(analyzer.tokens(name), *expected, "{name}");
    }
}

#[test]
fn jdf_r040_expansion_is_the_union_of_lines_and_not_transitive() {
    let analyzer = matching();
    assert_eq!(analyzer.expansion("l."), ["l.", "labem", "lázně"]);
    assert_eq!(analyzer.expansion("labem"), ["l.", "labem"]);
    assert_eq!(analyzer.expansion("praha"), ["praha"]);
    // A multi-word alternative is a phrase of the candidate's tokens.
    let tokens = analyzer.tokens("Lhota,Obecní úřad");
    let terms = analyzer.index_terms(&tokens);
    assert!(terms.contains(&"obú"), "{terms:?}");
    assert!(
        analyzer
            .index_terms(&analyzer.tokens("obú"))
            .contains(&"obecní")
    );
}

#[test]
fn jdf_r042_exact_means_every_candidate_token_is_accounted_for() {
    let analyzer = matching();
    let quality = |query: &str, candidate: &str| {
        analyzer.name_match(&analyzer.tokens(query), &analyzer.tokens(candidate))
    };
    assert!(quality("Ústí n.L.,hl.nádr.", "Ústí nad Labem,Hlavní nádraží").is_exact());
    assert!(quality("Ústí nad Labem,Hlavní nádraží", "Ústí n.L.,hl.nádr.").is_exact());
    assert!(quality("Dolní Jiřetín,rozc.", "Dolní Jiřetín,rozcestí").is_exact());
    let half = quality("Hostivice", "Hostivice-Sadová");
    assert_eq!((half.matched, half.tokens, half.score()), (1, 2, 0.5));
    // Order and repetition are ignored by the score, not by the sequence predicate.
    assert!(quality("Dejvice Praha", "Praha-Dejvice").is_exact());
    let sequence = |left: &str, right: &str| {
        analyzer.same_sequence(&analyzer.tokens(left), &analyzer.tokens(right))
    };
    assert!(sequence("hl.nádr.", "Hlavní nádraží"));
    // Expansions must be equal, not merely meet: `l.` (two lines) is not `labem` (one).
    assert!(!sequence("Ústí n.L.", "Ústí nad Labem"));
    assert!(!sequence("Dejvice Praha", "Praha-Dejvice"));
    assert!(!sequence("Ústí", "Ústí nad Labem"));
    // `labem` and `lázně` share `l.` but are not each other's expansion.
    assert!(!sequence("labem", "lázně"));
}

#[test]
fn jdf_r214_identity_tokens_expand_abbreviations() {
    let analyzer = identity();
    assert_eq!(
        analyzer.normalize("Ústí n.L."),
        analyzer.normalize("Ústí nad Labem")
    );
    assert_eq!(
        analyzer.normalize("hl.nádr."),
        analyzer.normalize("Hlavní nádraží")
    );
    assert_eq!(
        analyzer.normalize("Ústí n.\u{a0}L."),
        analyzer.normalize("Ústí nad Labem")
    );
    assert_eq!(analyzer.normalize("rozc."), "rozcestí");
    // Unmapped tokens lose trailing dots; dot-only tokens vanish.
    assert_eq!(
        analyzer.tokens("Nové Město n.Met. ..."),
        ["nové", "město", "nisou", "metují"]
    );
    assert_eq!(
        analyzer.tokens("Kostelec u Zlína,ZD."),
        ["kostelec", "u", "zlína", "zd"]
    );
    // A canonical form of two words gives two tokens.
    assert_eq!(analyzer.tokens("Lhota,obú"), ["lhota", "obecní", "úřad"]);
}

#[test]
fn jdf_r215_longest_alternative_and_later_lines_override() {
    let analyzer = identity();
    assert_eq!(analyzer.normalize("h."), "horách");
    assert_eq!(analyzer.normalize("hor."), "horní");
    assert_eq!(analyzer.normalize("l."), "lázně");
    assert_eq!(analyzer.normalize("Ústí nad Labem"), "ústí nisou labem");
    assert_eq!(analyzer.normalize("st."), "stanoviště");
    assert_eq!(analyzer.normalize("sv."), "svatého");
    let conflicts: Vec<&str> = analyzer
        .synonym_conflicts()
        .iter()
        .map(|conflict| &*conflict.alternative)
        .collect();
    assert_eq!(conflicts, ["h.", "l."]);
}
