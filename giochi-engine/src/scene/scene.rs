//! The `Scene` trait.

use crate::audio::AudioSystem;
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

/// What a scene wants the engine to do after the current frame.
///
/// Returning a transition request is how a game changes state without the scene
/// having to own the stack: the engine applies it once the frame is fully drawn,
/// so a scene is never dropped in the middle of its own `render`.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SceneTransition {
    /// Keep running this scene.
    #[default]
    Continue = 0,
    /// The game is over: the engine stops after this frame.
    Quit = 1,
    /// Switch to another scene, replacing this one.
    ///
    /// The payload is applied by the engine when the scene is driven through
    /// [`crate::core::engine::Engine::run`]; a scene managed by hand can ignore it.
    Replace = 2,
    /// Push an overlay scene (a pause menu, a dialogue) on top of this one.
    Push = 3,
    /// Pop this scene, returning to the one below.
    Pop = 4,
}

/// A game state driven by the engine.
///
/// The engine owns the scene (`Box<dyn Scene>`) and calls [`Scene::update`] once per
/// fixed step, then [`Scene::render`] once per frame. `on_enter`/`on_exit` bracket
/// the scene's time on the stack, which is where a game loads and frees its
/// textures and sounds.
///
/// `Time` is passed by value (it is `Copy` and ABI-stable), mirroring the
/// `const GiochiTime*` the C ABI hands to a callback.
///
/// The engine owns the renderer and the audio system, so it lends them to the scene
/// instead of letting the scene hold them: `update` gets `&Input` and
/// `&mut AudioSystem`, `render` gets `&mut RenderContext`. No engine object can
/// outlive the engine that way.
///
/// This trait is Rust-only: it never appears in the C ABI, where scenes are handles
/// plus a callback table (see `docs/C_ABI_CONTRACT.md`, §3.8).
pub trait Scene {
    /// Called once when the scene becomes the active scene.
    ///
    /// Use it to load assets and initialise state. Failures cannot be reported from
    /// here; load lazily or panic in `update` if an asset is mandatory.
    fn on_enter(&mut self) {}

    /// Called once when the scene leaves the stack.
    ///
    /// Use it to destroy every handle the scene created.
    fn on_exit(&mut self) {}

    /// Advances the scene by one fixed step.
    ///
    /// `time.delta` is always the engine's fixed timestep
    /// ([`crate::core::game_loop::FIXED_TIMESTEP`] by default), so simulation stays
    /// deterministic regardless of the display rate. `input` is the snapshot for the
    /// frame; `pressed`/`released` edges are only meaningful here and in `render`,
    /// never across frames.
    ///
    /// `audio` is the engine's mixer: play effects and music through it. It may be in
    /// silent mode, in which case playback returns an error that a game is expected
    /// to ignore (ADR-021).
    fn update(&mut self, time: Time, input: &Input, audio: &mut AudioSystem);

    /// Draws the frame.
    ///
    /// The engine clears the screen **before** calling this and presents **after**,
    /// so a scene only issues draw calls. It is called once per display frame, which
    /// may be more or less often than `update`.
    fn render(&mut self, renderer: &mut RenderContext);

    /// Asks for a transition after this frame. Defaults to
    /// [`SceneTransition::Continue`].
    fn transition(&mut self) -> SceneTransition {
        SceneTransition::Continue
    }
}

/// Convenience: a scene that does nothing, used as a placeholder and in tests.
///
/// # C ABI
///
/// Not exposed; it exists to give the engine and the examples a no-op default.
#[repr(C)]
#[derive(Debug, Default)]
pub struct EmptyScene {
    entered: u32,
    exited: u32,
    updated: u32,
    rendered: u32,
    _reserved: [u32; 4],
}

impl EmptyScene {
    /// Creates a scene that counts the callbacks it receives.
    pub const fn new() -> Self {
        Self {
            entered: 0,
            exited: 0,
            updated: 0,
            rendered: 0,
            _reserved: [0; 4],
        }
    }

    /// Number of times `on_enter` ran.
    pub fn entered(&self) -> u32 {
        self.entered
    }

    /// Number of times `on_exit` ran.
    pub fn exited(&self) -> u32 {
        self.exited
    }

    /// Number of fixed updates the scene received.
    pub fn updated(&self) -> u32 {
        self.updated
    }

    /// Number of render calls the scene received.
    pub fn rendered(&self) -> u32 {
        self.rendered
    }
}

impl Scene for EmptyScene {
    fn on_enter(&mut self) {
        self.entered = self.entered.saturating_add(1);
    }

    fn on_exit(&mut self) {
        self.exited = self.exited.saturating_add(1);
    }

    fn update(&mut self, _time: Time, _input: &Input, _audio: &mut AudioSystem) {
        self.updated = self.updated.saturating_add(1);
    }

    fn render(&mut self, _renderer: &mut RenderContext) {
        self.rendered = self.rendered.saturating_add(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_transition_is_continue() {
        assert_eq!(SceneTransition::default(), SceneTransition::Continue);
    }

    #[test]
    fn transition_values_are_stable_for_the_c_abi() {
        assert_eq!(SceneTransition::Continue as i32, 0);
        assert_eq!(SceneTransition::Quit as i32, 1);
        assert_eq!(SceneTransition::Replace as i32, 2);
        assert_eq!(SceneTransition::Push as i32, 3);
        assert_eq!(SceneTransition::Pop as i32, 4);
    }

    #[test]
    fn an_empty_scene_starts_at_zero_and_zeroes_its_reserved_tail() {
        let scene = EmptyScene::new();

        assert_eq!(scene.entered(), 0);
        assert_eq!(scene.exited(), 0);
        assert_eq!(scene.updated(), 0);
        assert_eq!(scene.rendered(), 0);
        assert_eq!(scene._reserved, [0; 4]);
    }

    #[test]
    fn default_callbacks_do_nothing() {
        struct Minimal;

        impl Scene for Minimal {
            fn update(&mut self, _time: Time, _input: &Input, _audio: &mut AudioSystem) {}
            fn render(&mut self, _renderer: &mut RenderContext) {}
        }

        let mut scene = Minimal;
        scene.on_enter();
        scene.on_exit();
        assert_eq!(scene.transition(), SceneTransition::Continue);
    }
}
