//! Arena com caixas a empilhar e um jogador em primeira pessoa com o controlador de
//! personagem do Rapier.
//!
//! O jogador é um corpo cinemático conduzido por [`PhysicsWorld3D::move_character`]: anda
//! para a frente até bater na parede, cai com gravidade e sobe um degrau. As caixas são
//! corpos dinâmicos que caem e se empilham. `--auto` conduz o jogador em frente sem
//! teclado e termina com uma linha de medição — é o modo usado pelo gate da fase.
//!
//! ```text
//! cargo run -p giochi-engine-3d --example physics_3d_demo --features physics-3d
//! cargo run -p giochi-engine-3d --example physics_3d_demo --features physics-3d -- \
//!     --frames 600 --auto
//! ```

use std::process::ExitCode;

use giochi_engine::platform::sdl::{quit_signal, QuitSignal};
use giochi_engine::platform::window::{Window, WindowConfig};
use giochi_engine::platform::Platform;
use giochi_engine_3d::context::{self, GlesVersion};
use giochi_engine_3d::math3d::{Camera3D, Color3D, Quat, Transform3D, Vec3};
use giochi_engine_3d::physics::{BodyDesc, ColliderShape, PhysicsWorld3D};
use giochi_engine_3d::shader::BuiltinShader;
use giochi_engine_3d::RenderContext3D;

const CUBE: f32 = 1.0;
const EYE_HEIGHT: f32 = 1.7;
const GRAVITY: f32 = -9.81;
const WALK_SPEED: f32 = 4.0;
/// The wall the walker must not pass through.
const WALL_X: f32 = 6.0;
const ARENA_HALF: f32 = 14.0;

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("erro: {error}");
            ExitCode::FAILURE
        }
    }
}

fn parse_options() -> Result<(Option<u64>, bool), String> {
    let mut frames = None;
    let mut auto = false;
    let mut arguments = std::env::args().skip(1).peekable();
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

fn run() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::init();
    let (frame_limit, auto) = parse_options()?;

    let mut world = PhysicsWorld3D::new(Vec3::new(0.0, GRAVITY, 0.0))?;
    let ground = world.create_body(BodyDesc::static_(Vec3::new(0.0, -0.5, 0.0)))?;
    world.attach_collider(
        ground,
        ColliderShape::cuboid(ARENA_HALF, 0.5, ARENA_HALF),
        0.9,
        0.0,
    )?;
    // The wall the acceptance criterion names: a walker that can pass through it is not
    // using physics at all.
    let wall = world.create_body(BodyDesc::static_(Vec3::new(WALL_X, 2.0, 0.0)))?;
    world.attach_collider(wall, ColliderShape::cuboid(0.5, 2.0, ARENA_HALF), 0.5, 0.0)?;
    // A 20 cm kerb to walk up: autostep is on, so a walker should climb it instead of
    // stopping dead.
    let kerb = world.create_body(BodyDesc::static_(Vec3::new(2.0, 0.1, -3.0)))?;
    world.attach_collider(kerb, ColliderShape::cuboid(1.5, 0.1, 1.5), 0.8, 0.0)?;

    // 20 crates is the number the phase's ARM64 acceptance criterion names.
    let mut crates = Vec::new();
    for index in 0..20u32 {
        let column = index % 4;
        let row = index / 4;
        let handle = world.create_body(BodyDesc::dynamic(Vec3::new(
            -4.0 + column as f32 * 1.1,
            2.0 + row as f32 * 1.1,
            0.0,
        )))?;
        world.attach_collider(handle, ColliderShape::cuboid(0.5, 0.5, 0.5), 0.6, 0.0)?;
        crates.push(handle);
    }

    let player = world.create_character(Vec3::new(0.0, 1.0, 0.0), 0.3, 0.6)?;

    let platform = Platform::new()?;
    let requested = GlesVersion::from_env();
    context::configure_attributes(&platform, requested)?;
    let window = Window::new_opengl(&platform, WindowConfig::new(1280, 720), "giochi physics 3D")?;
    let mut render = RenderContext3D::new(context::create(&window)?);
    println!(
        "GLES {} on {}",
        render.gles_version().major(),
        render.adapter()
    );

    let cube = render.create_cube_mesh(CUBE)?;
    let shader = render.create_builtin_shader(BuiltinShader::Flat)?;

    let mut frames: u64 = 0;
    let mut elapsed = 0.0f32;
    let mut grounded_ever = false;
    let mut blocked = false;

    loop {
        for event in platform.poll_events() {
            if quit_signal(&event) != QuitSignal::None {
                return finish(
                    &mut world,
                    &crates,
                    player,
                    frames,
                    elapsed,
                    blocked,
                    grounded_ever,
                );
            }
        }
        if frame_limit.is_some_and(|limit| frames >= limit) {
            return finish(
                &mut world,
                &crates,
                player,
                frames,
                elapsed,
                blocked,
                grounded_ever,
            );
        }

        let delta = 1.0 / 60.0;
        let desired = if auto {
            Vec3::new(WALK_SPEED * delta, -2.0 * delta, 0.0)
        } else {
            Vec3::ZERO
        };
        let movement = world.move_character(player, desired, delta)?;
        grounded_ever |= movement.grounded;
        world.step(delta)?;

        let player_position = world.position(player)?;
        // "Blocked" means the wall stopped the walker: it is past its start but short of
        // the wall's face.
        blocked |= player_position.x > 1.0 && player_position.x < WALL_X - 0.5;

        let camera = Camera3D {
            position: Vec3::new(
                player_position.x - 4.0,
                player_position.y + EYE_HEIGHT,
                player_position.z + 6.0,
            ),
            target: Vec3::new(
                player_position.x,
                player_position.y + 1.0,
                player_position.z,
            ),
            up: Vec3::UP,
            fov_y: 60f32.to_radians(),
            near_plane: 0.1,
            far_plane: 200.0,
            aspect: 16.0 / 9.0,
            _reserved: [0; 4],
        };
        let (width, height) = window.drawable_size();
        let camera = camera.with_drawable_size(width, height);

        render.begin_frame(&window);
        render.draw_mesh_colored(
            cube,
            shader,
            &Transform3D::from_rotation(Quat::IDENTITY)
                .with_scale(Vec3::new(ARENA_HALF * 2.0, 1.0, ARENA_HALF * 2.0))
                .with_position(Vec3::new(0.0, -0.5, 0.0)),
            &camera,
            Color3D::new(0.25, 0.28, 0.32, 1.0),
        )?;
        render.draw_mesh_colored(
            cube,
            shader,
            &Transform3D::from_rotation(Quat::IDENTITY)
                .with_scale(Vec3::new(1.0, 4.0, ARENA_HALF * 2.0))
                .with_position(Vec3::new(WALL_X, 2.0, 0.0)),
            &camera,
            Color3D::new(0.55, 0.35, 0.25, 1.0),
        )?;
        for (index, handle) in crates.iter().enumerate() {
            let transform = world.transform(*handle)?;
            let hue = (index as f32 * 0.15).sin() * 0.25 + 0.5;
            render.draw_mesh_colored(
                cube,
                shader,
                &Transform3D::from_rotation(transform.rotation).with_position(transform.position),
                &camera,
                Color3D::new(hue, 0.4 + hue * 0.3, 0.9 - hue * 0.4, 1.0),
            )?;
        }
        render.end_frame(&window);

        let error = render.take_gl_error();
        if error != 0 {
            return Err(format!("GL error 0x{error:04x} on frame {frames}").into());
        }
        frames += 1;
        elapsed += delta;
    }
}

fn finish(
    world: &mut PhysicsWorld3D,
    crates: &[u64],
    player: u64,
    frames: u64,
    elapsed: f32,
    blocked: bool,
    grounded: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let position = world.position(player)?;
    let mut awake = 0;
    let mut highest = 0.0f32;
    for handle in crates {
        let transform = world.transform(*handle)?;
        highest = highest.max(transform.position.y);
        if world.velocity(*handle)?.length() > 0.1 {
            awake += 1;
        }
    }
    println!(
        "physics_3d_demo: frames={frames} tempo={elapsed:.1}s fps={:.1} corpos={} jogador=({:.2},{:.2},{:.2}) \
         bloqueado_pela_parede={blocked} apoiado_no_chao={grounded} caixas_a_mover={awake} topo={:.2}",
        frames as f32 / elapsed.max(f32::EPSILON),
        world.body_count(),
        position.x,
        position.y,
        position.z,
        highest
    );
    Ok(())
}
