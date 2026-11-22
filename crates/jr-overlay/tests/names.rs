//! The overlay's name rules (OVL-R032–R035) and route labels and modes (R036–R038).

mod support;

use jr_model::timetable::Mode;
use jr_overlay::prepare::{Names, labels_compatible};
use jr_overlay::profile::BaseProfile;

fn names() -> Names {
    Names::new(&support::synonym_lines())
}

#[test]
fn ovl_r033_r034_czech_abbreviations_match_their_verbose_forms() {
    let names = names();
    let same = |a: &str, b: &str| names.rank(a, b).is_some();
    assert!(same("Ochoz u Brna, obecní úřad", "Ochoz u Brna,ObÚ"));
    assert!(same(
        "Bílovice nad Svitavou, železniční stanice",
        "Bílovice n.Svit.,žel.st."
    ));
    assert!(same("Moravský Krumlov, náměstí", "Moravský Krumlov,nám."));
    assert!(same(
        "Svatobořice-Mistřín, restaurace",
        "Svatobořice-Mistřín,rest."
    ));
    assert!(!same(
        "Ochoz u Brna, obecní úřad",
        "Ochoz u Brna, železniční stanice"
    ));
    assert!(same("Brno, hlavní nádraží", "Brno,,hl.nádr."));
    assert!(same(
        "Branišovice, státní silnice",
        "Branišovice,st.silnice"
    ));
    assert!(same(
        "Břeclav, Charvátská Nová Ves, Lednická",
        "Břeclav,Charvátská N.Ves,Lednická"
    ));
    assert!(same("Blansko, autobusové stanoviště", "Blansko,aut.st."));
    // R034: acronyms expand to several words.
    assert!(same("Starovice, zemědělské družstvo", "Starovice,ZD"));
    assert!(same("Jamolice, mateřská škola", "Jamolice,MŠ"));
    // A junction's distance is not part of its name.
    assert!(same("Miroslav, rozcestí", "Miroslav,rozc.4.0"));
    assert!(same(
        "Slavkov u Brna, rozcestí k žel. st.",
        "Slavkov u Brna,rozc.k žel.st.0.5"
    ));
    assert!(same("Blansko, 9. května", "Blansko,9.května"));
    // Leading locality segments may be left out by one side; other words may not.
    assert!(same("ZOO", "Brno,Bystrc,ZOO"));
    assert_eq!(names.rank("ZOO", "Brno,Bystrc,ZOO"), Some(2));
    assert_eq!(names.rank("Hevlín, škola", "Hevlín,,škola"), Some(0));
    assert!(!same("Jevišovice, policie", "Jevišovice,odd.policie"));
    assert!(!same(
        "Bílovice nad Svitavou, náměstí",
        "Bílovice n.Svit.,u kapličky"
    ));
    assert!(!same("Poliklinika Budějovická", "Budějovická"));
}

#[test]
fn ovl_r032_key_words_are_lower_case_letters_and_numbers() {
    let names = names();
    assert_eq!(
        names
            .key_words("Kounice, č.p. 129 (virtuální zastávka NM)")
            .join("-"),
        "kounice-č-p-129-virtuální-zastávka-nm"
    );
    assert_eq!(names.stable_key("Praha,Letňany"), "praha\u{1f}letňany");
    assert_eq!(names.stable_key("Praha,Letňany [?]"), "praha\u{1f}letňany");
}

#[test]
fn ovl_r035_headsigns_resolve_full_names_conservatively() {
    let names = names();
    let resolve = |raw: &str, destinations: &[&str]| names.resolve_headsign(raw, destinations).0;
    assert_eq!(
        resolve("Černý Most", &["Praha,Černý Most"]).as_deref(),
        Some("Praha,Černý Most")
    );
    assert_eq!(
        resolve("Praha,Smíchovské nádraží", &["Smíchovské nádraží"]).as_deref(),
        Some("Smíchovské nádraží")
    );
    assert_eq!(
        resolve(
            "Dobřichovice,žel. st.",
            &["Dobřichovice,železniční stanice"]
        )
        .as_deref(),
        Some("Dobřichovice,železniční stanice")
    );
    assert_eq!(
        resolve("Libuš", &["Sídliště Libuš", "Libuš"]).as_deref(),
        Some("Libuš")
    );
    assert_eq!(
        resolve(
            "Zahradní Město",
            &["Nádraží Zahradní Město", "Zahradní Město"]
        )
        .as_deref(),
        Some("Zahradní Město")
    );
    assert_eq!(
        resolve("Centrum", &["Obec A,Centrum", "Obec B,Centrum"]).as_deref(),
        Some("Obec B,Centrum")
    );
    assert_eq!(resolve("Budějovická", &["Poliklinika Budějovická"]), None);
    assert_eq!(resolve("Letiště / Airport ✈", &["Praha,Letiště"]), None);
    assert_eq!(resolve("Centrum přes Nádraží", &["Centrum"]), None);
}

#[test]
fn ovl_r036_r037_r038_modes_and_labels() {
    let base = BaseProfile::road().unwrap();
    assert!(base.compatible(Some(Mode::Trolleybus), Some(Mode::Bus)));
    assert!(!base.compatible(Some(Mode::Tram), Some(Mode::Bus)));
    assert!(!base.compatible(None, None));
    assert_eq!(base.mode_of_route_type(Some(202)), Some(Mode::Bus));
    assert_eq!(base.mode_of_route_type(Some(800)), Some(Mode::Trolleybus));
    assert_eq!(base.mode_of_route_type(Some(2)), Some(Mode::Rail));
    assert_eq!(base.mode_of_route_type(Some(7)), None);
    assert!(labels_compatible("H4", "H"));
    assert!(labels_compatible("h-4", "H4"));
    assert!(!labels_compatible("25", "2"));
    assert!(!labels_compatible("", "2"));
}
