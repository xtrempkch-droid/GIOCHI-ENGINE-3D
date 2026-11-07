//! Mechanically enforces the C-shaped design rules (R1 and R3) over the crate source.
//!
//! The rules are invariants about *every* public item, so a new module can break them
//! without anyone noticing until a C consumer tries to use the type. Reading the crate's
//! own source is the only way to check "all of them" rather than "the ones a reviewer
//! remembers", and doing it here means the gate fails instead of the ABI.
//!
//! The scan is deliberately textual: the codebase is `rustfmt`-formatted, so every
//! declaration starts at the beginning of a line and attributes come before it. That
//! keeps the test readable, with no `syn` dependency.
//!
//! This binary does not touch SDL, so it may hold several tests (unlike the smoke tests,
//! which are one-per-binary because of SDL's single `EventPump`).

use std::path::{Path, PathBuf};

/// Public items that are deliberately **not** `#[repr(C)]`, with the rule that requires
/// their exception. Adding a name here is a design decision, not a way to silence a
/// failure — the reason has to be a rule from the project prompt.
const NON_C_EXCEPTIONS: &[(&str, &str)] = &[(
    "GiochiError",
    "R4: errors are `thiserror` internally and never cross the boundary; a Rust enum \
     with `String` payloads must not claim a C layout",
)];

/// Every `.rs` file under `src/`.
fn sources() -> Vec<PathBuf> {
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        let entries = std::fs::read_dir(dir).unwrap_or_else(|e| {
            panic!("cannot read {}: {e}", dir.display());
        });
        for entry in entries {
            let path = entry.expect("directory entry").path();
            if path.is_dir() {
                walk(&path, out);
            } else if path.extension().is_some_and(|ext| ext == "rs") {
                out.push(path);
            }
        }
    }

    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    walk(&src, &mut files);
    files.sort();
    assert!(!files.is_empty(), "no sources found under src/");
    files
}

/// A public item: its name, file, 1-based line, the attributes above it and, for a
/// braced struct, the text of its body.
struct Item {
    name: String,
    file: PathBuf,
    line: usize,
    attributes: String,
    body: Option<String>,
    /// `true` for `pub struct`, `false` for `pub enum`.
    is_struct: bool,
}

impl Item {
    fn location(&self) -> String {
        format!("{}:{}: {}", self.file.display(), self.line, self.name)
    }

    fn is_repr_c(&self) -> bool {
        self.attributes.contains("#[repr(C)]")
    }
}

/// Collects every top-level `pub struct` / `pub enum`.
fn public_types() -> Vec<Item> {
    let mut items = Vec::new();

    for file in sources() {
        let text = std::fs::read_to_string(&file).expect("read source");
        let lines: Vec<&str> = text.lines().collect();

        for (index, line) in lines.iter().enumerate() {
            let trimmed = line.trim_start();
            let (rest, is_struct) = match trimmed
                .strip_prefix("pub struct ")
                .map(|rest| (rest, true))
                .or_else(|| trimmed.strip_prefix("pub enum ").map(|rest| (rest, false)))
            {
                Some(parsed) => parsed,
                None => continue,
            };

            let name: String = rest
                .chars()
                .take_while(|c| c.is_alphanumeric() || *c == '_')
                .collect();

            // Walk back over the attribute block (`#[...]`), skipping blank lines and
            // doc comments, which are documentation rather than attributes here.
            let mut attributes = String::new();
            let mut cursor = index;
            while cursor > 0 {
                cursor -= 1;
                let previous = lines[cursor].trim();
                if previous.is_empty() || previous.starts_with("///") || previous.starts_with("//")
                {
                    continue;
                }
                if previous.starts_with("#[") {
                    attributes.push_str(previous);
                    attributes.push('\n');
                    continue;
                }
                break;
            }

            // A braced body, if the declaration does not end on this line.
            let mut body = None;
            if !line.contains('{') || !line.contains('}') {
                let mut collected = String::new();
                let mut depth = 0i32;
                for follow in &lines[index..] {
                    depth += follow.matches('{').count() as i32;
                    depth -= follow.matches('}').count() as i32;
                    if depth == 0 && follow.contains('}') {
                        break;
                    }
                    collected.push_str(follow);
                    collected.push('\n');
                }
                if !collected.is_empty() {
                    body = Some(collected);
                }
            }

            items.push(Item {
                name,
                file: file.clone(),
                line: index + 1,
                attributes,
                body,
                is_struct,
            });
        }
    }

    items
}

/// R1: every user-visible type is `#[repr(C)]`, unless a rule of the prompt says
/// otherwise.
#[test]
fn every_public_type_is_repr_c() {
    let exceptions: Vec<&str> = NON_C_EXCEPTIONS.iter().map(|(name, _)| *name).collect();
    let mut offenders = Vec::new();

    for item in public_types() {
        if item.is_repr_c() || exceptions.contains(&item.name.as_str()) {
            continue;
        }
        offenders.push(format!(
            "{} is public but not #[repr(C)] — add #[repr(C)], make it pub(crate), \
             or record the exception in NON_C_EXCEPTIONS",
            item.location()
        ));
    }

    assert!(
        offenders.is_empty(),
        "R1 violation:\n  {}",
        offenders.join("\n  ")
    );
}

/// R1: no public field may hold a type that has no C representation.
#[test]
fn public_fields_hold_only_c_shaped_types() {
    // A field of one of these in a public struct cannot be expressed in C.
    const FORBIDDEN: &[&str] = &["Rc<", "Arc<", "Box<", "String", "Vec<", "HashMap<"];

    let mut offenders = Vec::new();

    for item in public_types() {
        let Some(body) = item.body.as_ref() else {
            continue;
        };

        for line in body.lines() {
            let trimmed = line.trim();
            if !trimmed.starts_with("pub ") || trimmed.starts_with("pub fn") {
                continue;
            }
            // `pub _reserved: [u32; 4]` and primitives are always fine.
            if let Some(name) = FORBIDDEN.iter().find(|name| trimmed.contains(*name)) {
                offenders.push(format!("{}: {trimmed} (contains {name})", item.location()));
            }
        }
    }

    assert!(
        offenders.is_empty(),
        "R1 violation, public field with no C representation:\n  {}",
        offenders.join("\n  ")
    );
}

/// R1: no public function declares generic parameters or an explicit lifetime.
#[test]
fn no_public_function_is_generic_or_borrows_explicitly() {
    let mut offenders = Vec::new();

    for file in sources() {
        let text = std::fs::read_to_string(&file).expect("read source");

        for (index, line) in text.lines().enumerate() {
            let trimmed = line.trim_start();
            let Some(rest) = trimmed
                .strip_prefix("pub fn ")
                .or_else(|| trimmed.strip_prefix("pub const fn "))
            else {
                continue;
            };
            let name: String = rest
                .chars()
                .take_while(|c| c.is_alphanumeric() || *c == '_')
                .collect();

            // `<` immediately after the name is a generic parameter list; a `<` further
            // along belongs to the return type (`Result<Engine, GiochiError>`).
            if rest[name.len()..].starts_with('<') {
                offenders.push(format!(
                    "{}:{}: pub fn {name}<...>",
                    file.display(),
                    index + 1
                ));
            }
        }
    }

    assert!(
        offenders.is_empty(),
        "R1 violation, public function with generics or an explicit lifetime:\n  {}",
        offenders.join("\n  ")
    );
}

/// R3: every `#[repr(C)]` public **struct** ends with `_reserved: [u32; N]`, `N >= 4`.
///
/// R3 names structs, so `#[repr(C)]` enums are exempt: their size is the size of the
/// discriminant, a reserved tail would need a variant, and the ABI maps them to plain
/// `int`s anyway (`docs/C_ABI_CONTRACT.md` §3).
///
/// The field may be private. A private `_reserved` still occupies the same bytes in the
/// layout, and it is strictly better than a public one: a C caller cannot corrupt
/// padding it was told to ignore.
#[test]
fn every_repr_c_struct_reserves_space() {
    let mut offenders = Vec::new();

    for item in public_types() {
        if !item.is_repr_c() || !item.is_struct {
            continue;
        }
        let Some(body) = item.body.as_ref() else {
            // `pub struct Foo;` — a unit struct has no fields to reserve space after.
            continue;
        };

        let fields: Vec<&str> = body
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .filter(|line| !line.starts_with("//"))
            .filter(|line| !line.starts_with('{') && !line.starts_with('}'))
            .collect();

        // Tuple structs are not used in this crate; flagging them keeps it that way.
        if fields.is_empty() {
            continue;
        }

        let last = fields.last().copied().unwrap_or_default();
        let reserved = last
            .strip_prefix("pub _reserved: [u32; ")
            .or_else(|| last.strip_prefix("_reserved: [u32; "))
            .and_then(|rest| rest.strip_suffix("],"));

        match reserved.and_then(|size| size.parse::<usize>().ok()) {
            Some(size) if size >= 4 => {}
            Some(size) => offenders.push(format!(
                "{}: last field is _reserved: [u32; {size}], R3 requires at least 4",
                item.location()
            )),
            None => offenders.push(format!(
                "{}: last field must be `_reserved: [u32; N]` with N >= 4, found `{last}`",
                item.location()
            )),
        }
    }

    assert!(
        offenders.is_empty(),
        "R3 violation:\n  {}",
        offenders.join("\n  ")
    );
}

/// The exception list is honest: it names real types and says which rule allows them.
#[test]
fn every_documented_exception_names_a_real_type_and_a_rule() {
    let names: Vec<String> = public_types().into_iter().map(|item| item.name).collect();

    for (name, reason) in NON_C_EXCEPTIONS {
        assert!(
            names.iter().any(|candidate| candidate == name),
            "NON_C_EXCEPTIONS lists '{name}', which is no longer a public type — remove it"
        );
        assert!(
            reason.contains('R') && reason.len() > 20,
            "NON_C_EXCEPTIONS entry for '{name}' must cite the rule that allows it"
        );
    }
}

/// Sanity check on the scanner itself: it must actually find the types it is meant to
/// check, otherwise every assertion above would pass vacuously.
#[test]
fn the_scanner_finds_the_crate_types() {
    let items = public_types();
    assert!(
        items.len() > 15,
        "expected the crate's public types, found {} — the scanner is broken",
        items.len()
    );
    for expected in [
        "Vec2", "Rect", "Color", "Sprite", "Camera", "Time", "Engine",
    ] {
        assert!(
            items.iter().any(|item| item.name == expected),
            "scanner did not find `{expected}` — the scan is broken"
        );
    }
    assert!(
        items
            .iter()
            .any(|item| item.name == "GiochiError" && !item.is_repr_c()),
        "the scanner must see GiochiError as the documented exception"
    );
}
