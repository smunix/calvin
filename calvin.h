#ifndef CALVIN_H
#define CALVIN_H

#include <stdint.h>
#include <stdbool.h>

/**
 * The global context for a Calvin compilation unit.
 * It owns the memory arena used for zero-copy AST nodes and Types.
 */
typedef struct TypeContext TypeContext;

/**
 * Create a new Calvin TypeContext.
 */
struct TypeContext *calvin_context_create(void);

/**
 * Free a Calvin TypeContext.
 * # Safety
 * The caller must ensure that `ctx` is a valid pointer allocated by `calvin_context_create`.
 */
void calvin_context_free(struct TypeContext *ctx);

/**
 * Allocate memory from the thread-local JIT arena.
 * This function is exported so the JIT-compiled code can call it directly.
 */
uint8_t *calvin_alloc(uintptr_t size, uintptr_t align);

/**
 * Reset the thread-local JIT arena, instantly freeing all JIT-allocated memory.
 */
void calvin_reset_region(void);

#endif /* CALVIN_H */
