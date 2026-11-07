//! Crate-private handle registry.
//!
//! Every engine resource (texture, sound, music, scene) is exposed to users as
//! an opaque `u64` handle. The handle maps to an `Rc<T>` inside a
//! [`HandleRegistry`]; the `Rc` never leaves the crate (see `docs/DECISIONS.md`,
//! ADR-002).
//!
//! Invariants:
//!
//! * handle `0` is reserved for "null/invalid" and is never issued;
//! * handles are never reused, so a stale handle can never alias a new resource;
//! * the registry is single-threaded (`RefCell`), matching the engine.
//!
//! The registry is not referenced by any phase-0 code yet: it is consumed by the
//! texture, audio and scene registries in phases 2, 4 and 5. Until then the
//! `dead_code` allowance below keeps `clippy -D warnings` meaningful; it is
//! removed in phase 2, when the first caller lands.
#![allow(dead_code)]

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};

/// Opaque handle type used by the Rust API.
pub(crate) type Handle = u64;

/// Handle `0` is always invalid and never returned by [`HandleRegistry::insert`].
pub(crate) const NULL_HANDLE: Handle = 0;

/// First handle value issued by a registry.
const FIRST_HANDLE: Handle = 1;

/// Maps opaque handles to values owned by the engine.
///
/// Values are stored behind `Rc<T>` so callers can hold a resource alive while
/// another part of the engine still references it. Lock-free reads are not
/// needed: the engine is single-threaded.
#[derive(Debug)]
pub(crate) struct HandleRegistry<T> {
    next: AtomicU64,
    entries: RefCell<HashMap<Handle, Rc<T>>>,
}

impl<T> HandleRegistry<T> {
    /// Creates an empty registry.
    pub(crate) fn new() -> Self {
        Self {
            next: AtomicU64::new(FIRST_HANDLE),
            entries: RefCell::new(HashMap::new()),
        }
    }

    /// Stores `value` and returns its new, unique handle.
    pub(crate) fn insert(&self, value: T) -> Handle {
        let handle = self.next.fetch_add(1, Ordering::Relaxed);
        self.entries.borrow_mut().insert(handle, Rc::new(value));
        handle
    }

    /// Returns a shared reference-counted pointer to the value behind `handle`.
    ///
    /// Returns [`None`] for [`NULL_HANDLE`] or for a handle that was removed.
    pub(crate) fn get(&self, handle: Handle) -> Option<Rc<T>> {
        if handle == NULL_HANDLE {
            return None;
        }
        self.entries.borrow().get(&handle).cloned()
    }

    /// Returns `true` if `handle` refers to a live entry.
    pub(crate) fn contains(&self, handle: Handle) -> bool {
        handle != NULL_HANDLE && self.entries.borrow().contains_key(&handle)
    }

    /// Drops the entry behind `handle`. Returns `false` if it was not registered.
    pub(crate) fn remove(&self, handle: Handle) -> bool {
        if handle == NULL_HANDLE {
            return false;
        }
        self.entries.borrow_mut().remove(&handle).is_some()
    }

    /// Number of live entries.
    pub(crate) fn len(&self) -> usize {
        self.entries.borrow().len()
    }

    /// Returns `true` when no entry is live.
    pub(crate) fn is_empty(&self) -> bool {
        self.entries.borrow().is_empty()
    }

    /// Removes every entry.
    pub(crate) fn clear(&self) {
        self.entries.borrow_mut().clear();
    }
}

impl<T> Default for HandleRegistry<T> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insert_returns_non_zero_unique_handles() {
        let registry = HandleRegistry::new();
        let first = registry.insert(10u32);
        let second = registry.insert(20u32);

        assert_ne!(first, NULL_HANDLE);
        assert_ne!(second, NULL_HANDLE);
        assert_ne!(first, second);
        assert_eq!(registry.len(), 2);
    }

    #[test]
    fn get_returns_the_stored_value() {
        let registry = HandleRegistry::new();
        let handle = registry.insert(String::from("texture"));

        let value = registry.get(handle).expect("handle must be live");
        assert_eq!(value.as_str(), "texture");
    }

    #[test]
    fn get_of_null_handle_is_none() {
        let registry: HandleRegistry<u32> = HandleRegistry::new();
        assert!(registry.get(NULL_HANDLE).is_none());
    }

    #[test]
    fn removed_handle_is_no_longer_resolvable() {
        let registry = HandleRegistry::new();
        let handle = registry.insert(1u8);

        assert!(registry.remove(handle));
        assert!(!registry.remove(handle));
        assert!(registry.get(handle).is_none());
        assert!(!registry.contains(handle));
        assert!(registry.is_empty());
    }

    #[test]
    fn removing_null_handle_is_a_no_op() {
        let registry: HandleRegistry<u32> = HandleRegistry::new();
        assert!(!registry.remove(NULL_HANDLE));
        assert!(registry.is_empty());
    }

    #[test]
    fn shared_pointer_keeps_value_alive_after_removal() {
        let registry = HandleRegistry::new();
        let handle = registry.insert(42u32);
        let shared = registry.get(handle).expect("handle must be live");

        assert!(registry.remove(handle));
        assert_eq!(*shared, 42);
    }

    #[test]
    fn clear_drops_every_entry() {
        let registry = HandleRegistry::new();
        registry.insert(1u8);
        registry.insert(2u8);

        registry.clear();
        assert_eq!(registry.len(), 0);
        assert!(registry.is_empty());
    }
}
