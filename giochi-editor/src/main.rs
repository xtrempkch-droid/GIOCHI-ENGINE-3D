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
  giochi-editor [--project DIR] [--scene FILE] [--config DIR] [--frames N]
  giochi-editor --version
  giochi-editor --help

Options:
  --project DIR  open a project (a folder containing project.toml)
  --scene FILE   open a .scene file before the first frame
  --config DIR   where settings and autosaves live (default: $XDG_CONFIG_HOME/giochi-editor)
  --play-frames N  render N frames of the preview window and return (headless check)
  --frames N     render N frames and exit (used by the headless check)
  -h, --help     show this help
  -V, --version  print the version

Keys:
  Ctrl+N        new scene
  Ctrl+O        open scene (in-editor file dialog)
  Ctrl+I        import an asset into the project
  Ctrl+S        save            Ctrl+Shift+S  save as
  F2            open/close the in-game console
  Esc           quit (when no text field or dialog owns the key)

Viewport:
  right drag    orbit           middle drag   pan
  wheel         zoom
";

fn main() -> ExitCode {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let mut options = EditorOptions::default();
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--project" => match args.next() {
                Some(path) if !path.is_empty() => options.project = Some(path.into()),
                _ => {
                    eprintln!("--project needs a directory\n\n{USAGE}");
                    return ExitCode::from(2);
                }
            },
            "--scene" => match args.next() {
                Some(path) if !path.is_empty() => options.scene = Some(path.into()),
                _ => {
                    eprintln!("--scene needs a path\n\n{USAGE}");
                    return ExitCode::from(2);
                }
            },
            "--config" => match args.next() {
                Some(path) if !path.is_empty() => options.config_dir = Some(path.into()),
                _ => {
                    eprintln!("--config needs a directory\n\n{USAGE}");
                    return ExitCode::from(2);
                }
            },
            "--play-frames" => {
                let parsed = args.next().and_then(|value| value.parse::<u64>().ok());
                match parsed {
                    Some(frames) if frames > 0 => options.play_frames = Some(frames),
                    _ => {
                        eprintln!("--play-frames needs a positive integer\n\n{USAGE}");
                        return ExitCode::from(2);
                    }
                }
            }
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
            "-V" | "--version" => {
                // What the packaging script names the folder after.
                println!("giochi-editor {}", giochi_editor::VERSION);
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
