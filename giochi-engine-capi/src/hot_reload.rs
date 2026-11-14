//! Hot reload — Part IX da ABI C.
//!
//! Como a consola, cada função é **uma** definição com o `#[cfg]` dentro do corpo: o
//! header não pode mudar com as features de quem compilou o `.so`. Sem a feature
//! `hot_reload` as funções devolvem
//! [`GIOCHI_ERROR_UNSUPPORTED`] e explicam
//! como recompilar.
//!
//! O caminho aceite é uma pasta (ou um ficheiro) que o C quer ver recarregado; o engine
//! observa recursivamente e aplica as texturas que já conhece.

use core::ffi::c_char;

#[cfg(feature = "hot_reload")]
use crate::abi_types::GIOCHI_ERROR_INVALID_ARGUMENT;
use crate::abi_types::GIOCHI_ERROR_UNSUPPORTED;
#[cfg(feature = "hot_reload")]
use crate::state;
use crate::state::{guard, set_last_error};

/// Message reported when the build has no watcher.
#[cfg(not(feature = "hot_reload"))]
const NO_WATCHER: &str = "this libgiochi was built without the `hot_reload` feature; \
     rebuild with --features hot_reload";

/// Starts watching `path` for changes.
///
/// Returns the number of paths being watched (`>= 1`) on success, or a negative error
/// code. `GIOCHI_ERROR_UNSUPPORTED` means this build has no `hot_reload` feature.
#[no_mangle]
pub extern "C" fn giochi_hot_reload_enable(path: *const c_char) -> i32 {
    guard(GIOCHI_ERROR_UNSUPPORTED, || {
        #[cfg(feature = "hot_reload")]
        {
            let Some(path) = state::cstr(path) else {
                set_last_error("the path is NULL");
                return GIOCHI_ERROR_INVALID_ARGUMENT;
            };
            match state::with_engine_mut("hot reload needs an initialised engine", |engine| {
                engine.enable_hot_reload(&[std::path::PathBuf::from(path)])
            }) {
                Ok(Ok(watched)) => i32::try_from(watched).unwrap_or(1),
                Ok(Err(error)) => state::fail(&error),
                Err(code) => code,
            }
        }
        #[cfg(not(feature = "hot_reload"))]
        {
            let _ = path;
            set_last_error(NO_WATCHER);
            GIOCHI_ERROR_UNSUPPORTED
        }
    })
}

/// Stops watching and discards the watcher.
#[no_mangle]
pub extern "C" fn giochi_hot_reload_disable() -> i32 {
    guard(GIOCHI_ERROR_UNSUPPORTED, || {
        #[cfg(feature = "hot_reload")]
        {
            match state::with_engine_mut("hot reload needs an initialised engine", |engine| {
                engine.disable_hot_reload()
            }) {
                Ok(()) => 0,
                Err(code) => code,
            }
        }
        #[cfg(not(feature = "hot_reload"))]
        {
            set_last_error(NO_WATCHER);
            GIOCHI_ERROR_UNSUPPORTED
        }
    })
}

/// `1` when a watcher is running, `0` otherwise.
#[no_mangle]
pub extern "C" fn giochi_hot_reload_is_enabled() -> i32 {
    guard(0, || {
        #[cfg(feature = "hot_reload")]
        {
            state::with_engine("hot reload needs an initialised engine", |engine| {
                i32::from(engine.is_hot_reload_enabled())
            })
            .unwrap_or(0)
        }
        #[cfg(not(feature = "hot_reload"))]
        {
            0
        }
    })
}

/// Drains pending changes and returns how many were applied.
///
/// Call it once per frame, before `update`, so a reload lands at a frame boundary
/// instead of in the middle of one.
#[no_mangle]
pub extern "C" fn giochi_hot_reload_poll() -> i32 {
    guard(0, || {
        #[cfg(feature = "hot_reload")]
        {
            state::with_engine_mut("hot reload needs an initialised engine", |engine| {
                engine.poll_hot_reload()
            })
            .map(|applied| i32::try_from(applied).unwrap_or(i32::MAX))
            .unwrap_or(0)
        }
        #[cfg(not(feature = "hot_reload"))]
        {
            0
        }
    })
}
