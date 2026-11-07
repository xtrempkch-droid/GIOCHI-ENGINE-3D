//! Scene stack management.

use crate::scene::scene::Scene;

/// A stack of scenes.
///
/// `push` overlays a new scene (useful for pause menus), `pop` returns to the
/// previous one and `replace` swaps the top of the stack. Only the top scene is
/// updated and rendered, which is what makes an overlay cheap: the scene below keeps
/// its state but costs nothing.
///
/// `on_enter`/`on_exit` are called exactly once per scene and always on the top of
/// the stack, so a scene can safely acquire resources in `on_enter` and release them
/// in `on_exit`.
///
/// # C ABI
///
/// Not exposed. C consumers use `giochi_scene_create`/`giochi_scene_destroy` plus
/// `giochi_engine_run_scene`; the stack is engine-owned (ADR-012). `#[repr(C)]` is
/// kept because R1 applies to every public struct in this crate.
#[repr(C)]
pub struct SceneManager {
    stack: Vec<Box<dyn Scene>>,
    _reserved: [u32; 4],
}

impl std::fmt::Debug for SceneManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SceneManager")
            .field("depth", &self.depth())
            .finish_non_exhaustive()
    }
}

impl Default for SceneManager {
    fn default() -> Self {
        Self::new()
    }
}

impl SceneManager {
    /// Creates an empty stack.
    pub fn new() -> Self {
        Self {
            stack: Vec::new(),
            _reserved: [0; 4],
        }
    }

    /// Pushes `scene` on top of the stack and calls its `on_enter`.
    ///
    /// The scene below (if any) is **not** told to exit: it stays alive and simply
    /// stops receiving updates until the new scene is popped.
    pub fn push(&mut self, mut scene: Box<dyn Scene>) {
        scene.on_enter();
        self.stack.push(scene);
    }

    /// Pops the top scene, calling its `on_exit` first.
    ///
    /// Returns `None` when the stack is empty. The scene below becomes active again
    /// without a second `on_enter`.
    pub fn pop(&mut self) -> Option<Box<dyn Scene>> {
        let mut scene = self.stack.pop()?;
        scene.on_exit();
        Some(scene)
    }

    /// Replaces the top scene (or pushes, when the stack is empty).
    ///
    /// The removed scene receives `on_exit`; the new one receives `on_enter`.
    pub fn replace(&mut self, mut scene: Box<dyn Scene>) {
        if let Some(mut previous) = self.stack.pop() {
            previous.on_exit();
        }
        scene.on_enter();
        self.stack.push(scene);
    }

    /// Empties the stack, calling `on_exit` from the top down.
    pub fn clear(&mut self) {
        while self.pop().is_some() {}
    }

    /// Immutable reference to the top scene, if any.
    pub fn current(&self) -> Option<&(dyn Scene + '_)> {
        match self.stack.last() {
            Some(scene) => Some(scene.as_ref()),
            None => None,
        }
    }

    /// Mutable reference to the top scene, if any.
    pub fn current_mut(&mut self) -> Option<&mut (dyn Scene + '_)> {
        match self.stack.last_mut() {
            Some(scene) => Some(scene.as_mut()),
            None => None,
        }
    }

    /// Number of scenes on the stack.
    pub fn depth(&self) -> usize {
        self.stack.len()
    }

    /// Returns `true` when the stack is empty.
    pub fn is_empty(&self) -> bool {
        self.stack.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::AudioSystem;
    use crate::core::time::Time;
    use crate::input::Input;
    use crate::render::RenderContext;

    /// Scene that records the order in which lifecycle callbacks arrive.
    #[derive(Clone)]
    struct Recorder {
        name: &'static str,
        log: std::rc::Rc<std::cell::RefCell<Vec<String>>>,
    }

    impl Recorder {
        fn new(
            name: &'static str,
            log: &std::rc::Rc<std::cell::RefCell<Vec<String>>>,
        ) -> Box<Self> {
            Box::new(Self {
                name,
                log: std::rc::Rc::clone(log),
            })
        }
    }

    impl Scene for Recorder {
        fn on_enter(&mut self) {
            self.log.borrow_mut().push(format!("enter:{}", self.name));
        }

        fn on_exit(&mut self) {
            self.log.borrow_mut().push(format!("exit:{}", self.name));
        }

        fn update(&mut self, _time: Time, _input: &Input, _audio: &mut AudioSystem) {
            self.log.borrow_mut().push(format!("update:{}", self.name));
        }

        fn render(&mut self, _renderer: &mut RenderContext) {
            self.log.borrow_mut().push(format!("render:{}", self.name));
        }
    }

    fn log() -> std::rc::Rc<std::cell::RefCell<Vec<String>>> {
        std::rc::Rc::new(std::cell::RefCell::new(Vec::new()))
    }

    #[test]
    fn a_new_stack_is_empty() {
        let manager = SceneManager::new();

        assert!(manager.is_empty());
        assert_eq!(manager.depth(), 0);
        assert!(manager.current().is_none());
        assert_eq!(manager._reserved, [0; 4]);
    }

    #[test]
    fn push_calls_on_enter_and_makes_the_scene_current() {
        let log = log();
        let mut manager = SceneManager::new();

        manager.push(Recorder::new("a", &log));

        assert_eq!(manager.depth(), 1);
        assert!(!manager.is_empty());
        assert_eq!(*log.borrow(), vec!["enter:a"]);
    }

    #[test]
    fn pop_calls_on_exit_and_restores_the_scene_below() {
        let log = log();
        let mut manager = SceneManager::new();
        manager.push(Recorder::new("a", &log));
        manager.push(Recorder::new("b", &log));

        let popped = manager.pop();

        assert!(popped.is_some());
        assert_eq!(manager.depth(), 1);
        assert_eq!(*log.borrow(), vec!["enter:a", "enter:b", "exit:b"]);
    }

    #[test]
    fn the_scene_below_is_not_re_entered_after_a_pop() {
        let log = log();
        let mut manager = SceneManager::new();
        manager.push(Recorder::new("a", &log));
        manager.push(Recorder::new("b", &log));
        manager.pop();

        assert_eq!(
            *log.borrow(),
            vec!["enter:a", "enter:b", "exit:b"],
            "only the popped scene may receive on_exit"
        );
    }

    #[test]
    fn pop_on_an_empty_stack_returns_none() {
        let mut manager = SceneManager::new();

        assert!(manager.pop().is_none());
        assert!(manager.is_empty());
    }

    #[test]
    fn replace_exits_the_previous_scene_and_enters_the_new_one() {
        let log = log();
        let mut manager = SceneManager::new();
        manager.push(Recorder::new("a", &log));

        manager.replace(Recorder::new("b", &log));

        assert_eq!(manager.depth(), 1, "replace must not grow the stack");
        assert_eq!(*log.borrow(), vec!["enter:a", "exit:a", "enter:b"]);
    }

    #[test]
    fn replace_on_an_empty_stack_pushes() {
        let log = log();
        let mut manager = SceneManager::new();

        manager.replace(Recorder::new("a", &log));

        assert_eq!(manager.depth(), 1);
        assert_eq!(*log.borrow(), vec!["enter:a"]);
    }

    #[test]
    fn only_the_top_scene_is_observable_as_current() {
        let log = log();
        let mut manager = SceneManager::new();
        manager.push(Recorder::new("a", &log));
        manager.push(Recorder::new("b", &log));

        // Driving the current scene must only affect the top of the stack.
        let mut input = Input::without_gamepad();
        input.update(&[]);

        let mut audio = AudioSystem::silent();
        if let Some(scene) = manager.current_mut() {
            scene.update(Time::new(), &input, &mut audio);
            assert_eq!(scene.transition(), crate::scene::SceneTransition::Continue);
        }

        assert_eq!(*log.borrow(), vec!["enter:a", "enter:b", "update:b"]);
    }

    #[test]
    fn clear_exits_every_scene_from_the_top_down() {
        let log = log();
        let mut manager = SceneManager::new();
        manager.push(Recorder::new("a", &log));
        manager.push(Recorder::new("b", &log));
        manager.push(Recorder::new("c", &log));
        log.borrow_mut().clear();

        manager.clear();

        assert!(manager.is_empty());
        assert_eq!(*log.borrow(), vec!["exit:c", "exit:b", "exit:a"]);
    }

    #[test]
    fn dropping_the_manager_does_not_run_on_exit() {
        let log = log();
        {
            let mut manager = SceneManager::new();
            manager.push(Recorder::new("a", &log));
            log.borrow_mut().clear();
        }

        assert!(
            log.borrow().is_empty(),
            "teardown is explicit; use clear() for on_exit"
        );
    }
}
