//! The `Scene` trait.

use crate::core::time::Time;
use crate::input::Input;
use crate::render::RenderContext;

/// Opaque scene identifier used at the C ABI boundary. `0` is invalid/null.
///
/// # C ABI
///
/// ```c
/// typedef uint64_t GiochiScene;
/// ```
pub type SceneHandle = u64;

/// A game state driven by the engine.
///
/// The engine owns the scene (`Box<dyn Scene>`) and calls [`Scene::update`] once
/// per fixed step, then [`Scene::render`] once per frame.
///
/// `Time` is passed by value (it is `Copy` and ABI-stable), which keeps the trait
/// usable from a C callback where the engine hands over a `const GiochiTime*`.
///
/// This trait is Rust-only: it never appears in the C ABI, where scenes are
/// handles plus a callback table.
pub trait Scene {
    /// Called once when the scene becomes the active scene.
    fn on_enter(&mut self) {}

    /// Called once when the scene leaves the stack.
    fn on_exit(&mut self) {}

    /// Advances the scene by one fixed step.
    fn update(&mut self, time: Time, input: &Input);

    /// Draws the frame.
    fn render(&mut self, renderer: &mut RenderContext);
}
