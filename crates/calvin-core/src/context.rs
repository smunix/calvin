use bumpalo::Bump;
use std::cell::Cell;

/// The global context for a Calvin compilation unit.
/// It owns the memory arena used for zero-copy AST nodes and Types.
pub struct TypeContext {
    arena: Bump,
    uid_ctr: Cell<usize>,
}

impl Default for TypeContext {
    fn default() -> Self {
        Self::new()
    }
}

impl TypeContext {
    pub fn new() -> Self {
        Self {
            arena: Bump::new(),
            uid_ctr: Cell::new(0),
        }
    }

    pub fn arena(&self) -> &Bump {
        &self.arena
    }

    pub fn alloc<T>(&self, val: T) -> &mut T {
        self.arena.alloc(val)
    }

    pub fn alloc_slice_clone<T: Clone>(&self, slice: &[T]) -> &mut [T] {
        self.arena.alloc_slice_clone(slice)
    }

    pub fn fresh_tvar_id(&self) -> usize {
        let id = self.uid_ctr.get();
        self.uid_ctr.set(id + 1);
        id
    }

    pub fn fresh_tvar(&self) -> crate::lang::types::TVarId {
        crate::lang::types::TVarId(self.fresh_tvar_id())
    }

    pub fn reset(&mut self) {
        self.arena.reset();
        self.uid_ctr.set(0);
    }
}
