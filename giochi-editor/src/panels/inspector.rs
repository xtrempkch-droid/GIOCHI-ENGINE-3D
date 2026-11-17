//! The inspector: the selected node's properties, edited in place.
//!
//! Phase 33 is the editor's MVP. Everything before it could show a scene; this is the
//! first panel that can *change* one, and the acceptance is a scene built entirely in the
//! editor, saved and reopened.
//!
//! # One clone per frame
//!
//! The panel clones the selected node out of the scene, draws editors over the clone, and
//! writes it back when something changed. That is a `SceneNode` copy per frame — a name,
//! a transform, a handful of options — and it is what keeps the borrow checker out of the
//! UI code: a `&mut` into `state.scene` cannot be held while `state.status` and
//! `state.dirty` are also being written. Performance is explicitly not critical here
//! (R7).
//!
//! # What is not here
//!
//! - **Undo**: phase 37. Until then an edit is immediate and final, which is why the
//!   dirty flag exists from phase 31.
//! - **PBR fields** (`emissive`, `roughness`): the scene format has `specular` and
//!   `shininess`, which is what the GLES 2.0 renderer actually consumes. A `roughness`
//!   slider that changed nothing on screen would be worse than its absence; the PBR-lite
//!   material is part of the Production Pack still pending (ADR-063).

use giochi_engine_3d::scene::{
    CameraDesc, LightDesc, MaterialDesc, MeshSource, NodeTransform, Primitive, SceneNode,
};

use crate::editor::command::SetNode;
use crate::editor::{EditorState, EditorTheme};

/// What the inspector changed this frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct InspectorOutcome {
    /// The node's properties changed and were written back.
    pub changed: bool,
    /// The node was selected from the header (the primary selection).
    pub selected: Option<giochi_engine_3d::scene::SceneNodeId>,
}

/// Draws the inspector and applies whatever changed.
pub fn draw(ui: &mut egui::Ui, state: &mut EditorState, theme: &EditorTheme) -> InspectorOutcome {
    let Some(primary) = state.primary() else {
        hint(
            ui,
            "Nothing selected. Click an object in the hierarchy or the viewport.",
            theme,
        );
        return InspectorOutcome::default();
    };
    if state.selected.len() > 1 {
        ui.label(
            egui::RichText::new(format!("{} objects selected", state.selected.len()))
                .strong()
                .color(theme.accent),
        );
        ui.label(
            egui::RichText::new("editing the primary selection")
                .small()
                .color(theme.muted),
        );
        ui.separator();
    }

    let Some(node) = state
        .scene
        .as_ref()
        .and_then(|scene| scene.node(primary))
        .cloned()
    else {
        hint(ui, "The selected object is no longer in the scene.", theme);
        return InspectorOutcome::default();
    };

    let mut edited = node.clone();
    let mut changed = false;

    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            header(ui, &edited, theme);

            egui::CollapsingHeader::new("Transform")
                .default_open(true)
                .show(ui, |ui| {
                    changed |= transform_editor(ui, &mut edited.transform);
                });

            egui::CollapsingHeader::new("Mesh")
                .default_open(true)
                .show(ui, |ui| {
                    changed |= mesh_editor(ui, &mut edited.mesh);
                });

            egui::CollapsingHeader::new("Material")
                .default_open(true)
                .show(ui, |ui| {
                    changed |= material_editor(ui, &mut edited.material);
                });

            if edited.is_light() {
                egui::CollapsingHeader::new("Light")
                    .default_open(true)
                    .show(ui, |ui| {
                        if let Some(light) = &mut edited.light {
                            changed |= light_editor(ui, light);
                        }
                    });
            }

            if edited.is_camera() {
                egui::CollapsingHeader::new("Camera")
                    .default_open(true)
                    .show(ui, |ui| {
                        if let Some(camera) = &mut edited.camera {
                            changed |= camera_editor(ui, camera);
                        }
                    });
            }
        });

    let changed = changed && edited != node;
    if changed {
        // A `DragValue` reports a change on every frame of a drag. The command is pushed
        // mergeable while the pointer is down or a field has focus, so the whole nudge is one
        // undo step instead of sixty (ADR-069).
        let continuing = state.scene.is_some()
            && (ui.ctx().input(|input| input.pointer.any_down())
                || ui.ctx().wants_keyboard_input());
        state.apply_command_mergeable(
            Box::new(SetNode::new(node, edited, "Change node")),
            continuing,
        );
    }

    InspectorOutcome {
        changed,
        selected: None,
    }
}

fn header(ui: &mut egui::Ui, node: &SceneNode, theme: &EditorTheme) {
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(&node.name).strong());
        ui.label(
            egui::RichText::new(format!("{} {}", node.kind(), node.id))
                .small()
                .color(theme.muted),
        );
    });
    ui.separator();
}

fn hint(ui: &mut egui::Ui, text: &str, theme: &EditorTheme) {
    ui.add_space(6.0);
    ui.label(egui::RichText::new(text).color(theme.muted).small());
}

// ---- Transform ---------------------------------------------------------------

/// Position, rotation and scale. Returns whether anything changed.
pub fn transform_editor(ui: &mut egui::Ui, transform: &mut NodeTransform) -> bool {
    let mut changed = false;

    ui.label("position");
    changed |= vec3_editor(ui, &mut transform.position, 0.1, "pos");

    ui.label("rotation (Euler, degrees)");
    let mut euler = quat_to_euler_degrees(transform.rotation);
    let before = euler;
    ui.horizontal(|ui| {
        for (axis, label) in ["X", "Y", "Z"].iter().enumerate() {
            ui.label(*label);
            changed |= ui
                .add(
                    egui::DragValue::new(&mut euler[axis])
                        .speed(1.0)
                        .range(-360.0..=360.0)
                        .suffix("°"),
                )
                .changed();
        }
    });
    if euler != before {
        transform.rotation = euler_degrees_to_quat(euler);
        changed = true;
    }

    ui.label("scale");
    changed |= vec3_editor(ui, &mut transform.scale, 0.1, "scale");
    if transform.scale.contains(&0.0) {
        ui.label(
            egui::RichText::new("a zero scale makes the object invisible")
                .small()
                .color(egui::Color32::from_rgb(0xd8, 0xa8, 0x40)),
        );
    }

    changed
}

fn vec3_editor(ui: &mut egui::Ui, values: &mut [f32; 3], speed: f64, id: &str) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        for (axis, label) in ["X", "Y", "Z"].iter().enumerate() {
            ui.label(*label);
            changed |= ui
                .add(
                    egui::DragValue::new(&mut values[axis])
                        .speed(speed)
                        .max_decimals(3),
                )
                .changed();
        }
        let _ = (id, &mut changed);
    });
    changed
}

/// Quaternion `[x, y, z, w]` → Euler `[x, y, z]` in degrees, XYZ order.
pub fn quat_to_euler_degrees(quaternion: [f32; 4]) -> [f32; 3] {
    let q = glam::Quat::from_array(quaternion);
    let (x, y, z) = q.to_euler(glam::EulerRot::XYZ);
    [x.to_degrees(), y.to_degrees(), z.to_degrees()]
}

/// Euler `[x, y, z]` in degrees, XYZ order → quaternion `[x, y, z, w]`.
pub fn euler_degrees_to_quat(euler: [f32; 3]) -> [f32; 4] {
    let q = glam::Quat::from_euler(
        glam::EulerRot::XYZ,
        euler[0].to_radians(),
        euler[1].to_radians(),
        euler[2].to_radians(),
    );
    [q.x, q.y, q.z, q.w]
}

// ---- Mesh --------------------------------------------------------------------

/// The mesh source: a primitive kind with its parameters, or an OBJ path.
pub fn mesh_editor(ui: &mut egui::Ui, mesh: &mut Option<MeshSource>) -> bool {
    let mut changed = false;
    let mut kind = match mesh {
        None => "none",
        Some(MeshSource::Primitive(Primitive::Cube { .. })) => "cube",
        Some(MeshSource::Primitive(Primitive::Sphere { .. })) => "sphere",
        Some(MeshSource::Primitive(Primitive::Plane { .. })) => "plane",
        Some(MeshSource::Obj(_)) => "obj",
    }
    .to_owned();

    egui::ComboBox::from_id_salt("inspector-mesh-kind")
        .selected_text(&kind)
        .show_ui(ui, |ui| {
            for option in ["none", "cube", "sphere", "plane", "obj"] {
                changed |= ui
                    .selectable_value(&mut kind, option.to_owned(), option)
                    .changed();
            }
        });

    if changed {
        *mesh = match kind.as_str() {
            "cube" => Some(MeshSource::Primitive(Primitive::Cube { size: 1.0 })),
            "sphere" => Some(MeshSource::Primitive(Primitive::Sphere {
                radius: 0.5,
                rings: 16,
                segments: 24,
            })),
            "plane" => Some(MeshSource::Primitive(Primitive::Plane {
                size: 10.0,
                subdivisions: 1,
            })),
            "obj" => Some(MeshSource::Obj("model.obj".to_owned())),
            _ => None,
        };
    }

    match mesh {
        Some(MeshSource::Primitive(Primitive::Cube { size })) => {
            changed |= ui
                .add(
                    egui::DragValue::new(size)
                        .speed(0.1)
                        .range(0.01..=1000.0)
                        .prefix("size "),
                )
                .changed();
        }
        Some(MeshSource::Primitive(Primitive::Sphere {
            radius,
            rings,
            segments,
        })) => {
            changed |= ui
                .add(
                    egui::DragValue::new(radius)
                        .speed(0.05)
                        .range(0.01..=1000.0)
                        .prefix("radius "),
                )
                .changed();
            changed |= ui
                .add(
                    egui::DragValue::new(rings)
                        .speed(0.2)
                        .range(3..=128)
                        .prefix("rings "),
                )
                .changed();
            changed |= ui
                .add(
                    egui::DragValue::new(segments)
                        .speed(0.2)
                        .range(3..=128)
                        .prefix("segments "),
                )
                .changed();
        }
        Some(MeshSource::Primitive(Primitive::Plane { size, subdivisions })) => {
            changed |= ui
                .add(
                    egui::DragValue::new(size)
                        .speed(0.2)
                        .range(0.01..=1000.0)
                        .prefix("size "),
                )
                .changed();
            changed |= ui
                .add(
                    egui::DragValue::new(subdivisions)
                        .speed(0.2)
                        .range(1..=128)
                        .prefix("subdivisions "),
                )
                .changed();
        }
        Some(MeshSource::Obj(path)) => {
            changed |= ui.text_edit_singleline(path).changed();
        }
        None => {}
    }
    changed
}

// ---- Material ----------------------------------------------------------------

/// Base colour, texture, specular strength and exponent.
pub fn material_editor(ui: &mut egui::Ui, material: &mut MaterialDesc) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.label("base colour");
        changed |= ui
            .color_edit_button_rgba_unmultiplied(&mut material.base_color)
            .changed();
    });
    ui.horizontal(|ui| {
        ui.label("texture");
        let mut texture = material.texture.clone().unwrap_or_default();
        if ui.text_edit_singleline(&mut texture).changed() {
            material.texture = if texture.trim().is_empty() {
                None
            } else {
                Some(texture)
            };
            changed = true;
        }
    });
    // Phase 34: a texture dragged from the asset browser lands here. `dnd_drop_zone`
    // reports the payload once the pointer is released over the slot, which is the whole
    // of "drag a texture onto the material".
    let (_, dropped) = ui.dnd_drop_zone::<crate::panels::asset_browser::AssetDrag, _>(
        egui::Frame::NONE.fill(ui.visuals().faint_bg_color),
        |ui| {
            ui.label(
                egui::RichText::new(match &material.texture {
                    Some(texture) => format!("drop a texture here (current: {texture})"),
                    None => "drop a texture here".to_owned(),
                })
                .small()
                .color(ui.visuals().weak_text_color()),
            );
        },
    );
    if let Some(drag) = dropped {
        if drag.kind == crate::project::AssetKind::Texture {
            material.texture = Some(drag.relative.clone());
            changed = true;
        }
    }
    changed |= ui
        .add(
            egui::Slider::new(&mut material.specular, 0.0..=1.0)
                .text("specular")
                .fixed_decimals(2),
        )
        .changed();
    changed |= ui
        .add(
            egui::Slider::new(&mut material.shininess, 1.0..=128.0)
                .text("shininess")
                .logarithmic(true),
        )
        .changed();
    changed
}

// ---- Light -------------------------------------------------------------------

/// The directional light the engine renders with.
pub fn light_editor(ui: &mut egui::Ui, light: &mut LightDesc) -> bool {
    let mut changed = false;
    ui.label("direction (the way the light travels)");
    changed |= read_only_vec3(ui, &mut light.direction, 0.05);
    ui.horizontal(|ui| {
        ui.label("colour");
        changed |= ui
            .color_edit_button_rgba_unmultiplied(&mut light.color)
            .changed();
    });
    changed |= ui
        .add(egui::Slider::new(&mut light.ambient, 0.0..=1.0).text("ambient"))
        .changed();
    changed |= ui
        .add(egui::Slider::new(&mut light.specular, 0.0..=2.0).text("specular"))
        .changed();
    changed |= ui
        .add(
            egui::Slider::new(&mut light.shininess, 1.0..=256.0)
                .text("shininess")
                .logarithmic(true),
        )
        .changed();
    changed
}

fn read_only_vec3(ui: &mut egui::Ui, values: &mut [f32; 3], speed: f64) -> bool {
    vec3_editor(ui, values, speed, "vec3")
}

// ---- Camera ------------------------------------------------------------------

/// The scene camera a game plays through. Never the viewport's (`OrbitCamera`).
pub fn camera_editor(ui: &mut egui::Ui, camera: &mut CameraDesc) -> bool {
    let mut changed = false;
    ui.label("position");
    changed |= read_only_vec3(ui, &mut camera.position, 0.1);
    ui.label("target");
    changed |= read_only_vec3(ui, &mut camera.target, 0.1);
    ui.label("up");
    changed |= read_only_vec3(ui, &mut camera.up, 0.1);

    let mut fov_degrees = camera.fov_y.to_degrees();
    if ui
        .add(
            egui::Slider::new(&mut fov_degrees, 10.0..=170.0)
                .text("fov")
                .suffix("°"),
        )
        .changed()
    {
        camera.fov_y = fov_degrees.to_radians();
        changed = true;
    }
    changed |= ui
        .add(
            egui::DragValue::new(&mut camera.near)
                .speed(0.01)
                .range(0.001..=1000.0)
                .prefix("near "),
        )
        .changed();
    changed |= ui
        .add(
            egui::DragValue::new(&mut camera.far)
                .speed(1.0)
                .range(0.01..=1_000_000.0)
                .prefix("far "),
        )
        .changed();
    if camera.far <= camera.near {
        ui.label(
            egui::RichText::new("far must be greater than near")
                .small()
                .color(egui::Color32::from_rgb(0xff, 0x6b, 0x6b)),
        );
    }
    changed
}

#[cfg(test)]
mod tests {
    use super::*;
    use giochi_engine_3d::scene::{Scene, SceneNodeId};

    fn editor_ui(mut body: impl FnMut(&mut egui::Ui)) {
        let ctx = egui::Context::default();
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| body(ui));
        });
    }

    fn scene_with_selection() -> EditorState {
        let mut state = EditorState::new();
        state.new_scene();
        state.select(Some(SceneNodeId(1)));
        state
    }

    #[test]
    fn euler_and_quaternion_round_trip() {
        for euler in [
            [0.0, 0.0, 0.0],
            [30.0, 0.0, 0.0],
            [0.0, 45.0, 0.0],
            [0.0, 0.0, -60.0],
            [15.0, 25.0, 35.0],
        ] {
            let quaternion = euler_degrees_to_quat(euler);
            let back = quat_to_euler_degrees(quaternion);
            for axis in 0..3 {
                assert!(
                    (euler[axis] - back[axis]).abs() < 1e-2,
                    "{euler:?} -> {back:?}"
                );
            }
        }
    }

    #[test]
    fn the_transform_editor_reports_no_change_without_input() {
        let mut transform = NodeTransform::default();
        let before = transform;
        editor_ui(|ui| {
            assert!(!transform_editor(ui, &mut transform));
        });
        assert_eq!(transform, before);
    }

    #[test]
    fn the_material_editor_reports_no_change_without_input() {
        let mut material = MaterialDesc::default();
        let before = material.clone();
        editor_ui(|ui| {
            assert!(!material_editor(ui, &mut material));
        });
        assert_eq!(material, before);
    }

    #[test]
    fn the_light_editor_reports_no_change_without_input() {
        let mut light = LightDesc::default();
        let before = light;
        editor_ui(|ui| {
            assert!(!light_editor(ui, &mut light));
        });
        assert_eq!(light, before);
    }

    #[test]
    fn the_camera_editor_reports_no_change_without_input() {
        let mut camera = CameraDesc::default();
        let before = camera;
        editor_ui(|ui| {
            assert!(!camera_editor(ui, &mut camera));
        });
        assert_eq!(camera, before);
    }

    #[test]
    fn the_mesh_editor_offers_every_kind_and_changes_nothing_by_itself() {
        let mut mesh = Some(MeshSource::Primitive(Primitive::Cube { size: 1.0 }));
        let before = mesh.clone();
        editor_ui(|ui| {
            assert!(!mesh_editor(ui, &mut mesh));
        });
        assert_eq!(mesh, before);

        let mut none = None;
        editor_ui(|ui| {
            assert!(!mesh_editor(ui, &mut none));
        });
        assert_eq!(none, None);
    }

    #[test]
    fn drawing_the_inspector_with_a_selection_does_not_dirty_the_scene() {
        let mut state = scene_with_selection();
        editor_ui(|ui| {
            let outcome = draw(ui, &mut state, &EditorTheme::dark());
            assert!(!outcome.changed);
        });
        assert!(!state.dirty);
        assert!(state.scene.is_some());
    }

    #[test]
    fn drawing_without_a_selection_changes_nothing() {
        let mut state = EditorState::new();
        state.new_scene();
        editor_ui(|ui| {
            let outcome = draw(ui, &mut state, &EditorTheme::dark());
            assert_eq!(outcome, InspectorOutcome::default());
        });
        assert!(!state.dirty);
    }

    #[test]
    fn a_multi_selection_says_how_many_and_edits_the_primary() {
        let mut state = scene_with_selection();
        state.toggle_selection(SceneNodeId(2));
        assert_eq!(state.selected.len(), 2);
        editor_ui(|ui| {
            let outcome = draw(ui, &mut state, &EditorTheme::dark());
            assert!(!outcome.changed);
        });
        assert_eq!(state.primary(), Some(SceneNodeId(1)));
    }

    #[test]
    fn an_edit_through_the_state_dirty_the_scene_and_survives_a_save() {
        // The MVP path, without a mouse: the inspector writes a modified node back, and
        // the file keeps it.
        let path =
            std::env::temp_dir().join(format!("giochi-inspector-mvp-{}.scene", std::process::id()));
        let mut state = scene_with_selection();
        if let Some(scene) = state.scene.as_mut() {
            let mut node = scene.node(SceneNodeId(1)).expect("cube").clone();
            node.transform.position = [2.5, 1.5, -0.5];
            node.material.base_color = [1.0, 0.2, 0.1, 1.0];
            assert!(scene.replace_node(node));
        }
        state.mark_dirty();
        assert!(state.dirty);
        state.save_scene_as(&path).expect("saved");

        let mut reopened = EditorState::new();
        reopened.open_scene(&path).expect("reopened");
        let node = reopened
            .scene
            .as_ref()
            .and_then(|scene| scene.node(SceneNodeId(1)))
            .expect("cube after reload");
        assert_eq!(node.transform.position, [2.5, 1.5, -0.5]);
        assert_eq!(node.material.base_color, [1.0, 0.2, 0.1, 1.0]);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn every_kind_of_node_has_a_section_that_draws() {
        let mut state = EditorState::new();
        state.new_scene();
        for id in [SceneNodeId(1), SceneNodeId(2), SceneNodeId(3)] {
            state.select(Some(id));
            editor_ui(|ui| {
                let _ = draw(ui, &mut state, &EditorTheme::dark());
            });
            assert!(!state.dirty, "drawing alone must not mark the scene dirty");
        }
        assert_eq!(state.scene.as_ref().map(|scene| scene.nodes.len()), Some(3));
        assert!(Scene::default_scene().first_camera().is_some());
    }
}
