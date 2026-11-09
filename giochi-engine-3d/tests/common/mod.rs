//! Helpers shared by the 3D integration tests.

use std::sync::{Mutex, MutexGuard, OnceLock};

/// Serialises tests that create a [`Platform`](giochi_engine::platform::Platform).
///
/// SDL2 allows **one `EventPump` per process**, and `Platform::new` creates one. Cargo
/// runs the tests of a binary in parallel threads, so the second test to reach
/// `Platform::new` failed; the GL helpers interpreted any failure as "this machine has no
/// OpenGL driver" and skipped. In CI that meant every GL test but one silently did not
/// run — the suite reported `ok` while exercising a single code path.
///
/// The guard is returned to the test so the lock is held for as long as the platform is
/// alive. Tests in the same binary still run concurrently up to this point; the expensive
/// GL work is what serialises.
pub fn platform_guard() -> MutexGuard<'static, ()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}
