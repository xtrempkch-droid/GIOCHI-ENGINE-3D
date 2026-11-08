//! Crate-private handle registry for 3D resources.
//!
//! The same discipline as the 2D engine's registry (ADR-002), deliberately duplicated
//! rather than shared: a `GiochiTexture` and a `GiochiGlTexture` are different
//! resources with different lifetimes, and letting one be passed where the other is
//! expected is a mistake worth making impossible (`docs/C_ABI_CONTRACT.md` §9.5).
//!
//! Invariants:
//!
//! * handle `0` means "null/invalid" and is never issued;
//! * handles are never reused, so a stale handle can never alias a new resource;
//! * the registry is single-threaded, matching the engine.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};

/// Opaque handle type. `0` is always invalid.
pub(crate) type Handle = u64;

/// Handle `0` is reserved and never issued.
pub const NULL_HANDLE: Handle = 0;

/// Maps opaque handles to 3D resources.
#[derive(Debug)]
pub(crate) struct HandleRegistry<T> {
    next: AtomicU64,
    entries: RefCell<HashMap<Handle, Rc<T>>>,
}

impl<T> Default for HandleRegistry<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> HandleRegistry<T> {
    /// Creates an empty registry.
    pub(crate) fn new() -> Self {
        Self {
            next: AtomicU64::new(1),
            entries: RefCell::new(HashMap::new()),
        }
    }

    /// Stores `value` and returns its new, unique handle.
    pub(crate) fn insert(&self, value: T) -> Handle {
        let handle = self.next.fetch_add(1, Ordering::Relaxed);
        self.entries.borrow_mut().insert(handle, Rc::new(value));
        handle
    }

    /// Returns the value behind `handle`, or `None` for null/unknown.
    pub(crate) fn get(&self, handle: Handle) -> Option<Rc<T>> {
        if handle == NULL_HANDLE {
            return None;
        }
        self.entries.borrow().get(&handle).cloned()
    }

    /// Removes `handle` and hands back the registry's own reference.
    ///
    /// This is the only correct way to take ownership of a resource that needs an explicit
    /// teardown, and the distinction from [`Self::get`] is load-bearing rather than
    /// stylistic: `get` **clones** the `Rc`, so the count is two by the time the caller
    /// sees it and `Rc::try_unwrap` can never succeed. Deleting a GL buffer needs exactly
    /// that unwrap, so using `get` there silently leaked every mesh and shader — the
    /// registry entry disappeared while the VAO, VBO, EBO and program stayed alive in the
    /// driver. `take` returns the sole remaining strong reference (the temporary clones
    /// `get` hands to a draw are dropped before the draw returns).
    pub(crate) fn take(&self, handle: Handle) -> Option<Rc<T>> {
        if handle == NULL_HANDLE {
            return None;
        }
        self.entries.borrow_mut().remove(&handle)
    }

    /// Number of live entries.
    pub(crate) fn len(&self) -> usize {
        self.entries.borrow().len()
    }

    /// Returns `true` when nothing is registered.
    pub(crate) fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The live handles, in unspecified order.
    ///
    /// Needed by `RenderContext3D::release_all`, which must delete GL objects through a
    /// context that the registry does not have.
    pub(crate) fn handles(&self) -> Vec<Handle> {
        self.entries.borrow().keys().copied().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn handles_start_at_one_so_zero_stays_invalid() {
        let registry = HandleRegistry::new();
        let handle = registry.insert(42);

        assert_eq!(handle, 1);
        assert_ne!(handle, NULL_HANDLE);
    }

    #[test]
    fn a_null_handle_never_resolves() {
        let registry: HandleRegistry<i32> = HandleRegistry::new();

        assert!(registry.get(NULL_HANDLE).is_none());
        assert!(registry.take(NULL_HANDLE).is_none());
    }

    #[test]
    fn handles_are_never_reused_so_a_stale_one_cannot_alias_a_new_resource() {
        let registry = HandleRegistry::new();
        let first = registry.insert("a");
        assert!(registry.take(first).is_some());
        let second = registry.insert("b");

        assert_ne!(
            first, second,
            "reusing {first} would resurrect a stale handle"
        );
        assert!(registry.get(first).is_none(), "the old handle stays dead");
        assert_eq!(*registry.get(second).expect("live"), "b");
    }

    #[test]
    fn removing_twice_reports_false_the_second_time() {
        let registry = HandleRegistry::new();
        let handle = registry.insert(1);

        assert!(registry.take(handle).is_some());
        assert!(registry.take(handle).is_none());
    }

    #[test]
    fn len_tracks_inserts_and_removes() {
        let registry = HandleRegistry::new();
        assert!(registry.is_empty());

        let a = registry.insert(1);
        let b = registry.insert(2);
        assert_eq!(registry.len(), 2);

        drop(registry.take(a));
        assert_eq!(registry.len(), 1);

        drop(registry.take(b));
        assert!(registry.is_empty());
        assert!(registry.get(b).is_none());
    }
}
