//! `giochi-editor` — the visual scene editor's entry point.
//!
//! ```text
//! cargo run -p giochi-editor
//! cargo run -p giochi-editor -- --frames 120
//! ```

use std::process::ExitCode;

use giochi_editor::{run, EditorOptions};

const USAGE: &str = "\
giochi-editor — visual scene editor for GIOCHI-ENGINE

Usage:
  giochi-editor [--frames N]
  giochi-editor --help

Options:
  --frames N   render N frames and exit (used by the headless check)
  -h, --help   show this help

Keys:
  F2           open/close the in-game console
  Esc          quit (when no text field or console owns the key)
";

fn main() -> ExitCode {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let mut options = EditorOptions::default();
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--frames" => {
                let parsed = args.next().and_then(|value| value.parse::<u64>().ok());
                match parsed {
                    Some(frames) if frames > 0 => options.frames = Some(frames),
                    _ => {
                        eprintln!("--frames needs a positive integer\n\n{USAGE}");
                        return ExitCode::from(2);
                    }
                }
            }
            "-h" | "--help" => {
                println!("{USAGE}");
                return ExitCode::SUCCESS;
            }
            other => {
                eprintln!("unknown argument: {other}\n\n{USAGE}");
                return ExitCode::from(2);
            }
        }
    }

    match run(options) {
        Ok(stats) => {
            log::info!("editor closed after {} frame(s)", stats.frames);
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("giochi-editor: {error}");
            ExitCode::FAILURE
        }
    }
}
