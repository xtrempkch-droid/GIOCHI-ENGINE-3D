//! Consola in-game — Part VIII da ABI C.
//!
//! # A parte que torna isto diferente
//!
//! A consola vive atrás da feature `devtools`, que traz o `egui`. O header não pode
//! mudar com a feature de quem compilou o `.so`, por isso cada função é **uma** definição
//! com um `#[cfg]` *dentro* do corpo: com `devtools` ligada executa a sério; sem ela
//! devolve [`GIOCHI_ERROR_UNSUPPORTED`](crate::abi_types::GIOCHI_ERROR_UNSUPPORTED) e
//! explica em `giochi_get_last_error()` como recompilar. Duas definições `#[cfg]`-adas
//! seriam duas declarações no header, porque o cbindgen não avalia `cfg`.

use core::ffi::c_char;

#[cfg(feature = "devtools")]
use crate::abi_types::GIOCHI_ERROR_INVALID_ARGUMENT;
use crate::abi_types::GIOCHI_ERROR_UNSUPPORTED;
#[cfg(feature = "devtools")]
use crate::state;
use crate::state::{guard, set_last_error};

/// Message reported when the build has no console.
#[cfg(not(feature = "devtools"))]
const NO_CONSOLE: &str = "this libgiochi was built without the `devtools` feature, so it has no \
     console; rebuild with --features devtools";

/// Executes one console command line, as if it had been typed into the console.
///
/// Returns `0` on success. The output goes to the console scrollback, readable with
/// `giochi_console_output_count`/`giochi_console_output_line` — C has no
/// `Vec<String>`, and copying into a caller buffer is the convention the rest of the
/// ABI uses. Returns `GIOCHI_ERROR_UNSUPPORTED` when this build has no console.
#[no_mangle]
pub extern "C" fn giochi_console_execute(line: *const c_char) -> i32 {
    guard(GIOCHI_ERROR_UNSUPPORTED, || {
        #[cfg(feature = "devtools")]
        {
            let Some(line) = state::cstr(line) else {
                set_last_error("the command is NULL");
                return GIOCHI_ERROR_INVALID_ARGUMENT;
            };
            match state::with_engine_mut("the console needs an initialised engine", |engine| {
                engine.devtools_mut().execute(&line);
            }) {
                Ok(()) => 0,
                Err(code) => code,
            }
        }
        #[cfg(not(feature = "devtools"))]
        {
            let _ = line;
            set_last_error(NO_CONSOLE);
            GIOCHI_ERROR_UNSUPPORTED
        }
    })
}

/// How many lines the console scrollback holds (`0` without a console).
#[no_mangle]
pub extern "C" fn giochi_console_output_count() -> i32 {
    guard(0, || {
        #[cfg(feature = "devtools")]
        {
            state::with_engine("the console needs an initialised engine", |engine| {
                i32::try_from(engine.devtools().console().output_len()).unwrap_or(i32::MAX)
            })
            .unwrap_or(0)
        }
        #[cfg(not(feature = "devtools"))]
        {
            0
        }
    })
}

/// Copies scrollback line `index` (`0` is the oldest) into `buffer` as NUL-terminated
/// UTF-8.
///
/// `capacity` includes the NUL; `out_len` receives the bytes written, not counting the
/// NUL. A line longer than the buffer is truncated, which is why `out_len` exists.
#[no_mangle]
pub extern "C" fn giochi_console_output_line(
    index: i32,
    buffer: *mut c_char,
    capacity: usize,
    out_len: *mut usize,
) -> i32 {
    guard(GIOCHI_ERROR_UNSUPPORTED, || {
        #[cfg(feature = "devtools")]
        {
            if buffer.is_null() || capacity == 0 || index < 0 {
                set_last_error("buffer is NULL, capacity is 0, or index is negative");
                return GIOCHI_ERROR_INVALID_ARGUMENT;
            }
            match state::with_engine("the console needs an initialised engine", |engine| {
                let lines = engine.devtools().console().output_text();
                let Some(line) = lines.get(index as usize) else {
                    return Err(());
                };
                let bytes = line.as_bytes();
                let written = bytes.len().min(capacity - 1);
                // SAFETY: the caller guarantees `buffer` has `capacity` writable bytes;
                // `written` is at most `capacity - 1` and one NUL follows it.
                unsafe {
                    for (offset, byte) in bytes[..written].iter().enumerate() {
                        *buffer.add(offset) = *byte as c_char;
                    }
                    *buffer.add(written) = 0;
                    if !out_len.is_null() {
                        *out_len = written;
                    }
                }
                Ok(())
            }) {
                Ok(Ok(())) => 0,
                Ok(Err(())) => {
                    set_last_error("the scrollback index is out of range");
                    GIOCHI_ERROR_INVALID_ARGUMENT
                }
                Err(code) => code,
            }
        }
        #[cfg(not(feature = "devtools"))]
        {
            let _ = (index, buffer, capacity, out_len);
            set_last_error(NO_CONSOLE);
            GIOCHI_ERROR_UNSUPPORTED
        }
    })
}

/// Clears the scrollback and the command history.
#[no_mangle]
pub extern "C" fn giochi_console_clear() -> i32 {
    guard(GIOCHI_ERROR_UNSUPPORTED, || {
        #[cfg(feature = "devtools")]
        {
            match state::with_engine_mut("the console needs an initialised engine", |engine| {
                engine.devtools_mut().console_mut().clear();
            }) {
                Ok(()) => 0,
                Err(code) => code,
            }
        }
        #[cfg(not(feature = "devtools"))]
        {
            set_last_error(NO_CONSOLE);
            GIOCHI_ERROR_UNSUPPORTED
        }
    })
}
