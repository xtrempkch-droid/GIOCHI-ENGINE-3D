//! Física 2D — Part V da ABI C.
//!
//! Expõe [`giochi_engine::physics`] a consumidores C: um mundo, corpos com descrição,
//! colliders de caixa, passo fixo e leitura de posição. É o que um teste de fumo C
//! precisa para provar que a física existe do outro lado da fronteira — um corpo cai,
//! a posição muda e o mundo avança em 60 Hz, não no tempo do frame.
//!
//! # Handles
//!
//! O mundo é um ponteiro opaco (`GiochiPhysicsWorld *`), como o motor 3D (ADR-039): a
//! struct tem um Rapier por dentro, cujo tamanho muda entre versões da dependência e não
//! é assunto do header. Os corpos são `uint64_t` gerados pelo motor e nunca reutilizados;
//! `0` nunca é válido.
//!
//! # Por que sempre compilado
//!
//! As fases 19–21 têm features porque trazem `notify` e `egui`. Esta parte não: um
//! `libgiochi.so` sem física obrigaria o header a mudar com a feature, e o header é um
//! contrato. O custo é o Rapier 2D no binário (~1 MB), medido em `docs/PERF_LOG.md`.

use giochi_engine::physics::{BodyDesc, BodyKind, ColliderShape, PhysicsWorld2D};
use giochi_engine::Vec2;

use crate::abi_types::GIOCHI_ERROR_INVALID_ARGUMENT;
use crate::state::{guard, set_last_error};

/// Um mundo físico 2D, opaco para o C.
///
/// Criado por `giochi_physics_world_create` e destruído por
/// `giochi_physics_world_destroy`; cada mundo é independente, ao contrário do motor 2D,
/// que é singleton (ADR-012).
pub struct GiochiPhysicsWorld {
    world: PhysicsWorld2D,
}

fn world<'a>(pointer: *mut GiochiPhysicsWorld) -> Option<&'a mut GiochiPhysicsWorld> {
    // SAFETY: the contract is that the pointer came from `giochi_physics_world_create`
    // and has not been destroyed; NULL is checked first, which is the case a caller can
    // recover from.
    unsafe { pointer.as_mut() }
}

fn report(error: giochi_engine::physics::PhysicsError) -> i32 {
    set_last_error(&error.to_string());
    GIOCHI_ERROR_INVALID_ARGUMENT
}

/// Creates a physics world with the given gravity and a 60 Hz fixed step.
///
/// Returns `NULL` on a non-finite gravity; `giochi_get_last_error()` explains.
#[no_mangle]
pub extern "C" fn giochi_physics_world_create(
    gravity_x: f32,
    gravity_y: f32,
) -> *mut GiochiPhysicsWorld {
    guard(std::ptr::null_mut(), || {
        match PhysicsWorld2D::new(Vec2::new(gravity_x, gravity_y)) {
            Ok(world) => Box::into_raw(Box::new(GiochiPhysicsWorld { world })),
            Err(error) => {
                report(error);
                std::ptr::null_mut()
            }
        }
    })
}

/// Destroys a physics world and every body in it.
///
/// Returns `GIOCHI_ERROR_INVALID_ARGUMENT` for a `NULL` world. A pointer used after this
/// call is undefined behaviour, as with every other `giochi_*_destroy`.
#[no_mangle]
pub extern "C" fn giochi_physics_world_destroy(pointer: *mut GiochiPhysicsWorld) -> i32 {
    guard(GIOCHI_ERROR_INVALID_ARGUMENT, || {
        if pointer.is_null() {
            set_last_error("the physics world pointer is NULL");
            return GIOCHI_ERROR_INVALID_ARGUMENT;
        }
        // SAFETY: rebuilt from the pointer `create` produced; the contract says the caller
        // destroys it exactly once.
        drop(unsafe { Box::from_raw(pointer) });
        0
    })
}

/// Creates a body and writes its handle to `out_handle`.
///
/// `kind` is `0` dynamic, `1` fixed and `2` kinematic; anything else is refused. Returns
/// `0` on success.
#[no_mangle]
pub extern "C" fn giochi_physics_create_body(
    pointer: *mut GiochiPhysicsWorld,
    kind: u32,
    x: f32,
    y: f32,
    rotation: f32,
    out_handle: *mut u64,
) -> i32 {
    guard(GIOCHI_ERROR_INVALID_ARGUMENT, || {
        let Some(world) = world(pointer) else {
            set_last_error("the physics world pointer is NULL");
            return GIOCHI_ERROR_INVALID_ARGUMENT;
        };
        if out_handle.is_null() {
            set_last_error("out_handle is NULL");
            return GIOCHI_ERROR_INVALID_ARGUMENT;
        }
        let desc = match kind {
            0 => BodyDesc::dynamic(Vec2::new(x, y)),
            1 => BodyDesc::static_(Vec2::new(x, y)),
            2 => BodyDesc::kinematic(Vec2::new(x, y)),
            other => {
                set_last_error(&format!("unknown body kind {other}"));
                return GIOCHI_ERROR_INVALID_ARGUMENT;
            }
        }
        .with_rotation(rotation);
        match world.world.create_body(desc) {
            Ok(handle) => {
                // SAFETY: `out_handle` is non-NULL and points at a placeable `uint64_t`.
                unsafe { *out_handle = handle };
                0
            }
            Err(error) => report(error),
        }
    })
}

/// Gives `body` a box collider with the given half extents.
#[no_mangle]
pub extern "C" fn giochi_physics_attach_box(
    pointer: *mut GiochiPhysicsWorld,
    body: u64,
    half_width: f32,
    half_height: f32,
    friction: f32,
    restitution: f32,
) -> i32 {
    guard(GIOCHI_ERROR_INVALID_ARGUMENT, || {
        let Some(world) = world(pointer) else {
            set_last_error("the physics world pointer is NULL");
            return GIOCHI_ERROR_INVALID_ARGUMENT;
        };
        match world.world.attach_collider(
            body,
            ColliderShape::boxed(half_width, half_height),
            friction,
            restitution,
        ) {
            Ok(()) => 0,
            Err(error) => report(error),
        }
    })
}

/// Advances the world by `dt` seconds.
///
/// Returns `0` on success, like every other fallible entry point. The number of 60 Hz
/// steps the accumulator ran is deliberately **not** the return value: a positive number
/// would break "0 means success", which is the convention the whole header rests on.
#[no_mangle]
pub extern "C" fn giochi_physics_step(pointer: *mut GiochiPhysicsWorld, dt: f32) -> i32 {
    guard(GIOCHI_ERROR_INVALID_ARGUMENT, || {
        let Some(world) = world(pointer) else {
            set_last_error("the physics world pointer is NULL");
            return GIOCHI_ERROR_INVALID_ARGUMENT;
        };
        match world.world.step(dt) {
            Ok(_steps) => 0,
            Err(error) => report(error),
        }
    })
}

/// Reads the body's position into `out_x` and `out_y`.
#[no_mangle]
pub extern "C" fn giochi_physics_get_position(
    pointer: *mut GiochiPhysicsWorld,
    body: u64,
    out_x: *mut f32,
    out_y: *mut f32,
) -> i32 {
    guard(GIOCHI_ERROR_INVALID_ARGUMENT, || {
        let Some(world) = world(pointer) else {
            set_last_error("the physics world pointer is NULL");
            return GIOCHI_ERROR_INVALID_ARGUMENT;
        };
        if out_x.is_null() || out_y.is_null() {
            set_last_error("out_x or out_y is NULL");
            return GIOCHI_ERROR_INVALID_ARGUMENT;
        }
        match world.world.position(body) {
            Ok(position) => {
                // SAFETY: both pointers were checked for NULL above.
                unsafe {
                    *out_x = position.x;
                    *out_y = position.y;
                }
                0
            }
            Err(error) => report(error),
        }
    })
}

/// Applies an impulse to the body at its centre of mass.
#[no_mangle]
pub extern "C" fn giochi_physics_apply_impulse(
    pointer: *mut GiochiPhysicsWorld,
    body: u64,
    impulse_x: f32,
    impulse_y: f32,
) -> i32 {
    guard(GIOCHI_ERROR_INVALID_ARGUMENT, || {
        let Some(world) = world(pointer) else {
            set_last_error("the physics world pointer is NULL");
            return GIOCHI_ERROR_INVALID_ARGUMENT;
        };
        match world
            .world
            .apply_impulse(body, Vec2::new(impulse_x, impulse_y))
        {
            Ok(()) => 0,
            Err(error) => report(error),
        }
    })
}

/// Destroys a body and its collider.
#[no_mangle]
pub extern "C" fn giochi_physics_destroy_body(pointer: *mut GiochiPhysicsWorld, body: u64) -> i32 {
    guard(GIOCHI_ERROR_INVALID_ARGUMENT, || {
        let Some(world) = world(pointer) else {
            set_last_error("the physics world pointer is NULL");
            return GIOCHI_ERROR_INVALID_ARGUMENT;
        };
        match world.world.destroy_body(body) {
            Ok(()) => 0,
            Err(error) => report(error),
        }
    })
}

/// Number of live bodies in the world, or `0` for a `NULL` world.
#[no_mangle]
pub extern "C" fn giochi_physics_body_count(pointer: *mut GiochiPhysicsWorld) -> u32 {
    guard(0, || {
        world(pointer)
            .map(|world| world.world.body_count() as u32)
            .unwrap_or(0)
    })
}

const _: () = assert!(BodyKind::Dynamic as u32 == 0);
const _: () = assert!(BodyKind::Fixed as u32 == 1);
const _: () = assert!(BodyKind::KinematicPosition as u32 == 2);
