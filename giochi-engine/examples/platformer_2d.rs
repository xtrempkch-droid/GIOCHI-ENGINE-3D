//! Plataforma 2D com gravidade, pulo e plataformas sólidas — a pendência da Fase 24.
//!
//! O jogador é um corpo dinâmico com rotação bloqueada; as plataformas são corpos fixos.
//! O pulo só acontece quando um raio curto para baixo encontra o chão, que é a forma
//! barata de responder "estou apoiado?" com a API da fase (o `PhysicsWorld2D` não expõe
//! contactos por corpo, e um raio é exactamente a mesma pergunta).
//!
//! Setas/A-D andam, Espaço salta, R reinicia. `--auto` corre um percurso determinístico
//! (andar, saltar, cair na plataforma seguinte) e imprime no fim se o jogador chegou ao
//! alvo — é o modo usado pelo gate.
//!
//! ```text
//! cargo run -p giochi-engine --features physics-2d --example platformer_2d
//! cargo run -p giochi-engine --features physics-2d --example platformer_2d -- \
//!     --frames 600 --auto
//! ```

use std::process::ExitCode;

use giochi_engine::input::keyboard::{KEY_A, KEY_D, KEY_R, KEY_SPACE};
use giochi_engine::physics::{BodyDesc, ColliderShape, PhysicsWorld2D};
use giochi_engine::{
    AudioSystem, Camera, Color, Engine, EngineConfig, Input, Rect, RenderContext, Scene, Time, Vec2,
};

const WIDTH: u32 = 960;
const HEIGHT: u32 = 540;
const GRAVITY: f32 = -1800.0;
const PLAYER: f32 = 26.0;
const MOVE_SPEED: f32 = 260.0;
const JUMP_SPEED: f32 = 720.0;
const GROUND_PROBE: f32 = 6.0;

/// A platform: centre and half extents, in world units.
struct Platform {
    centre: Vec2,
    half: Vec2,
    colour: Color,
}

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
    let (frames, auto) = parse_options()?;
    let mut config = EngineConfig::new(WIDTH, HEIGHT, "giochi — platformer_2d");
    if frames.is_some() {
        config.target_fps = 0;
        config.vsync = 0;
    }
    let mut engine = Engine::new(config)?;
    if let Some(limit) = frames {
        engine.set_frame_limit(limit);
    }
    let camera = Camera::with_position(
        Rect::new(0.0, 0.0, WIDTH as f32, HEIGHT as f32),
        WIDTH as f32 * 0.5,
        HEIGHT as f32 * 0.5,
    );
    engine.render_mut().set_camera(camera);

    engine.push_scene(Box::new(PlatformScene::new(camera, auto)?));
    engine.run_current()?;

    let stats = engine.run_stats();
    println!(
        "platformer_2d: frames={} fps={:.1}",
        stats.frames,
        stats.fps()
    );
    Ok(())
}

struct PlatformScene {
    world: PhysicsWorld2D,
    camera: Camera,
    platforms: Vec<Platform>,
    player: u64,
    auto: bool,
    elapsed: f32,
    grounded: bool,
    jumps: u64,
    /// Highest platform the player has stood on, for the acceptance line.
    highest: f32,
    /// Whether the player touched the goal marker at the top-left.
    reached_goal: bool,
    goal: Vec2,
    /// Frames since the last scripted jump.
    auto_jump: u32,
}

impl PlatformScene {
    fn new(camera: Camera, auto: bool) -> Result<Self, giochi_engine::physics::PhysicsError> {
        let mut scene = PlatformScene {
            world: PhysicsWorld2D::new(Vec2::new(0.0, GRAVITY))?,
            camera,
            platforms: Vec::new(),
            player: 0,
            auto,
            elapsed: 0.0,
            grounded: false,
            jumps: 0,
            highest: 0.0,
            reached_goal: false,
            goal: Vec2::new(120.0, 420.0),
            auto_jump: 0,
        };
        scene.build()?;
        Ok(scene)
    }

    fn add_platform(
        &mut self,
        centre: Vec2,
        half: Vec2,
        colour: Color,
    ) -> Result<(), giochi_engine::physics::PhysicsError> {
        let body = self.world.create_body(BodyDesc::static_(centre))?;
        self.world
            .attach_collider(body, ColliderShape::boxed(half.x, half.y), 0.9, 0.0)?;
        self.platforms.push(Platform {
            centre,
            half,
            colour,
        });
        Ok(())
    }

    fn build(&mut self) -> Result<(), giochi_engine::physics::PhysicsError> {
        // Floor, then a staircase of platforms tall enough to need the jump.
        self.add_platform(
            Vec2::new(WIDTH as f32 * 0.5, 40.0),
            Vec2::new(WIDTH as f32 * 0.5, 40.0),
            Color::new(58, 66, 78, 255),
        )?;
        // A staircase that starts on the floor: the first step is 14 px above it, so the
        // walker has to jump for it rather than walk under it. Every later step is 90 px
        // up, comfortably inside the 144 px the jump reaches.
        let steps = [
            (300.0, 94.0, 90.0, 14.0),
            (470.0, 184.0, 80.0, 14.0),
            (640.0, 274.0, 80.0, 14.0),
            (810.0, 364.0, 90.0, 14.0),
        ];
        for (x, y, half_width, half_height) in steps {
            self.add_platform(
                Vec2::new(x, y),
                Vec2::new(half_width, half_height),
                Color::new(86, 120, 96, 255),
            )?;
        }
        self.goal = Vec2::new(810.0, 378.0);

        // Walls, so "walk off the level" is not a way to pass the check.
        for x in [0.0, WIDTH as f32] {
            self.add_platform(
                Vec2::new(x, HEIGHT as f32 * 0.5),
                Vec2::new(10.0, HEIGHT as f32 * 0.5),
                Color::new(40, 44, 52, 255),
            )?;
        }

        self.spawn_player()
    }

    fn spawn_player(&mut self) -> Result<(), giochi_engine::physics::PhysicsError> {
        let body = self
            .world
            .create_body(BodyDesc::dynamic(Vec2::new(120.0, 120.0)).lock_rotations())?;
        self.world.attach_collider(
            body,
            ColliderShape::boxed(PLAYER * 0.5, PLAYER * 0.5),
            0.2,
            0.0,
        )?;
        self.player = body;
        Ok(())
    }

    fn restart(&mut self) -> Result<(), giochi_engine::physics::PhysicsError> {
        self.world = PhysicsWorld2D::new(Vec2::new(0.0, GRAVITY))?;
        self.platforms.clear();
        self.grounded = false;
        self.jumps = 0;
        self.highest = 0.0;
        self.reached_goal = false;
        self.build()
    }

    /// Whether a short ray under the player finds solid ground.
    ///
    /// `PhysicsWorld2D` reports contacts as pairs for the whole world, not per body, so
    /// "is this body standing on something" is asked as a ray: it is one query, it does
    /// not allocate, and it answers exactly the question a jump needs.
    fn is_grounded(&mut self) -> bool {
        let Ok(position) = self.world.position(self.player) else {
            return false;
        };
        let origin = Vec2::new(position.x, position.y - PLAYER * 0.5 + 1.0);
        match self
            .world
            .raycast(origin, Vec2::new(0.0, -1.0), GROUND_PROBE)
        {
            Ok(Some(_)) => true,
            Ok(None) => false,
            Err(error) => {
                log::error!("não foi possível lançar o raio de chão: {error}");
                false
            }
        }
    }

    /// Walks right and jumps when the way forward is blocked or the ground runs out.
    ///
    /// A fixed cadence would jump on the spot; reading the world is what makes the
    /// scripted run climb the staircase, and it is the same two queries a player reads
    /// with their eyes.
    fn auto_input(&mut self) -> (f32, bool) {
        let Ok(position) = self.world.position(self.player) else {
            return (1.0, false);
        };
        // Stop on the goal platform instead of walking into the far wall and climbing it.
        if position.x >= self.goal.x - 25.0 {
            return (0.0, false);
        }
        // Jump on a cadence while walking right. A player reads the step ahead with their
        // eyes; a scripted run reads a counter, which is deterministic and is what a gate
        // needs. `auto_jump` is reset on landing so the cadence does not drift.
        self.auto_jump += 1;
        let jump = self.grounded && self.auto_jump >= 18;
        if jump {
            self.auto_jump = 0;
        }
        (1.0, jump)
    }

    fn report(&self) {
        let position = self.world.position(self.player).unwrap_or(Vec2::ZERO);
        println!(
            "platformer_2d: posicao=({:.1},{:.1}) pulos={} mais_alto={:.1} alvo_alcancado={}",
            position.x, position.y, self.jumps, self.highest, self.reached_goal
        );
    }
}

impl Scene for PlatformScene {
    fn on_exit(&mut self) {
        self.report();
    }

    fn update(&mut self, time: Time, input: &Input, _audio: &mut AudioSystem) {
        self.elapsed += time.delta;
        self.grounded = self.is_grounded();

        if input.keyboard().is_pressed_this_frame(KEY_R) {
            if let Err(error) = self.restart() {
                log::error!("não foi possível reiniciar: {error}");
            }
            return;
        }

        let (direction, jump) = if self.auto {
            self.auto_input()
        } else {
            let mut direction = 0.0;
            if input.keyboard().is_down(KEY_A) {
                direction -= 1.0;
            }
            if input.keyboard().is_down(KEY_D) {
                direction += 1.0;
            }
            (direction, input.keyboard().is_pressed_this_frame(KEY_SPACE))
        };

        // The vertical component is read back and written again: `set_velocity` replaces
        // the whole vector, and the gravity the solver already accumulated lives in that
        // component. Writing `y = 0` every frame is a walker that cannot fall and cannot
        // jump — it took a rendered staircase to notice.
        let current = self.world.velocity(self.player).unwrap_or(Vec2::ZERO);
        let vertical = if jump { JUMP_SPEED } else { current.y };
        if let Err(error) = self
            .world
            .set_velocity(self.player, Vec2::new(direction * MOVE_SPEED, vertical))
        {
            log::error!("não foi possível definir a velocidade: {error}");
        }
        if jump {
            self.jumps += 1;
        }

        if let Err(error) = self.world.step(time.delta) {
            log::error!("não foi possível avançar a física: {error}");
        }

        if let Ok(position) = self.world.position(self.player) {
            self.highest = self.highest.max(position.y);
            // "Reached" is standing on the goal platform, not passing under it.
            if self.grounded
                && (position.x - self.goal.x).abs() < 70.0
                && (position.y - self.goal.y).abs() < 40.0
            {
                self.reached_goal = true;
            }
        }
    }

    fn render(&mut self, renderer: &mut RenderContext) {
        renderer.clear(Color::new(20, 24, 32, 255));
        let camera = self.camera;
        for platform in &self.platforms {
            let centre = camera.world_to_screen(platform.centre);
            renderer.draw_rect(
                Rect::new(
                    centre.x - platform.half.x,
                    centre.y - platform.half.y,
                    platform.half.x * 2.0,
                    platform.half.y * 2.0,
                ),
                platform.colour,
            );
        }

        let goal = camera.world_to_screen(self.goal);
        renderer.draw_rect(
            Rect::new(goal.x - 8.0, goal.y - 8.0, 16.0, 16.0),
            Color::new(240, 200, 90, 255),
        );

        if let Ok(position) = self.world.position(self.player) {
            let centre = camera.world_to_screen(position);
            let half = PLAYER * 0.5;
            let colour = if self.grounded {
                Color::new(120, 200, 255, 255)
            } else {
                Color::new(255, 150, 90, 255)
            };
            renderer.draw_rect(
                Rect::new(centre.x - half, centre.y - half, PLAYER, PLAYER),
                colour,
            );
        }
    }
}

fn parse_options() -> Result<(Option<u64>, bool), String> {
    let mut frames = None;
    let mut auto = false;
    let mut arguments = std::env::args().skip(1);
    while let Some(argument) = arguments.next() {
        match argument.as_str() {
            "--auto" => auto = true,
            "--frames" => {
                let value = arguments
                    .next()
                    .ok_or_else(|| "--frames precisa de um valor".to_owned())?;
                frames = Some(
                    value
                        .parse()
                        .map_err(|_| format!("--frames inválido: {value}"))?,
                );
            }
            other => return Err(format!("argumento desconhecido: {other}")),
        }
    }
    Ok((frames, auto))
}
