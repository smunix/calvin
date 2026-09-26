use bumpalo::Bump;
use std::cell::RefCell;

thread_local! {
    /// A thread-local memory arena for JIT-allocated values (arrays, closures, tuples).
    /// Mirrors Hobbes' `fregion` thread-local allocation strategy for evaluation.
    static JIT_ARENA: RefCell<Bump> = RefCell::new(Bump::new());
}

/// Allocate memory from the thread-local JIT arena.
/// This function is exported so the JIT-compiled code can call it directly.
#[no_mangle]
pub extern "C" fn calvin_alloc(size: usize, align: usize) -> *mut u8 {
    JIT_ARENA.with(|arena| {
        let layout = std::alloc::Layout::from_size_align(size, align).unwrap();
        arena.borrow().alloc_layout(layout).as_ptr()
    })
}

/// Reset the thread-local JIT arena, instantly freeing all JIT-allocated memory.
#[no_mangle]
pub extern "C" fn calvin_reset_region() {
    JIT_ARENA.with(|arena| {
        arena.borrow_mut().reset();
    });
}
