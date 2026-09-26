use calvin_core::lang::types::{MonoType, Prim};
use cranelift_codegen::ir::types::{F64, I128, I16, I32, I64, I8};
use cranelift_codegen::ir::Type;

/// Lower a Calvin `MonoType` to a Cranelift `Type`.
pub fn lower_type<'a>(ty: &'a MonoType<'a>) -> Type {
    match ty.chase() {
        MonoType::Prim(Prim::Unit) => I8, // Cranelift doesn't have void; use I8 and ignore
        MonoType::Prim(Prim::Bool) => I8,
        MonoType::Prim(Prim::Char) => I32,
        MonoType::Prim(Prim::Byte) => I8,
        MonoType::Prim(Prim::Short) => I16,
        MonoType::Prim(Prim::Int) => I64,
        MonoType::Prim(Prim::Long) => I64,
        MonoType::Prim(Prim::Int128) => I128,
        MonoType::Prim(Prim::Float) => F64,
        MonoType::Prim(Prim::Double) => F64,
        MonoType::Prim(Prim::Time)
        | MonoType::Prim(Prim::TimeSpan)
        | MonoType::Prim(Prim::DateTime) => I64,
        // Pointers for complex types
        MonoType::Array(_)
        | MonoType::Tuple(_)
        | MonoType::Record(_, _)
        | MonoType::Variant(_, _)
        | MonoType::Fn(_, _) => I64,
        MonoType::TVar(_, _)
        | MonoType::TGen(_)
        | MonoType::App(_, _)
        | MonoType::Constraint(_, _, _)
        | MonoType::FixedArray(_, _) => I64,
    }
}
