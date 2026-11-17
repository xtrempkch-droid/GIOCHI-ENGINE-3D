//! `editor_default_scene` — writes the scene `File > New Scene` creates, then reads it
//! back through the file format.
//!
//! The plan asked for this example as the acceptance of phase 31's "New Scene": the same
//! three objects (cube, sun, camera) must survive a save/load round trip, and the output
//! is what the smoke check greps.
//!
//! ```text
//! cargo run -p giochi-editor --example editor_default_scene
//! cargo run -p giochi-editor --example editor_default_scene -- /tmp/x.scene
//! ```

use std::process::ExitCode;

use giochi_engine_3d::scene::{Scene, SCENE_FORMAT_VERSION};

fn main() -> ExitCode {
    // Relative to the crate, not to the working directory: `cargo run` starts in the
    // workspace root, and a default that wrote there would scatter `.scene` files.
    let path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| format!("{}/assets/scenes/default.scene", env!("CARGO_MANIFEST_DIR")));

    let scene = Scene::default_scene();
    if let Err(error) = scene.save_to_path(&path) {
        eprintln!("erro ao gravar {path}: {error}");
        return ExitCode::FAILURE;
    }

    let loaded = match Scene::load_from_path(&path) {
        Ok(loaded) => loaded,
        Err(error) => {
            eprintln!("erro ao ler {path}: {error}");
            return ExitCode::FAILURE;
        }
    };

    println!(
        "scena={} formato={} versao={} nos={}",
        loaded.name,
        loaded.format,
        loaded.version,
        loaded.nodes.len()
    );
    for node in &loaded.nodes {
        let parent = node
            .parent
            .map(|parent| format!(" parent={parent}"))
            .unwrap_or_default();
        println!(
            "no id={} tipo={} nome={}{parent} pos=({:.2},{:.2},{:.2})",
            node.id,
            node.kind(),
            node.name,
            node.transform.position[0],
            node.transform.position[1],
            node.transform.position[2]
        );
    }
    match loaded.mesh_bounds() {
        Some((min, max)) => println!(
            "limites=({:.2},{:.2},{:.2})..({:.2},{:.2},{:.2})",
            min[0], min[1], min[2], max[0], max[1], max[2]
        ),
        None => println!("limites=nenhum"),
    }

    let ok = loaded.format == "giochi-scene"
        && loaded.version == SCENE_FORMAT_VERSION
        && loaded == scene
        && loaded.nodes.len() == 3;
    if ok {
        println!("roundtrip=ok");
        ExitCode::SUCCESS
    } else {
        eprintln!("roundtrip=divergente");
        ExitCode::FAILURE
    }
}
