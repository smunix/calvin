use crate::context::TypeContext;

/// Create a new Calvin TypeContext.
#[no_mangle]
pub extern "C" fn calvin_context_create() -> *mut TypeContext {
    let ctx = Box::new(TypeContext::new());
    Box::into_raw(ctx)
}

/// Free a Calvin TypeContext.
#[no_mangle]
/// # Safety
/// The caller must ensure that `ctx` is a valid pointer allocated by `calvin_context_create`.
pub unsafe extern "C" fn calvin_context_free(ctx: *mut TypeContext) {
    if !ctx.is_null() {
        unsafe {
            let _ = Box::from_raw(ctx);
        }
    }
}
