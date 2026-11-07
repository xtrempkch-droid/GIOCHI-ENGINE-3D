//! Scene stack management.

use crate::scene::scene::Scene;

/// A stack of scenes.
///
/// `push` overlays a new scene (useful for pause menus), `pop` returns to the
/// previous one and `replace` swaps the top of the stack. Only the top scene is
/// updated and rendered.
#[repr(C)]
#[derive(Debug, Default)]
pub struct SceneManager {
    _reserved: [u32; 4],
}

impl SceneManager {
    /// Creates an empty stack.
    pub const fn new() -> Self {
        Self { _reserved: [0; 4] }
    }

    /// Pushes `scene` on top of the stack and calls its `on_enter`.
    ///
    /// Implemented in phase 5.
    pub fn push(&mut self, _scene: Box<dyn Scene>) {
        todo!("fase 5: empilhar cena")
    }

    /// Pops the top scene, calling its `on_exit`.
    ///
    /// Returns `None` when the stack is empty.
    ///
    /// Implemented in phase 5.
    pub fn pop(&mut self) -> Option<Box<dyn Scene>> {
        todo!("fase 5: desempilhar cena")
    }

    /// Replaces the top scene (or pushes, when the stack is empty).
    ///
    /// Implemented in phase 5.
    pub fn replace(&mut self, _scene: Box<dyn Scene>) {
        todo!("fase 5: substituir cena")
    }

    /// Mutable reference to the top scene, if any.
    ///
    /// Implemented in phase 5.
    pub fn current_mut(&mut self) -> Option<&mut dyn Scene> {
        todo!("fase 5: cena atual")
    }

    /// Number of scenes on the stack.
    ///
    /// Implemented in phase 5.
    pub fn depth(&self) -> usize {
        todo!("fase 5: profundidade da pilha")
    }

    /// Returns `true` when the stack is empty.
    ///
    /// Implemented in phase 5.
    pub fn is_empty(&self) -> bool {
        todo!("fase 5: pilha vazia")
    }
}
