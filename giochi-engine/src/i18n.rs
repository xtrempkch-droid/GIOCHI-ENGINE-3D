//! Translations: one key, several languages, one file per language.
//!
//! # Why a hand-written format and not JSON
//!
//! `giochi-engine` has no `serde` — that decision is deliberate (ADR-073 keeps the crate's
//! dependency list to what the engine itself needs, and a game that wants JSON already has
//! it through `giochi-engine-3d`). A translation catalogue is a list of `key = value` lines,
//! which is what the format below is; a JSON parser for it would be three times the code
//! and would need a dependency to justify it.
//!
//! # The format
//!
//! ```text
//! # a comment, ignored
//! menu.start = Start
//! menu.quit = Quit
//! hud.lap = Lap {current} of {total}
//! ```
//!
//! - `#` at the start of a line is a comment; blank lines are ignored.
//! - Everything after the first `=` is the value, so a value may contain `=`.
//! - Leading and trailing spaces around the key and the value are trimmed.
//! - `\n` in a value is a newline, and `\\` is a backslash. Nothing else is an escape, so a
//!   value with `{`, `%` or `$` needs no escaping — which matters, because those are the
//!   characters a translation is most likely to contain.
//!
//! # Placeholders and the one-pass rule
//!
//! `{name}` is replaced by a value at formatting time. Substitution is **one pass**: a
//! translated string that happens to contain `{other}` is not then substituted again. The
//! alternative — repeat until nothing changes — turns a translation into a template engine
//! and lets a bad string recurse forever.
//!
//! # Falling back
//!
//! A missing key falls back to the **fallback language**, then to the key itself. Showing
//! `menu.quit` is ugly; showing an empty label is a bug the player cannot report, because
//! there is nothing to tap.
//!
//! # Right-to-left
//!
//! [`Language::is_rtl`] exists for the one thing a 2D engine has to do differently: align a
//! label to the right and mirror the order of a row. The engine does not lay text out, so it
//! cannot do more than answer the question; the game's UI code decides.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// A language the engine can display.
///
/// The set is small on purpose: the project ships Portuguese, English and Spanish strings,
/// and a language with no catalogue would be a menu entry that changes nothing. Adding one
/// is one variant, one code and one file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub enum Language {
    /// English.
    #[default]
    English,
    /// Portuguese (European).
    Portuguese,
    /// Spanish.
    Spanish,
}

impl Language {
    /// Every language the engine knows.
    pub const ALL: [Language; 3] = [Language::English, Language::Portuguese, Language::Spanish];

    /// The two-letter code used in file names: `en`, `pt`, `es`.
    pub const fn code(self) -> &'static str {
        match self {
            Self::English => "en",
            Self::Portuguese => "pt",
            Self::Spanish => "es",
        }
    }

    /// The language's own name, which is what a language picker shows.
    pub const fn native_name(self) -> &'static str {
        match self {
            Self::English => "English",
            Self::Portuguese => "Português",
            Self::Spanish => "Español",
        }
    }

    /// The language for a code, accepting a full locale (`pt_BR`, `pt-BR`, `pt`).
    ///
    /// Only the primary subtag is looked at: the project has no regional variants, and
    /// mapping `pt_BR` to `pt` is what a player expects rather than English.
    pub fn from_code(code: &str) -> Option<Self> {
        let primary = code
            .split(['_', '-'])
            .next()
            .unwrap_or(code)
            .to_ascii_lowercase();
        Self::ALL
            .into_iter()
            .find(|language| language.code() == primary)
    }

    /// Whether the language is written right to left.
    pub const fn is_rtl(self) -> bool {
        // None of the three is, and saying so in one place is better than scattering
        // `false` through the UI code. When Arabic arrives, this is the line to change.
        false
    }

    /// The file name a catalogue for this language uses inside a folder.
    pub fn file_name(self) -> String {
        format!("{}.txt", self.code())
    }
}

/// One language's strings.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Catalogue {
    language: Language,
    entries: BTreeMap<String, String>,
}

impl Catalogue {
    /// An empty catalogue for `language`.
    pub fn new(language: Language) -> Self {
        Self {
            language,
            entries: BTreeMap::new(),
        }
    }

    /// The language this catalogue holds.
    pub const fn language(&self) -> Language {
        self.language
    }

    /// How many strings it holds.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether it holds nothing.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The raw string for `key`, without substitution.
    pub fn get(&self, key: &str) -> Option<&str> {
        self.entries.get(key).map(String::as_str)
    }

    /// Adds or replaces a string. Returns the previous one, if any.
    pub fn insert(&mut self, key: impl Into<String>, value: impl Into<String>) -> Option<String> {
        self.entries.insert(key.into(), value.into())
    }

    /// Every key, sorted: `BTreeMap` order, which is what a diff of two catalogues wants.
    pub fn keys(&self) -> impl Iterator<Item = &str> {
        self.entries.keys().map(String::as_str)
    }

    /// Parses the `key = value` format.
    ///
    /// A line without `=` is **skipped and logged**, not an error: a translator saving a
    /// file with a stray paragraph should not blank the whole menu. The count of skipped
    /// lines is returned so a loader can report it.
    pub fn parse(language: Language, text: &str) -> (Self, usize) {
        let mut catalogue = Self::new(language);
        let mut skipped = 0;
        for (number, raw) in text.lines().enumerate() {
            let line = raw.trim();
            if line.is_empty() || line.starts_with('#') || line.starts_with("//") {
                continue;
            }
            let Some((key, value)) = line.split_once('=') else {
                log::warn!(
                    "{} line {}: no '=' in {:?}; skipped",
                    language.file_name(),
                    number + 1,
                    line
                );
                skipped += 1;
                continue;
            };
            let key = key.trim();
            if key.is_empty() {
                skipped += 1;
                continue;
            }
            catalogue
                .entries
                .insert(key.to_owned(), unescape(value.trim()));
        }
        (catalogue, skipped)
    }

    /// Reads a catalogue from a file.
    ///
    /// # Errors
    ///
    /// The filesystem's error, with the path in the message.
    pub fn load(language: Language, path: impl AsRef<Path>) -> Result<Self, String> {
        let path = path.as_ref();
        let text = std::fs::read_to_string(path)
            .map_err(|error| format!("cannot read {}: {error}", path.display()))?;
        let (catalogue, skipped) = Self::parse(language, &text);
        if skipped > 0 {
            log::warn!("{}: {skipped} line(s) skipped", path.display());
        }
        Ok(catalogue)
    }

    /// The file a catalogue for `language` would be at, inside `folder`.
    pub fn path_in(folder: impl AsRef<Path>, language: Language) -> PathBuf {
        folder.as_ref().join(language.file_name())
    }
}

/// Decodes the two escapes the format defines.
fn unescape(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut chars = value.chars();
    while let Some(ch) = chars.next() {
        if ch != '\\' {
            out.push(ch);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('\\') => out.push('\\'),
            // An unknown escape keeps the backslash: dropping it would silently mangle a
            // Windows path in a translation, which is exactly the kind of bug nobody finds.
            Some(other) => {
                out.push('\\');
                out.push(other);
            }
            None => out.push('\\'),
        }
    }
    out
}

/// The live translation state: one catalogue per loaded language, plus the fallback.
///
/// Not a singleton: a test, a menu and a game can each hold one, and the engine has no
/// opinion about which is "the" language. A game that wants a global makes one, which is a
/// line of code and keeps the engine free of global mutable state.
#[derive(Debug, Clone)]
pub struct Translator {
    catalogues: BTreeMap<Language, Catalogue>,
    language: Language,
    fallback: Language,
}

impl Default for Translator {
    fn default() -> Self {
        Self::new(Language::English)
    }
}

impl Translator {
    /// A translator for `language` with no strings loaded yet.
    ///
    /// The fallback starts as English, because a key missing from a half-translated
    /// catalogue should read as *something* rather than as a key.
    pub fn new(language: Language) -> Self {
        Self {
            catalogues: BTreeMap::new(),
            language,
            fallback: Language::English,
        }
    }

    /// Adds (or replaces) a catalogue.
    pub fn add(&mut self, catalogue: Catalogue) -> &mut Self {
        self.catalogues.insert(catalogue.language(), catalogue);
        self
    }

    /// Loads `folder/<code>.txt` for every language that has one.
    ///
    /// Missing files are not errors: a game that ships English only should not have to
    /// create empty files for the other two.
    ///
    /// # Errors
    ///
    /// A file that exists but cannot be read — a permission problem is worth reporting,
    /// because it looks exactly like "the translation does not work".
    pub fn load_folder(&mut self, folder: impl AsRef<Path>) -> Result<usize, String> {
        let folder = folder.as_ref();
        let mut loaded = 0;
        for language in Language::ALL {
            let path = Catalogue::path_in(folder, language);
            if !path.exists() {
                continue;
            }
            self.add(Catalogue::load(language, &path)?);
            loaded += 1;
        }
        Ok(loaded)
    }

    /// The active language.
    pub const fn language(&self) -> Language {
        self.language
    }

    /// The language used when a key is missing from the active one.
    pub const fn fallback(&self) -> Language {
        self.fallback
    }

    /// Switches the active language, returning the previous one.
    pub fn set_language(&mut self, language: Language) -> Language {
        std::mem::replace(&mut self.language, language)
    }

    /// Sets the fallback language.
    pub fn set_fallback(&mut self, language: Language) {
        self.fallback = language;
    }

    /// The catalogue for `language`, if it was loaded.
    pub fn catalogue(&self, language: Language) -> Option<&Catalogue> {
        self.catalogues.get(&language)
    }

    /// Every language with a catalogue.
    pub fn loaded_languages(&self) -> Vec<Language> {
        self.catalogues.keys().copied().collect()
    }

    /// The translation of `key` with no substitution.
    ///
    /// Falls back to the fallback language, then to the key itself. An empty string counts
    /// as missing: a catalogue with `menu.quit =` is a translator's mistake, and showing the
    /// key is more useful than showing nothing.
    pub fn text(&self, key: &str) -> String {
        for language in [self.language, self.fallback] {
            if let Some(value) = self
                .catalogues
                .get(&language)
                .and_then(|catalogue| catalogue.get(key))
            {
                if !value.is_empty() {
                    return value.to_owned();
                }
            }
        }
        key.to_owned()
    }

    /// The translation of `key` with `{name}` placeholders replaced, in one pass.
    ///
    /// ```ignore
    /// let text = translator.format("hud.lap", &[("current", "2"), ("total", "5")]);
    /// ```
    pub fn format(&self, key: &str, values: &[(&str, &str)]) -> String {
        let template = self.text(key);
        substitute(&template, values)
    }

    /// Whether `key` exists in the active or the fallback catalogue.
    ///
    /// For a UI that wants to hide a row whose string is missing instead of showing the key.
    pub fn has(&self, key: &str) -> bool {
        [self.language, self.fallback].into_iter().any(|language| {
            self.catalogues
                .get(&language)
                .is_some_and(|catalogue| catalogue.get(key).is_some_and(|v| !v.is_empty()))
        })
    }

    /// The keys present in `language` but missing from another, and vice versa.
    ///
    /// The tool a translator wants: run it in a test and a forgotten key becomes a failing
    /// build instead of an English sentence in the Portuguese menu.
    pub fn missing_keys(&self, language: Language, reference: Language) -> Vec<String> {
        let (Some(target), Some(reference)) = (
            self.catalogues.get(&language),
            self.catalogues.get(&reference),
        ) else {
            return Vec::new();
        };
        reference
            .keys()
            .filter(|key| target.get(key).is_none_or(str::is_empty))
            .map(str::to_owned)
            .collect()
    }
}

/// Replaces `{name}` in `template`, one pass, left to right.
fn substitute(template: &str, values: &[(&str, &str)]) -> String {
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(start) = rest.find('{') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        let Some(end) = after.find('}') else {
            // An unclosed brace is literal text: a translation is allowed to be wrong about
            // braces without the whole string disappearing.
            out.push_str(&rest[start..]);
            return out;
        };
        let name = &after[..end];
        match values.iter().find(|(key, _)| *key == name) {
            Some((_, value)) => out.push_str(value),
            // An unknown placeholder stays as written, so a missing value is visible in the
            // string rather than silently deleted.
            None => {
                out.push('{');
                out.push_str(name);
                out.push('}');
            }
        }
        rest = &after[end + 1..];
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const EN: &str = "\
# English
menu.start = Start
menu.quit = Quit
hud.lap = Lap {current} of {total}
hud.newline = first\\nsecond
hud.backslash = C:\\\\games\\\\giochi
hud.equals = a = b
";

    const PT: &str = "\
# Português
menu.start = Começar
menu.quit = Sair
hud.lap = Volta {current} de {total}
";

    fn translator() -> Translator {
        let mut translator = Translator::new(Language::English);
        let (en, skipped) = Catalogue::parse(Language::English, EN);
        assert_eq!(skipped, 0);
        let (pt, _) = Catalogue::parse(Language::Portuguese, PT);
        translator.add(en).add(pt);
        translator
    }

    #[test]
    fn the_format_reads_keys_values_comments_and_escapes() {
        let (catalogue, skipped) = Catalogue::parse(Language::English, EN);
        assert_eq!(skipped, 0);
        assert_eq!(catalogue.get("menu.start"), Some("Start"));
        assert_eq!(catalogue.get("hud.newline"), Some("first\nsecond"));
        assert_eq!(catalogue.get("hud.backslash"), Some("C:\\games\\giochi"));
        // Everything after the first `=` is the value, so a value may contain `=`.
        assert_eq!(catalogue.get("hud.equals"), Some("a = b"));
        assert_eq!(catalogue.len(), 6);
    }

    #[test]
    fn a_line_without_equals_is_skipped_and_counted() {
        let (catalogue, skipped) =
            Catalogue::parse(Language::English, "good = yes\nthis is prose\n= no key\n");
        assert_eq!(skipped, 2);
        assert_eq!(catalogue.len(), 1);
        assert_eq!(catalogue.get("good"), Some("yes"));
    }

    #[test]
    fn the_active_language_wins_and_the_fallback_covers_the_gaps() {
        let translator = translator();
        assert_eq!(translator.text("menu.start"), "Start");

        let mut translator = translator;
        translator.set_language(Language::Portuguese);
        assert_eq!(translator.text("menu.start"), "Começar");
        // `hud.newline` exists only in English: the fallback answers instead of the key.
        assert_eq!(translator.text("hud.newline"), "first\nsecond");
        // A key in neither catalogue reads as itself, which is a visible bug rather than a
        // blank button.
        assert_eq!(translator.text("menu.missing"), "menu.missing");
        assert!(!translator.has("menu.missing"));
        assert!(translator.has("menu.start"));
    }

    #[test]
    fn an_empty_translation_counts_as_missing() {
        let mut translator = Translator::new(Language::Portuguese);
        let (mut pt, _) =
            Catalogue::parse(Language::Portuguese, "menu.quit =\nmenu.start = Começar");
        pt.insert("hud.lap", "Volta");
        translator.add(pt);
        let (en, _) = Catalogue::parse(Language::English, "menu.quit = Quit\n");
        translator.add(en);
        translator.set_language(Language::Portuguese);
        assert_eq!(
            translator.text("menu.quit"),
            "Quit",
            "the fallback fills it"
        );
    }

    #[test]
    fn placeholders_are_replaced_in_one_pass() {
        let translator = translator();
        assert_eq!(
            translator.format("hud.lap", &[("current", "2"), ("total", "5")]),
            "Lap 2 of 5"
        );
        // An unknown placeholder stays visible.
        assert_eq!(
            translator.format("hud.lap", &[("current", "1")]),
            "Lap 1 of {total}"
        );
        // One pass: a value that itself contains a placeholder is not expanded again.
        assert_eq!(
            translator.format("hud.lap", &[("current", "{total}"), ("total", "3")]),
            "Lap {total} of 3"
        );
    }

    #[test]
    fn an_unclosed_brace_is_literal_text() {
        let mut translator = Translator::new(Language::English);
        let (en, _) = Catalogue::parse(Language::English, "odd = half {open\n");
        translator.add(en);
        assert_eq!(translator.text("odd"), "half {open");
    }

    #[test]
    fn switching_language_returns_the_previous_one() {
        let mut translator = translator();
        assert_eq!(translator.language(), Language::English);
        assert_eq!(
            translator.set_language(Language::Spanish),
            Language::English
        );
        assert_eq!(translator.language(), Language::Spanish);
    }

    #[test]
    fn a_missing_translation_is_reported_by_language() {
        let translator = translator();
        let missing = translator.missing_keys(Language::Portuguese, Language::English);
        assert!(missing.contains(&"hud.newline".to_owned()), "{missing:?}");
        assert!(!missing.contains(&"menu.start".to_owned()), "{missing:?}");
        // The other direction has nothing missing: Portuguese is a subset here.
        assert!(translator
            .missing_keys(Language::English, Language::Portuguese)
            .is_empty());
    }

    #[test]
    fn a_language_code_accepts_a_full_locale() {
        assert_eq!(Language::from_code("pt"), Some(Language::Portuguese));
        assert_eq!(Language::from_code("pt_BR"), Some(Language::Portuguese));
        assert_eq!(Language::from_code("pt-BR"), Some(Language::Portuguese));
        assert_eq!(Language::from_code("EN"), Some(Language::English));
        assert_eq!(Language::from_code("es_419"), Some(Language::Spanish));
        assert_eq!(Language::from_code("de"), None);
    }

    #[test]
    fn every_language_has_a_code_a_name_and_a_file() {
        let mut codes = Vec::new();
        for language in Language::ALL {
            assert!(!language.code().is_empty());
            assert!(!language.native_name().is_empty());
            assert!(language.file_name().ends_with(".txt"));
            assert!(!language.is_rtl());
            assert_eq!(Language::from_code(language.code()), Some(language));
            codes.push(language.code());
        }
        codes.sort_unstable();
        codes.dedup();
        assert_eq!(codes.len(), Language::ALL.len(), "codes are unique");
    }

    #[test]
    fn a_folder_loads_every_catalogue_it_finds() {
        let dir = std::env::temp_dir().join(format!(
            "giochi-i18n-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::create_dir_all(&dir).expect("dir");
        std::fs::write(dir.join("en.txt"), EN).expect("en");
        std::fs::write(dir.join("pt.txt"), PT).expect("pt");

        let mut translator = Translator::new(Language::English);
        assert_eq!(translator.load_folder(&dir).expect("loaded"), 2);
        assert_eq!(
            translator.loaded_languages(),
            vec![Language::English, Language::Portuguese]
        );
        assert_eq!(translator.text("menu.start"), "Start");
        translator.set_language(Language::Portuguese);
        assert_eq!(translator.text("menu.start"), "Começar");

        // A folder with no files is not an error, it is an untranslated game.
        let empty = dir.join("empty");
        std::fs::create_dir_all(&empty).expect("dir");
        let mut bare = Translator::new(Language::English);
        assert_eq!(bare.load_folder(&empty).expect("loaded"), 0);
        assert!(bare.loaded_languages().is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_catalogue_file_that_cannot_be_read_names_the_path() {
        let error = Catalogue::load(Language::English, "/tmp/giochi-no-such-folder/en.txt")
            .expect_err("must fail");
        assert!(error.contains("en.txt"), "{error}");
    }
}
