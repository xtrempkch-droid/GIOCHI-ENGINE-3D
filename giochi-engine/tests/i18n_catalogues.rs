//! The shipped catalogues have to stay complete and in sync.
//!
//! `Translator` falls back to English when a key is missing, which is the right behaviour at
//! runtime and the wrong one for a repository: a Portuguese menu with three English rows
//! looks like it works. This test is the check a translator runs before shipping, pointed at
//! `assets/i18n/`.

use giochi_engine::{Catalogue, Language, Translator};

fn catalogues() -> Translator {
    let folder = format!("{}/../assets/i18n", env!("CARGO_MANIFEST_DIR"));
    let mut translator = Translator::new(Language::English);
    let loaded = translator
        .load_folder(&folder)
        .expect("the catalogues load");
    assert_eq!(loaded, Language::ALL.len(), "one file per language");
    translator
}

#[test]
fn every_language_has_every_key() {
    let translator = catalogues();
    for language in Language::ALL {
        let missing = translator.missing_keys(language, Language::English);
        assert!(
            missing.is_empty(),
            "{} is missing {} key(s): {missing:?}",
            language.code(),
            missing.len()
        );
    }
}

#[test]
fn no_language_has_a_key_english_does_not() {
    // A key that exists only in Portuguese is a key the English UI cannot show, which is
    // almost always a typo in the key rather than a deliberate addition.
    let translator = catalogues();
    for language in Language::ALL {
        let extra = translator.missing_keys(Language::English, language);
        assert!(
            extra.is_empty(),
            "{} has key(s) English does not: {extra:?}",
            language.code()
        );
    }
}

#[test]
fn the_placeholders_agree_across_languages() {
    // A translation that drops `{total}` renders a sentence with a hole in it, and no test of
    // the engine would notice: the string is present and non-empty.
    let translator = catalogues();
    let english = translator
        .catalogue(Language::English)
        .expect("English loaded");
    for language in Language::ALL {
        let Some(catalogue) = translator.catalogue(language) else {
            continue;
        };
        for key in english.keys() {
            let expected = placeholders(english.get(key).unwrap_or_default());
            let actual = placeholders(catalogue.get(key).unwrap_or_default());
            assert_eq!(
                expected,
                actual,
                "{}: {key} has placeholders {actual:?}, English has {expected:?}",
                language.code()
            );
        }
    }
}

#[test]
fn no_string_is_empty() {
    let translator = catalogues();
    for language in Language::ALL {
        let catalogue = translator.catalogue(language).expect("loaded");
        for key in catalogue.keys() {
            assert!(
                !catalogue.get(key).unwrap_or_default().is_empty(),
                "{}: {key} is empty",
                language.code()
            );
        }
    }
}

#[test]
fn a_catalogue_is_a_plain_key_value_file() {
    // The format is documented in the module docs; this pins it against the shipped files so
    // a translator that switches to TOML gets a failing test instead of silent skips.
    let path = format!("{}/../assets/i18n/en.txt", env!("CARGO_MANIFEST_DIR"));
    let (catalogue, skipped) = Catalogue::parse(
        Language::English,
        &std::fs::read_to_string(path).expect("read"),
    );
    assert_eq!(skipped, 0);
    assert!(catalogue.len() > 10);
}

/// Every `{name}` in `text`, sorted.
fn placeholders(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(start) = rest.find('{') {
        let after = &rest[start + 1..];
        let Some(end) = after.find('}') else {
            break;
        };
        out.push(after[..end].to_owned());
        rest = &after[end + 1..];
    }
    out.sort();
    out
}
