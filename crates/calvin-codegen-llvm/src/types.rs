use calvin_core::lang::types::{MonoType, Prim};
use inkwell::context::Context;
use inkwell::types::BasicTypeEnum;

/// Lower a Calvin `MonoType` to an LLVM `BasicTypeEnum`.
pub fn lower_type<'ctx, 'a>(context: &'ctx Context, ty: &'a MonoType<'a>) -> BasicTypeEnum<'ctx> {
    match ty.chase() {
        MonoType::Prim(Prim::Unit) => context.i8_type().into(), // LLVM doesn't allow void as a basic value; use i8
        MonoType::Prim(Prim::Bool) => context.bool_type().into(),
        MonoType::Prim(Prim::Char) => context.i32_type().into(),
        MonoType::Prim(Prim::Byte) => context.i8_type().into(),
        MonoType::Prim(Prim::Short) => context.i16_type().into(),
        MonoType::Prim(Prim::Int) => context.i64_type().into(),
        MonoType::Prim(Prim::Long) => context.i64_type().into(),
        MonoType::Prim(Prim::Int128) => context.i128_type().into(),
        MonoType::Prim(Prim::Float) => context.f64_type().into(),
        MonoType::Prim(Prim::Double) => context.f64_type().into(),
        MonoType::Prim(Prim::Time)
        | MonoType::Prim(Prim::TimeSpan)
        | MonoType::Prim(Prim::DateTime) => context.i64_type().into(),

        // Pointers for complex types (represented as i64 or pointer type depending on architecture)
        // For now, representing heap-allocated complex structures as opaque pointers
        MonoType::Array(_)
        | MonoType::Tuple(_)
        | MonoType::Record(_, _)
        | MonoType::Variant(_, _)
        | MonoType::Fn(_, _) => context.ptr_type(inkwell::AddressSpace::default()).into(),

        MonoType::TVar(_, _)
        | MonoType::TGen(_)
        | MonoType::App(_, _)
        | MonoType::Constraint(_, _, _)
        | MonoType::FixedArray(_, _) => context.ptr_type(inkwell::AddressSpace::default()).into(),
    }
}
