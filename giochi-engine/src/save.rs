//! Save slots: numbered files with a small header and an opaque payload.
//!
//! # Why the engine stores an opaque string
//!
//! What a game needs to save is its own business — a racer saves positions and lap times, a
//! platformer saves a flag per level. The engine's job is the part everybody gets wrong:
//! naming the file, writing it without leaving half a save behind, refusing a save from a
//! newer build, and listing what is there with enough metadata to draw a menu. So the payload
//! is a `&str` the game produces (JSON, usually) and the header is the engine's.
//!
//! # The file
//!
//! ```text
//! giochi-save
//! version = 1
//! slot = 2
//! name = Depois da ponte
//! saved_at = 1763500000
//! playtime = 123.5
//! payload_bytes = 512
//! ---
//! {"level": 3, "lives": 2}
//! ```
//!
//! The `---` line ends the header, and `payload_bytes` says exactly how much follows, so a
//! payload containing anything at all — including `---` or a line that looks like a header —
//! reads back byte for byte. Parsing the payload is the game's problem; **not** mangling it is
//! this module's.
//!
//! # The four rules
//!
//! 1. **Atomic**: written to `.tmp` and renamed, so a power cut leaves either the old save or
//!    the new one, never a truncated file. GIOCHI-OS runs on an SD card, where this is not
//!    hypothetical.
//! 2. **Versioned**: a save from a newer build is refused with the two numbers, rather than
//!    half-read. A save from an older build is accepted — that is what a version number is
//!    for.
//! 3. **Named**: the slot's display name and play time live in the header, so a load menu does
//!    not have to parse the payload to draw a row.
//! 4. **Bounded**: a slot number outside the range is an error, not a path traversal: the file
//!    name is built from the number, never from a string a caller supplied.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// The magic first line of every save file.
pub const MAGIC: &str = "giochi-save";
/// The format version this build writes, and the newest it reads.
pub const VERSION: u32 = 1;
/// How many numbered slots exist. `0..SLOT_COUNT`.
pub const SLOT_COUNT: usize = 8;
/// The extension of a save file.
pub const EXTENSION: &str = "save";

/// What went wrong with a save or a load.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SaveError {
    /// The slot number is outside `0..SLOT_COUNT`.
    ///
    /// A separate variant rather than an I/O error because it is a programming mistake, not a
    /// disk problem, and a game that shows "could not save" for it is hiding a bug.
    Slot(usize),
    /// The file could not be read or written. The message names the path.
    Io(String),
    /// The file is not a save, or its header is damaged.
    Corrupt(String),
    /// The save was written by a newer build.
    Version {
        /// The version in the file.
        found: u32,
        /// The version this build understands.
        supported: u32,
    },
}

impl std::fmt::Display for SaveError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Slot(slot) => write!(formatter, "slot {slot} is outside 0..{SLOT_COUNT}"),
            Self::Io(message) => write!(formatter, "{message}"),
            Self::Corrupt(message) => write!(formatter, "damaged save: {message}"),
            Self::Version { found, supported } => write!(
                formatter,
                "the save is version {found} and this build reads up to {supported}"
            ),
        }
    }
}

impl std::error::Error for SaveError {}

/// What a game hands to [`SaveSlots::save`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SaveRequest<'a> {
    /// The slot to write, `0..SLOT_COUNT`.
    pub slot: usize,
    /// The name a load menu shows. Control characters are replaced, and it is trimmed.
    pub name: &'a str,
    /// Seconds of play so far, for the menu.
    pub playtime: f32,
    /// The game's own data. Opaque to the engine.
    pub payload: &'a str,
}

impl<'a> SaveRequest<'a> {
    /// A request for `slot` with `payload` and no name or play time.
    pub const fn new(slot: usize, payload: &'a str) -> Self {
        Self {
            slot,
            name: "",
            playtime: 0.0,
            payload,
        }
    }

    /// The same request with a display name.
    pub const fn with_name(mut self, name: &'a str) -> Self {
        self.name = name;
        self
    }

    /// The same request with a play time.
    pub const fn with_playtime(mut self, seconds: f32) -> Self {
        self.playtime = seconds;
        self
    }
}

/// What a load menu needs to draw one row.
#[derive(Debug, Clone, PartialEq)]
pub struct SlotInfo {
    /// The slot number.
    pub slot: usize,
    /// The display name the game gave it.
    pub name: String,
    /// Unix seconds, or `0` when the file did not say.
    pub saved_at: u64,
    /// Seconds of play at the moment of saving.
    pub playtime: f32,
    /// The format version in the file.
    pub version: u32,
    /// The payload's length in bytes, for the row's detail line.
    pub payload_bytes: usize,
    /// Where the file is.
    pub path: PathBuf,
}

/// A loaded save: the metadata and the game's payload.
#[derive(Debug, Clone, PartialEq)]
pub struct Loaded {
    /// The header.
    pub info: SlotInfo,
    /// The game's data, unchanged.
    pub payload: String,
}

/// A folder of save slots.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SaveSlots {
    root: PathBuf,
}

impl SaveSlots {
    /// Slots in `root`, which is created lazily on the first write.
    pub fn new(root: impl AsRef<Path>) -> Self {
        Self {
            root: root.as_ref().to_path_buf(),
        }
    }

    /// The folder the slots live in.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The file for `slot`, or [`SaveError::Slot`].
    ///
    /// The name is built from the number, so a caller cannot hand a path in.
    pub fn path_for(&self, slot: usize) -> Result<PathBuf, SaveError> {
        if slot >= SLOT_COUNT {
            return Err(SaveError::Slot(slot));
        }
        Ok(self.root.join(format!("slot-{slot}.{EXTENSION}")))
    }

    /// Writes `request`, returning the metadata it will read back as.
    ///
    /// # Errors
    ///
    /// [`SaveError::Slot`] for a bad slot, [`SaveError::Io`] when the folder or the file
    /// cannot be written.
    pub fn save(&self, request: SaveRequest<'_>) -> Result<SlotInfo, SaveError> {
        self.save_at(request, unix_now())
    }

    /// [`SaveSlots::save`] with an explicit timestamp, so a test does not depend on a clock.
    pub fn save_at(&self, request: SaveRequest<'_>, saved_at: u64) -> Result<SlotInfo, SaveError> {
        let path = self.path_for(request.slot)?;
        std::fs::create_dir_all(&self.root).map_err(|error| {
            SaveError::Io(format!("cannot create {}: {error}", self.root.display()))
        })?;

        let name = sanitise_name(request.name);
        let payload_bytes = request.payload.len();
        let playtime = if request.playtime.is_finite() {
            request.playtime.max(0.0)
        } else {
            0.0
        };
        let header = format!(
            "{MAGIC}\nversion = {VERSION}\nslot = {}\nname = {name}\nsaved_at = {saved_at}\n\
             playtime = {playtime}\npayload_bytes = {payload_bytes}\n---\n",
            request.slot
        );
        let mut contents = String::with_capacity(header.len() + payload_bytes);
        contents.push_str(&header);
        contents.push_str(request.payload);
        contents.push('\n');

        // Atomic: a rename replaces the old file or leaves it alone, and a truncated write
        // never becomes the visible save.
        let temporary = path.with_extension(format!("{EXTENSION}.tmp"));
        std::fs::write(&temporary, &contents).map_err(|error| {
            SaveError::Io(format!("cannot write {}: {error}", temporary.display()))
        })?;
        std::fs::rename(&temporary, &path).map_err(|error| {
            let _ = std::fs::remove_file(&temporary);
            SaveError::Io(format!("cannot replace {}: {error}", path.display()))
        })?;

        Ok(SlotInfo {
            slot: request.slot,
            name,
            saved_at,
            playtime,
            version: VERSION,
            payload_bytes,
            path,
        })
    }

    /// Reads `slot` back.
    ///
    /// # Errors
    ///
    /// [`SaveError::Slot`] for a bad slot, [`SaveError::Io`] when the file is not there,
    /// [`SaveError::Corrupt`] when the header is damaged, [`SaveError::Version`] when the file
    /// is from a newer build.
    pub fn load(&self, slot: usize) -> Result<Loaded, SaveError> {
        let path = self.path_for(slot)?;
        let text = std::fs::read_to_string(&path)
            .map_err(|error| SaveError::Io(format!("cannot read {}: {error}", path.display())))?;
        parse(&text, slot, &path)
    }

    /// The header of `slot`, or `None` when there is no readable save there.
    ///
    /// A damaged file reads as `None` rather than as an error: a load menu with eight rows
    /// should draw seven and leave one empty, not refuse to open.
    pub fn info(&self, slot: usize) -> Option<SlotInfo> {
        let path = self.path_for(slot).ok()?;
        let text = std::fs::read_to_string(&path).ok()?;
        parse(&text, slot, &path).ok().map(|loaded| loaded.info)
    }

    /// Every readable slot, in slot order.
    pub fn list(&self) -> Vec<SlotInfo> {
        (0..SLOT_COUNT).filter_map(|slot| self.info(slot)).collect()
    }

    /// Whether `slot` has a readable save.
    pub fn exists(&self, slot: usize) -> bool {
        self.info(slot).is_some()
    }

    /// The most recently written slot, if any.
    pub fn newest(&self) -> Option<SlotInfo> {
        self.list().into_iter().max_by_key(|info| info.saved_at)
    }

    /// Deletes `slot`. Returns whether a file went away.
    pub fn delete(&self, slot: usize) -> bool {
        match self.path_for(slot) {
            Ok(path) => std::fs::remove_file(path).is_ok(),
            Err(_) => false,
        }
    }

    /// Deletes every slot. Returns how many went away.
    pub fn delete_all(&self) -> usize {
        (0..SLOT_COUNT).filter(|slot| self.delete(*slot)).count()
    }

    /// The first slot with no save, for a game that wants "save to the next free one".
    pub fn first_free(&self) -> Option<usize> {
        (0..SLOT_COUNT).find(|slot| !self.exists(*slot))
    }
}

/// Parses a save file. `slot` and `path` are only used to fill the metadata.
fn parse(text: &str, slot: usize, path: &Path) -> Result<Loaded, SaveError> {
    let mut lines = text.split_inclusive('\n');
    let Some(first) = lines.next() else {
        return Err(SaveError::Corrupt("the file is empty".to_owned()));
    };
    if first.trim_end() != MAGIC {
        return Err(SaveError::Corrupt(format!(
            "the first line is {:?}, not {MAGIC}",
            first.trim_end()
        )));
    }

    let mut version = 0;
    let mut name = String::new();
    let mut saved_at = 0;
    let mut playtime = 0.0;
    let mut payload_bytes = 0;
    let mut header_len = first.len();
    let mut seen_end = false;
    for line in lines.by_ref() {
        header_len += line.len();
        let line = line.trim_end_matches(['\n', '\r']);
        if line == "---" {
            seen_end = true;
            break;
        }
        let Some((key, value)) = line.split_once('=') else {
            return Err(SaveError::Corrupt(format!(
                "header line {line:?} has no '='"
            )));
        };
        let (key, value) = (key.trim(), value.trim());
        match key {
            "version" => {
                version = value
                    .parse()
                    .map_err(|_| SaveError::Corrupt(format!("version {value:?} is not a number")))?
            }
            "name" => name = value.to_owned(),
            "saved_at" => saved_at = value.parse().unwrap_or(0),
            "playtime" => playtime = value.parse().unwrap_or(0.0),
            "payload_bytes" => {
                payload_bytes = value.parse().map_err(|_| {
                    SaveError::Corrupt(format!("payload_bytes {value:?} is not a number"))
                })?
            }
            // `slot` is in the header for a human reading the file; the file name is the
            // authority, so a mismatched number is not worth refusing over.
            _ => {}
        }
    }
    if !seen_end {
        return Err(SaveError::Corrupt(
            "the header has no `---` line".to_owned(),
        ));
    }
    if version > VERSION {
        return Err(SaveError::Version {
            found: version,
            supported: VERSION,
        });
    }

    let payload = text.get(header_len..).unwrap_or_default();
    if payload.len() < payload_bytes {
        return Err(SaveError::Corrupt(format!(
            "the header declares {payload_bytes} payload bytes and the file has {}",
            payload.len()
        )));
    }
    // The writer appends one newline after the payload; a file without it (written by hand)
    // still reads.
    let payload = payload.get(..payload_bytes).unwrap_or(payload);
    Ok(Loaded {
        info: SlotInfo {
            slot,
            name,
            saved_at,
            playtime,
            version,
            payload_bytes,
            path: path.to_path_buf(),
        },
        payload: payload.to_owned(),
    })
}

/// Trims `name` and replaces control characters, so one line stays one line.
fn sanitise_name(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|ch| if ch.is_control() { ' ' } else { ch })
        .collect();
    cleaned.trim().chars().take(64).collect()
}

/// Seconds since the Unix epoch, or `0` if the clock is before it.
fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

/// A folder for saves, next to the executable by default.
///
/// A game that wants saves elsewhere builds [`SaveSlots`] itself; this is the default a demo
/// or a small game needs so it does not have to invent one.
pub fn default_folder() -> PathBuf {
    if let Some(dir) = std::env::var_os("GIOCHI_SAVE_DIR") {
        return PathBuf::from(dir);
    }
    std::env::current_dir()
        .map(|dir| dir.join("saves"))
        .unwrap_or_else(|_| PathBuf::from("saves"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folder(name: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "giochi-saves-{name}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&path);
        path
    }

    #[test]
    fn a_save_round_trips_with_its_metadata() {
        let slots = SaveSlots::new(folder("roundtrip"));
        let info = slots
            .save_at(
                SaveRequest::new(2, r#"{"level":3}"#)
                    .with_name("Depois da ponte")
                    .with_playtime(123.5),
                1_763_500_000,
            )
            .expect("saved");
        assert_eq!(info.slot, 2);
        assert_eq!(info.name, "Depois da ponte");
        assert_eq!(info.saved_at, 1_763_500_000);
        assert!((info.playtime - 123.5).abs() < 1e-4);
        assert_eq!(info.payload_bytes, r#"{"level":3}"#.len());

        let loaded = slots.load(2).expect("loaded");
        assert_eq!(loaded.payload, r#"{"level":3}"#);
        assert_eq!(loaded.info, info);
        let _ = std::fs::remove_dir_all(slots.root());
    }

    #[test]
    fn a_payload_that_looks_like_a_header_survives() {
        // The payload is length-prefixed, so nothing in it can confuse the parser — including
        // a line that says `---` or one that says `version = 99`.
        let slots = SaveSlots::new(folder("tricky"));
        let payload = "---\nversion = 99\nname = nope\n{\"real\": true}";
        slots
            .save_at(SaveRequest::new(0, payload), 1)
            .expect("saved");
        let loaded = slots.load(0).expect("loaded");
        assert_eq!(loaded.payload, payload);
        assert_eq!(loaded.info.version, VERSION);
        let _ = std::fs::remove_dir_all(slots.root());
    }

    #[test]
    fn an_empty_payload_is_legal() {
        let slots = SaveSlots::new(folder("empty"));
        slots.save_at(SaveRequest::new(1, ""), 7).expect("saved");
        let loaded = slots.load(1).expect("loaded");
        assert_eq!(loaded.payload, "");
        assert_eq!(loaded.info.payload_bytes, 0);
        let _ = std::fs::remove_dir_all(slots.root());
    }

    #[test]
    fn a_slot_number_outside_the_range_is_refused_and_never_becomes_a_path() {
        let slots = SaveSlots::new(folder("slot"));
        assert_eq!(slots.path_for(SLOT_COUNT), Err(SaveError::Slot(SLOT_COUNT)));
        assert_eq!(slots.path_for(usize::MAX), Err(SaveError::Slot(usize::MAX)));
        assert!(matches!(
            slots.save(SaveRequest::new(99, "x")),
            Err(SaveError::Slot(99))
        ));
        assert!(matches!(slots.load(99), Err(SaveError::Slot(99))));
        assert!(!slots.exists(99));
        assert!(slots.info(99).is_none());
        assert!(!slots.delete(99));
    }

    #[test]
    fn a_save_from_a_newer_build_is_refused_with_the_two_numbers() {
        let slots = SaveSlots::new(folder("version"));
        std::fs::create_dir_all(slots.root()).expect("dir");
        let path = slots.path_for(0).expect("path");
        std::fs::write(
            &path,
            "giochi-save\nversion = 99\nslot = 0\nname = x\npayload_bytes = 2\n---\nhi\n",
        )
        .expect("written");
        match slots.load(0) {
            Err(SaveError::Version { found, supported }) => {
                assert_eq!(found, 99);
                assert_eq!(supported, VERSION);
            }
            other => panic!("expected a version error, got {other:?}"),
        }
        // `info` treats it as unreadable rather than failing a whole menu.
        assert!(slots.info(0).is_none());
        let _ = std::fs::remove_dir_all(slots.root());
    }

    #[test]
    fn an_older_version_reads() {
        let slots = SaveSlots::new(folder("older"));
        std::fs::create_dir_all(slots.root()).expect("dir");
        let path = slots.path_for(1).expect("path");
        std::fs::write(
            &path,
            "giochi-save\nversion = 0\nslot = 1\nname = old\nsaved_at = 5\nplaytime = 1.0\npayload_bytes = 2\n---\nok\n",
        )
        .expect("written");
        let loaded = slots.load(1).expect("an older save reads");
        assert_eq!(loaded.payload, "ok");
        assert_eq!(loaded.info.name, "old");
        let _ = std::fs::remove_dir_all(slots.root());
    }

    #[test]
    fn damaged_files_are_reported_and_do_not_break_the_listing() {
        let slots = SaveSlots::new(folder("damaged"));
        std::fs::create_dir_all(slots.root()).expect("dir");
        let cases = [
            ("not a save at all", "the first line"),
            ("giochi-save\nversion = 1\nname = x\n", "`---`"),
            (
                "giochi-save\nversion = 1\npayload_bytes = 50\n---\nshort\n",
                "declares 50",
            ),
            ("giochi-save\ngarbage\n---\n", "no '='"),
        ];
        for (index, (content, expected)) in cases.iter().enumerate() {
            let path = slots.path_for(index).expect("path");
            std::fs::write(&path, content).expect("written");
            let error = slots.load(index).expect_err("must fail");
            assert!(
                format!("{error}").contains(expected),
                "case {index}: {error} does not mention {expected:?}"
            );
        }
        // One good save among the damaged ones still lists.
        slots
            .save_at(SaveRequest::new(SLOT_COUNT - 1, "fine"), 9)
            .expect("saved");
        assert_eq!(slots.list().len(), 1);
        let _ = std::fs::remove_dir_all(slots.root());
    }

    #[test]
    fn the_listing_is_in_slot_order_and_newest_finds_the_latest() {
        let slots = SaveSlots::new(folder("list"));
        slots.save_at(SaveRequest::new(3, "c"), 30).expect("saved");
        slots.save_at(SaveRequest::new(0, "a"), 10).expect("saved");
        slots.save_at(SaveRequest::new(5, "e"), 50).expect("saved");
        let listed: Vec<usize> = slots.list().iter().map(|info| info.slot).collect();
        assert_eq!(listed, vec![0, 3, 5]);
        assert_eq!(slots.newest().map(|info| info.slot), Some(5));
        assert_eq!(slots.first_free(), Some(1));
        let _ = std::fs::remove_dir_all(slots.root());
    }

    #[test]
    fn saving_over_a_slot_replaces_it() {
        let slots = SaveSlots::new(folder("replace"));
        slots
            .save_at(SaveRequest::new(0, "first"), 1)
            .expect("saved");
        slots
            .save_at(SaveRequest::new(0, "second"), 2)
            .expect("saved");
        assert_eq!(slots.load(0).expect("loaded").payload, "second");
        assert_eq!(slots.list().len(), 1);
        // No `.tmp` left behind.
        let leftovers: Vec<_> = std::fs::read_dir(slots.root())
            .expect("dir")
            .filter_map(|entry| entry.ok())
            .filter(|entry| entry.path().to_string_lossy().ends_with(".tmp"))
            .collect();
        assert!(leftovers.is_empty(), "{leftovers:?}");
        let _ = std::fs::remove_dir_all(slots.root());
    }

    #[test]
    fn deleting_works_and_says_so() {
        let slots = SaveSlots::new(folder("delete"));
        slots.save_at(SaveRequest::new(1, "x"), 1).expect("saved");
        slots.save_at(SaveRequest::new(2, "y"), 2).expect("saved");
        assert!(slots.delete(1));
        assert!(!slots.delete(1), "a second delete finds nothing");
        assert!(!slots.exists(1));
        assert!(slots.exists(2));
        assert_eq!(slots.delete_all(), 1);
        assert!(slots.list().is_empty());
        let _ = std::fs::remove_dir_all(slots.root());
    }

    #[test]
    fn the_name_is_sanitised() {
        let slots = SaveSlots::new(folder("name"));
        let info = slots
            .save_at(
                SaveRequest::new(0, "x").with_name("  line\nbreak\there  "),
                1,
            )
            .expect("saved");
        assert_eq!(info.name, "line break here");
        // A very long name is capped, so one row cannot push a menu around.
        let long = "a".repeat(500);
        let info = slots
            .save_at(SaveRequest::new(1, "x").with_name(&long), 1)
            .expect("saved");
        assert_eq!(info.name.len(), 64);
        let _ = std::fs::remove_dir_all(slots.root());
    }

    #[test]
    fn a_negative_or_not_finite_playtime_is_stored_as_zero() {
        let slots = SaveSlots::new(folder("playtime"));
        let info = slots
            .save_at(SaveRequest::new(0, "x").with_playtime(-5.0), 1)
            .expect("saved");
        assert_eq!(info.playtime, 0.0);
        let info = slots
            .save_at(SaveRequest::new(1, "x").with_playtime(f32::NAN), 1)
            .expect("saved");
        assert_eq!(info.playtime, 0.0);
        let _ = std::fs::remove_dir_all(slots.root());
    }

    #[test]
    fn the_file_is_readable_by_a_human() {
        // Not a test of the parser: a save a developer can `cat` and understand is worth the
        // four extra lines, and this is the assertion that keeps the header from rotting into
        // a binary blob.
        let slots = SaveSlots::new(folder("human"));
        slots
            .save_at(SaveRequest::new(4, "{\"lap\":2}").with_name("Volta 3"), 42)
            .expect("saved");
        let text = std::fs::read_to_string(slots.path_for(4).expect("path")).expect("read");
        assert!(text.starts_with("giochi-save\n"));
        assert!(text.contains("name = Volta 3"));
        assert!(text.contains("slot = 4"));
        assert!(text.contains("---\n{\"lap\":2}"));
        let _ = std::fs::remove_dir_all(slots.root());
    }

    #[test]
    fn a_missing_save_is_an_io_error_that_names_the_path() {
        let slots = SaveSlots::new(folder("missing"));
        let error = slots.load(3).expect_err("must fail");
        match error {
            SaveError::Io(message) => assert!(message.contains("slot-3.save"), "{message}"),
            other => panic!("expected an I/O error, got {other:?}"),
        }
    }
}
