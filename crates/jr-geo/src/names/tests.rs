use super::*;

/// The shipped tables, read from the rail coordinates policy at compile time.
fn tables() -> NameTables {
    #[derive(Deserialize)]
    struct Policy {
        names: NameTables,
    }
    let text = include_str!("../../../../policy/rail_coordinates.toml");
    toml::from_str::<Policy>(text)
        .expect("policy/rail_coordinates.toml [names]")
        .names
}

/// The names of the F# `PerformanceEquivalenceTests` fixture that exercise a rule.
const NAMES: &[&str] = &[
    "Praha hl.n.",
    "Praha-Smíchov",
    "Praha-Smíchov sev.n.",
    "Ostrava střed",
    "Głuchołazy",
    "Głuchołazy Fabryka Mebli",
    "Głuchołas.F-ka Mebli",
    "Katowice Zachód",
    "Bärenstein (Annaberg)",
    "Bärenstein",
    "Bad Schandau Bf",
    "Bad Schandau Ost",
    "Zittau Süd",
    "Linz/Donau Hbf",
    "Ærøskøbing",
    "Lysá n.L. zast.",
    "Kolín R12",
    "Benešov u Prahy zastávka",
    "Most město",
    "Ústí nad Labem-Střekov",
    "Košice juh",
    "Hp",
    "St",
    "R5",
    "",
    "  ",
    "Ž",
    "(Pražská)",
    "A (B)",
    "Nové Město n.M.",
];

#[test]
fn czptt_r090_normalize_matches_the_reference() {
    let tables = tables();
    let cases = [
        ("Praha hl.n.", "PRAHA HL N"),
        ("Praha-Smíchov sev.n.", "PRAHA SMICHOV SEV N"),
        ("Głuchołazy Fabryka Mebli", "GLUCHOLAZY FABRYKA MEBLI"),
        ("Ærøskøbing", "AEROSKOBING"),
        ("Zittau Süd", "ZITTAU SUD"),
        ("Straße", "STRASSE"),
        ("Linz/Donau Hbf", "LINZ DONAU HBF"),
        ("Ústí nad Labem-Střekov", "USTI NAD LABEM STREKOV"),
        ("Bärenstein (Annaberg)", "BARENSTEIN ANNABERG"),
        // Already decomposed input loses its marks too.
        ("Kolı\u{301}n", "KOLIN"),
        ("  ", ""),
        ("(Pražská)", "PRAZSKA"),
    ];
    for (name, expected) in cases {
        assert_eq!(normalize(name, &tables), expected, "{name}");
    }
    for name in NAMES {
        let once = normalize(name, &tables);
        assert_eq!(normalize(&once, &tables), once, "idempotent on {name}");
    }
}

#[test]
fn czptt_r091_railway_core_strips_suffixes_and_trailing_qualifier() {
    let tables = tables();
    let cases = [
        ("Bärenstein (Annaberg)", "BARENSTEIN"),
        ("Unterretzbach z", "UNTERRETZBACH"),
        ("Bad Schandau Bf", "BAD SCHANDAU"),
        ("Kolín R12", "KOLIN"),
        ("Benešov u Prahy zastávka", "BENESOV U PRAHY"),
        ("Lysá n.L. zast.", "LYSA N L"),
        ("A (B)", "A"),
        // Nothing left after stripping: the full form.
        ("Hp", "HP"),
        ("R5", "R5"),
        ("(Pražská)", "PRAZSKA"),
        // Only a trailing qualifier comes off.
        ("Brno (hl.n.) jih", "BRNO HL N JIH"),
    ];
    for (name, expected) in cases {
        assert_eq!(railway_core(name, &tables), expected, "{name}");
    }
}

#[test]
fn czptt_r092_similarity_counts_characters_and_the_bounded_form_agrees() {
    let tables = tables();
    let prepare = |name: &str| Prepared::new(name, &tables);
    // 𝔸 is one character but two UTF-16 units: one edit in three characters.
    let similarity_of = |a: &str, b: &str| similarity(&prepare(a), &prepare(b));
    assert!((similarity_of("A\u{1D538}B", "AB") - 2.0 / 3.0).abs() < 1e-12);
    assert_eq!(similarity_of("Žilina", "Zilina"), 1.0);
    assert_eq!(similarity_of("", "Zilina"), 0.0);
    // The compact core: spacing differences cost nothing.
    assert_eq!(similarity_of("Nové Město", "Novéměsto"), 1.0);
    assert!(similarity_of("Głuchołas.F-ka Mebli", "Głuchołazy Fabryka Mebli") >= 0.58);

    // The F# equivalence of the prepared, bounded scan with the plain similarity.
    let prepared: Vec<Prepared> = NAMES.iter().map(|name| prepare(name)).collect();
    for left in &prepared {
        for right in &prepared {
            let expected = similarity(left, right);
            for minimum in [0.0, 0.58, 0.9] {
                assert_eq!(
                    similarity_at_least(left, right, minimum),
                    (expected >= minimum).then_some(expected),
                    "{} / {} at {minimum}",
                    left.full(),
                    right.full()
                );
            }
        }
    }
}

#[test]
fn czptt_r093_protected_qualifiers_must_survive_a_fuzzy_match() {
    let tables = tables();
    let compatible = |expected: &str, candidate: &str| {
        qualifiers_compatible(
            &Prepared::new(expected, &tables),
            &Prepared::new(candidate, &tables),
            &tables,
        )
    };
    assert!(!compatible("Bad Schandau Ost", "Bad Schandau"));
    assert!(compatible("Bad Schandau Ost", "Bad Schandau Ostbahnhof"));
    assert!(compatible("Most město", "Most mesto"));
    assert!(!compatible("Ostrava střed", "Ostrava hl.n."));
    assert!(compatible("Bad Schandau", "Bad Schandau Ost"));
}
