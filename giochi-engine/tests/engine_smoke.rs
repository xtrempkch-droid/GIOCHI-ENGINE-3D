//! Headless integration test for the `Engine` facade and the scene stack.
//!
//! Runs against SDL's `dummy` video driver. The engine's own loop is exercised end
//! to end: fixed steps, rendering, transitions and the frame limit.
//!
//! One test per binary: SDL allows a single `EventPump` per process.

use std::cell::RefCell;
use std::rc::Rc;

use giochi_engine::{
    codes, AudioSystem, EmptyScene, Engine, EngineConfig, Input, RenderContext, Scene,
    SceneTransition, Time,
};

/// Records what the engine did, so the test can assert on the real call order.
#[derive(Clone, Default)]
struct Log(Rc<RefCell<Vec<String>>>);

impl Log {
    fn push(&self, entry: impl Into<String>) {
        self.0.borrow_mut().push(entry.into());
    }

    fn entries(&self) -> Vec<String> {
        self.0.borrow().clone()
    }
}

/// Scene that counts its callbacks and optionally asks to quit.
struct Probe {
    name: &'static str,
    log: Log,
    quit_after_updates: Option<u32>,
    updates: u32,
    renders: u32,
}

impl Probe {
    fn new(name: &'static str, log: &Log) -> Box<Self> {
        Box::new(Self {
            name,
            log: log.clone(),
            quit_after_updates: None,
            updates: 0,
            renders: 0,
        })
    }

    fn quitting_after(name: &'static str, log: &Log, updates: u32) -> Box<Self> {
        Box::new(Self {
            name,
            log: log.clone(),
            quit_after_updates: Some(updates),
            updates: 0,
            renders: 0,
        })
    }
}

impl Scene for Probe {
    fn on_enter(&mut self) {
        self.log.push(format!("enter:{}", self.name));
    }

    fn on_exit(&mut self) {
        self.log.push(format!("exit:{}", self.name));
    }

    fn update(&mut self, _time: Time, _input: &Input, audio: &mut AudioSystem) {
        self.updates += 1;
        // The scene must be able to reach audio; a silent engine errors, which a
        // game ignores.
        let _ = audio.is_available();
        self.log.push(format!("update:{}", self.name));
    }

    fn render(&mut self, renderer: &mut RenderContext) {
        self.renders += 1;
        let (width, height) = renderer.size();
        renderer.draw_rect(
            giochi_engine::Rect::new(0.0, 0.0, width as f32, height as f32),
            giochi_engine::Color::rgb(20, 30, 40),
        );
        self.log.push(format!("render:{}", self.name));
    }

    fn transition(&mut self) -> SceneTransition {
        match self.quit_after_updates {
            Some(limit) if self.updates >= limit => SceneTransition::Quit,
            _ => SceneTransition::Continue,
        }
    }
}

fn engine_with_dummy_driver() -> Engine {
    std::env::set_var("SDL_VIDEODRIVER", "dummy");
    let mut engine =
        Engine::new(EngineConfig::new(160, 120, "giochi-engine-test")).expect("engine must start");
    // With `profiling`, the overlay owns one panel texture; the leak assertion at the
    // end counts engine textures and must not include a debug aid.
    engine.set_overlay_visible(false);
    engine
}

#[test]
fn engine_and_scene_stack_without_a_display() {
    let log = Log::default();
    let mut engine = engine_with_dummy_driver();

    // --- invalid configuration is rejected before SDL is touched ------------
    let error = Engine::new(EngineConfig::new(0, 120, "bad")).expect_err("must fail");
    assert_eq!(error.code(), codes::INVALID_ARGUMENT);

    // --- the stack starts empty --------------------------------------------
    assert!(engine.scenes().is_empty());
    assert_eq!(engine.run_stats().frames, 0);

    // --- pushing calls on_enter, popping calls on_exit ---------------------
    engine.push_scene(Probe::new("a", &log));
    assert_eq!(engine.scenes().depth(), 1);
    assert_eq!(log.entries(), vec!["enter:a"]);

    engine.push_scene(Probe::new("b", &log));
    assert_eq!(engine.scenes().depth(), 2);
    assert_eq!(log.entries(), vec!["enter:a", "enter:b"]);

    assert!(engine.pop_scene());
    assert_eq!(log.entries(), vec!["enter:a", "enter:b", "exit:b"]);
    assert_eq!(engine.scenes().depth(), 1);

    // --- a replace runs exit then enter ------------------------------------
    engine.replace_scene(Probe::new("c", &log));
    assert_eq!(engine.scenes().depth(), 1);
    assert_eq!(
        log.entries(),
        vec!["enter:a", "enter:b", "exit:b", "exit:a", "enter:c"]
    );

    // --- the real loop: fixed steps, rendering, frame limit ----------------
    log.0.borrow_mut().clear();

    // Time the "caller" spends between building the engine and starting the loop — asset
    // loading, a title screen, a debug breakpoint. It must not be integrated as if it had
    // been a frame. `FrameClock` starts at `Engine::new`, so `run_current` has to restart
    // it: without that the first `tick()` returned this sleep plus the whole construction
    // cost, and the run began by simulating up to `MAX_FRAME_TIME` (15 steps at 60 Hz) of a
    // frame that never happened. It also made `elapsed` and `fps()` count set-up as play.
    //
    // One frame is run so the assertion is about the first frame alone: a longer run would
    // charge the first frame's *work* to the second one, which is what a frame loop is
    // supposed to do.
    std::thread::sleep(std::time::Duration::from_millis(200));

    engine.set_frame_limit(1);
    engine.run_current().expect("loop must run");

    let startup = engine.run_stats();
    assert_eq!(startup.frames, 1);
    assert_eq!(
        startup.steps, 0,
        "the first frame integrated {} fixed steps: the 200 ms spent before the loop was \
         charged to it",
        startup.steps
    );

    log.0.borrow_mut().clear();
    engine.set_frame_limit(40);
    engine.run_current().expect("loop must run");

    let stats = engine.run_stats();
    assert_eq!(stats.frames, 40, "the frame limit must be honoured");
    assert!(stats.steps > 0, "the fixed loop must produce steps");
    assert!(stats.elapsed > 0.0);
    assert!(stats.fps() > 0.0);

    let entries = log.entries();
    let updates = entries.iter().filter(|e| e.as_str() == "update:c").count();
    let renders = entries.iter().filter(|e| e.as_str() == "render:c").count();
    assert_eq!(renders, 40, "one render per display frame");
    assert_eq!(
        u64::from(updates as u32),
        stats.steps,
        "one update per fixed step"
    );

    // --- a scene asking to quit stops the loop ----------------------------
    engine.reset_quit();
    engine.clear_frame_limit();
    let quit_log = Log::default();
    engine.replace_scene(Probe::quitting_after("q", &quit_log, 3));
    engine.run_current().expect("loop must run");

    assert!(quit_log.entries().iter().any(|e| e == "update:q"));
    assert!(engine.run_stats().frames > 0);
    assert!(!engine.is_running(), "a Quit transition must stop the loop");

    // --- an empty stack keeps running and drawing -------------------------
    engine.reset_quit();
    engine.set_frame_limit(5);
    while engine.pop_scene() {}
    assert!(engine.scenes().is_empty());
    engine.run_current().expect("an empty stack is legal");
    assert_eq!(engine.run_stats().frames, 5);

    // --- EmptyScene is usable and counts its callbacks ---------------------
    engine.reset_quit();
    engine.set_frame_limit(3);
    engine.push_scene(Box::new(EmptyScene::new()));
    engine.run_current().expect("loop must run");
    assert_eq!(engine.run_stats().frames, 3);

    // --- resources and audio stay consistent ------------------------------
    assert_eq!(engine.render().texture_count(), 0, "nothing may leak");
    assert_eq!(engine.audio().sound_count(), 0);
    assert!(engine.fixed_timestep() > 0.0);
}
