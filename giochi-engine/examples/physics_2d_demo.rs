//! Torre de caixas a cair, com `drag` e `raycast` do Rapier 2D.
//!
//! Espaço cria uma caixa no cursor, arrastar o botão esquerdo puxa o corpo sob o cursor,
//! `R` reinicia a torre. `--auto` arrasta uma caixa uma vez e fica em silêncio, para que
//! a torre assente e a medida de estabilidade signifique alguma coisa; `--frames N`
//! termina sozinho — é o modo usado pelo gate da fase.
//!
//! ```text
//! cargo run -p giochi-engine --example physics_2d_demo --features physics-2d
//! cargo run -p giochi-engine --example physics_2d_demo --features physics-2d -- \
//!     --frames 1800 --auto
//! ```
//!
//! A cada segundo o demo imprime a maior velocidade de qualquer caixa nesse segundo. Uma
//! torre adormecida fica abaixo de `0.05`; um solucionador instável produz valores na casa
//! das unidades.

use std::process::ExitCode;

use giochi_engine::input::keyboard::{KEY_R, KEY_SPACE};
use giochi_engine::input::mouse::MOUSE_LEFT;
use giochi_engine::physics::{BodyDesc, ColliderShape, PhysicsWorld2D};
use giochi_engine::{
    AudioSystem, Camera, Color, Engine, EngineConfig, Input, Rect, RenderContext, Scene, Time, Vec2,
};

const WIDTH: u32 = 960;
const HEIGHT: u32 = 540;
const GRAVITY: f32 = -30.0;
const BOX: f32 = 28.0;
const FLOOR_Y: f32 = 60.0;

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("erro: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let options = Options::parse(std::env::args().skip(1))?;
    let mut config = EngineConfig::new(WIDTH, HEIGHT, "giochi — physics_2d_demo");
    if options.frames.is_some() {
        config.target_fps = 0;
        config.vsync = 0;
    }
    let mut engine = Engine::new(config)?;
    if let Some(frames) = options.frames {
        engine.set_frame_limit(frames);
    }
    let camera = Camera::with_position(
        Rect::new(0.0, 0.0, WIDTH as f32, HEIGHT as f32),
        WIDTH as f32 * 0.5,
        HEIGHT as f32 * 0.5,
    );
    engine.render_mut().set_camera(camera);

    let scene = PhysicsScene::new(camera, options.auto)?;
    engine.push_scene(Box::new(scene));
    engine.run_current()?;

    let stats = engine.run_stats();
    println!(
        "physics_2d_demo: frames={} fps={:.1}",
        stats.frames,
        stats.fps()
    );
    Ok(())
}

/// A world, the boxes in it, and the drag state.
struct PhysicsScene {
    world: PhysicsWorld2D,
    camera: Camera,
    /// One colour per body, so a falling tower is readable with no sprite at all.
    boxes: Vec<(u64, Color)>,
    auto: bool,
    elapsed: f32,
    dragged: Option<u64>,
    /// Offset from the body centre to the cursor when the drag started, so grabbing an
    /// edge does not snap the box to the middle of the mouse.
    grab_offset: Vec2,
    drag_until: f32,
    next_grab: f32,
    peak_speed: f32,
    peak_window: f32,
}

impl PhysicsScene {
    fn new(camera: Camera, auto: bool) -> Result<Self, giochi_engine::physics::PhysicsError> {
        let mut scene = PhysicsScene {
            world: PhysicsWorld2D::new(Vec2::new(0.0, GRAVITY))?,
            camera,
            boxes: Vec::new(),
            auto,
            elapsed: 0.0,
            dragged: None,
            grab_offset: Vec2::ZERO,
            drag_until: 0.0,
            next_grab: 2.0,
            peak_speed: 0.0,
            peak_window: 0.0,
        };
        scene.build_level()?;
        Ok(scene)
    }

    fn build_level(&mut self) -> Result<(), giochi_engine::physics::PhysicsError> {
        // Floor and two walls: a box that leaves the screen is a bug the demo should not
        // be able to hide.
        let floor = self.world.create_body(BodyDesc::static_(Vec2::new(
            WIDTH as f32 * 0.5,
            FLOOR_Y * 0.5,
        )))?;
        self.world.attach_collider(
            floor,
            ColliderShape::boxed(WIDTH as f32 * 0.5, FLOOR_Y * 0.5),
            0.9,
            0.0,
        )?;
        for x in [0.0, WIDTH as f32] {
            let wall = self
                .world
                .create_body(BodyDesc::static_(Vec2::new(x, HEIGHT as f32 * 0.5)))?;
            self.world.attach_collider(
                wall,
                ColliderShape::boxed(10.0, HEIGHT as f32 * 0.5),
                0.4,
                0.0,
            )?;
        }

        self.spawn_tower()
    }

    fn spawn_tower(&mut self) -> Result<(), giochi_engine::physics::PhysicsError> {
        for row in 0..7u32 {
            let offset = if row % 2 == 0 { 0.0 } else { BOX * 0.5 };
            self.spawn_box(Vec2::new(
                WIDTH as f32 * 0.5 + offset,
                FLOOR_Y + BOX * 0.5 + row as f32 * (BOX + 1.0),
            ))?;
        }
        Ok(())
    }

    fn spawn_box(&mut self, position: Vec2) -> Result<u64, giochi_engine::physics::PhysicsError> {
        let body = self.world.create_body(BodyDesc::dynamic(position))?;
        self.world
            .attach_collider(body, ColliderShape::boxed(BOX * 0.5, BOX * 0.5), 0.7, 0.05)?;
        let index = self.boxes.len() as u32;
        let hue = (index * 47) % 200;
        self.boxes.push((
            body,
            Color::new((60 + hue) as u8, (200 - hue / 2) as u8, 120, 255),
        ));
        Ok(body)
    }

    fn is_box(&self, handle: u64) -> bool {
        self.boxes.iter().any(|(body, _)| *body == handle)
    }

    fn restart(&mut self) -> Result<(), giochi_engine::physics::PhysicsError> {
        self.world = PhysicsWorld2D::new(Vec2::new(0.0, GRAVITY))?;
        self.boxes.clear();
        self.dragged = None;
        self.build_level()
    }

    /// Picks the box under the cursor with a short downward ray.
    ///
    /// A ray rather than a point query because that is what the phase publishes, and a
    /// downward cast from just above the cursor is a point query that also proves the
    /// raycast works.
    fn try_grab(&mut self, cursor: Vec2) {
        let origin = Vec2::new(cursor.x, cursor.y + 1.0);
        match self.world.raycast(origin, Vec2::new(0.0, -1.0), 4.0) {
            Ok(Some(hit)) if self.is_box(hit.body) => {
                println!(
                    "raycast: corpo {} a {:.2} unidades do cursor",
                    hit.body, hit.distance
                );
                let position = self.world.position(hit.body).unwrap_or(cursor);
                self.grab_offset = Vec2::new(position.x - cursor.x, position.y - cursor.y);
                self.dragged = Some(hit.body);
            }
            Ok(Some(hit)) => println!("raycast: corpo {} não é arrastável", hit.body),
            Ok(None) => println!("raycast: nada sob o cursor"),
            Err(error) => log::error!("raycast falhou: {error}"),
        }
    }

    fn release(&mut self) {
        if let Some(handle) = self.dragged.take() {
            // A dragged body has no velocity of its own; zeroing it on release is what
            // stops a box from being launched by the last teleport.
            if let Err(error) = self.world.set_velocity(handle, Vec2::ZERO) {
                log::error!("não foi possível soltar o corpo: {error}");
            }
        }
    }

    fn drag_to(&mut self, cursor: Vec2) {
        if let Some(handle) = self.dragged {
            let target = Vec2::new(cursor.x + self.grab_offset.x, cursor.y + self.grab_offset.y);
            if let Err(error) = self.world.set_position(handle, target) {
                log::error!("não foi possível arrastar o corpo: {error}");
            }
        }
    }

    /// One deterministic drag, then silence.
    ///
    /// `--auto` exists to be measurable, so it deliberately does *not* keep dropping
    /// boxes: after the automated drag the tower has to settle, and the stability line at
    /// the end of a long run is what says whether it did.
    fn update_auto(&mut self) {
        if self.dragged.is_none() && self.elapsed >= self.next_grab {
            if let Some((handle, _)) = self.boxes.last().copied() {
                self.dragged = Some(handle);
                self.grab_offset = Vec2::ZERO;
                self.drag_until = self.elapsed + 1.0;
                self.next_grab = f32::MAX;
            }
        }
        if self.dragged.is_some() {
            if self.elapsed >= self.drag_until {
                self.release();
            } else {
                let target = Vec2::new(
                    WIDTH as f32 * 0.5 + (self.elapsed * 3.0).sin() * 140.0,
                    HEIGHT as f32 * 0.55 + (self.elapsed * 2.0).cos() * 60.0,
                );
                self.drag_to(target);
            }
        }
    }

    /// The largest speed among the boxes, printed once per simulated second.
    ///
    /// This is the number the phase's acceptance criterion asks for: a stack that is
    /// correctly asleep reports a peak of 0, and a solver fighting itself reports a
    /// constant non-zero speed forever.
    fn track_stability(&mut self, delta: f32) {
        let mut peak = 0.0f32;
        for (handle, _) in &self.boxes {
            if let Ok(velocity) = self.world.velocity(*handle) {
                peak = peak.max(velocity.length());
            }
        }
        self.peak_speed = self.peak_speed.max(peak);
        self.peak_window += delta;
        if self.peak_window >= 1.0 {
            println!(
                "estabilidade: velocidade máxima no último segundo = {:.3}",
                self.peak_speed
            );
            self.peak_speed = 0.0;
            self.peak_window = 0.0;
        }
    }
}

impl Scene for PhysicsScene {
    fn update(&mut self, time: Time, input: &Input, _audio: &mut AudioSystem) {
        self.elapsed += time.delta;
        let cursor = self.camera.screen_to_world(input.mouse().position_vec());

        if self.auto {
            self.update_auto();
        } else {
            if input.keyboard().is_pressed_this_frame(KEY_R) {
                if let Err(error) = self.restart() {
                    log::error!("não foi possível reiniciar: {error}");
                }
                return;
            }
            if input.keyboard().is_pressed_this_frame(KEY_SPACE) {
                if let Err(error) = self.spawn_box(cursor) {
                    log::error!("não foi possível criar a caixa: {error}");
                }
            }
            if input.mouse().is_pressed_this_frame(MOUSE_LEFT) {
                self.try_grab(cursor);
            }
            if self.dragged.is_some() {
                if input.mouse_down(MOUSE_LEFT) {
                    self.drag_to(cursor);
                } else {
                    self.release();
                }
            }
        }

        if let Err(error) = self.world.step(time.delta) {
            log::error!("não foi possível avançar a física: {error}");
        }
        self.track_stability(time.delta);
    }

    fn render(&mut self, renderer: &mut RenderContext) {
        renderer.clear(Color::new(18, 20, 26, 255));
        renderer.draw_rect(
            Rect::new(0.0, 0.0, WIDTH as f32, FLOOR_Y),
            Color::new(70, 80, 95, 255),
        );

        let camera = self.camera;
        let half = BOX * 0.5;
        for (handle, colour) in &self.boxes {
            let Ok(transform) = self.world.transform(*handle) else {
                continue;
            };
            let centre = camera.world_to_screen(transform.position);
            renderer.draw_rect(
                Rect::new(centre.x - half, centre.y - half, BOX, BOX),
                *colour,
            );
        }

        if let Some(handle) = self.dragged {
            if let Ok(transform) = self.world.transform(handle) {
                let centre = camera.world_to_screen(transform.position);
                renderer.draw_line(
                    Vec2::new(centre.x, centre.y),
                    Vec2::new(centre.x, centre.y + 18.0),
                    Color::new(255, 240, 120, 255),
                );
            }
        }
    }
}

struct Options {
    frames: Option<u64>,
    auto: bool,
}

impl Options {
    fn parse(arguments: impl Iterator<Item = String>) -> Result<Self, String> {
        let mut options = Self {
            frames: None,
            auto: false,
        };
        let mut arguments = arguments.peekable();
        while let Some(argument) = arguments.next() {
            match argument.as_str() {
                "--auto" => options.auto = true,
                "--frames" => {
                    let value = arguments
                        .next()
                        .ok_or_else(|| "--frames precisa de um valor".to_owned())?;
                    options.frames = Some(
                        value
                            .parse()
                            .map_err(|_| format!("--frames inválido: {value}"))?,
                    );
                }
                other => return Err(format!("argumento desconhecido: {other}")),
            }
        }
        if options.frames == Some(0) {
            return Err("--frames 0 não desenha nada".to_owned());
        }
        Ok(options)
    }
}
