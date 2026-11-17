//! Loads the shipped translation catalogues and prints the menu in every language.
//!
//! ```text
//! cargo run -p giochi-engine --example i18n_demo
//! ```
//!
//! It also runs the check a translator wants before shipping: every key that exists in one
//! language must exist in all of them. A missing key is not a crash — it falls back to
//! English — which is exactly why it needs to be *reported* somewhere.

use std::process::ExitCode;

use giochi_engine::{Language, Translator};

fn main() -> ExitCode {
    env_logger::init();

    let folder = format!("{}/../assets/i18n", env!("CARGO_MANIFEST_DIR"));
    let mut translator = Translator::new(Language::English);
    match translator.load_folder(&folder) {
        Ok(loaded) => println!("loaded {loaded} catalogue(s) from {folder}"),
        Err(error) => {
            eprintln!("erro: {error}");
            return ExitCode::FAILURE;
        }
    }

    for language in Language::ALL {
        translator.set_language(language);
        println!("\n{} ({})", language.native_name(), language.code());
        for key in [
            "menu.start",
            "menu.options",
            "options.music",
            "common.cancel",
        ] {
            println!("  {key:<16} {}", translator.text(key));
        }
        println!(
            "  {:<16} {}",
            "hud.lap",
            translator.format("hud.lap", &[("current", "2"), ("total", "5")])
        );
    }

    // The check: every key in English has to exist in the others.
    let mut missing = 0;
    for language in Language::ALL {
        if language == Language::English {
            continue;
        }
        let keys = translator.missing_keys(language, Language::English);
        if !keys.is_empty() {
            println!("\n{} is missing {} key(s):", language.code(), keys.len());
            for key in &keys {
                println!("  {key}");
            }
            missing += keys.len();
        }
    }

    if missing == 0 {
        println!("\ni18n=ok");
        ExitCode::SUCCESS
    } else {
        println!("\ni18n={missing} missing");
        ExitCode::FAILURE
    }
}
