use calvin_core::context::TypeContext;
use calvin_core::lang::expr::Expr;
use calvin_core::lang::types::{Constraint, MonoType, Prim};
use calvin_core::lang::unqualifier::{
    ConsRecordUnqualifier, ConsVariantUnqualifier, HasCtorUnqualifier, HasFieldUnqualifier,
    Unqualifier,
};

#[test]
fn test_has_field_unqualifier() {
    let ctx = TypeContext::new();
    let unq = HasFieldUnqualifier;

    assert!(unq.handles("HasField"));

    let field_name_ty = ctx.alloc(MonoType::Prim(Prim::Unit));
    let int_ty = ctx.alloc(MonoType::Prim(Prim::Int));

    // { x: Int }
    let fields = ctx.arena().alloc_slice_copy(&[("x", int_ty as &MonoType)]);
    let record_ty = ctx.alloc(MonoType::Record(fields, None));

    let args = vec![
        field_name_ty as &MonoType,
        record_ty as &MonoType,
        int_ty as &MonoType,
    ];
    let constraint = Constraint {
        class_name: "HasField",
        arguments: ctx.alloc_slice_clone(&args),
    };

    assert!(unq.refine(&ctx, &constraint));
    let dict = unq.satisfy(&ctx, &constraint, &[]).expect("Should satisfy");
    assert!(matches!(dict, Expr::Var("dict_HasField")));
}

#[test]
fn test_cons_record_unqualifier() {
    let ctx = TypeContext::new();
    let unq = ConsRecordUnqualifier;

    assert!(unq.handles("ConsRecord"));

    let field_name_ty = ctx.alloc(MonoType::Prim(Prim::Unit));
    let int_ty = ctx.alloc(MonoType::Prim(Prim::Int));

    // r1 = { x: Int }
    let fields1 = ctx.arena().alloc_slice_copy(&[("x", int_ty as &MonoType)]);
    let r1 = ctx.alloc(MonoType::Record(fields1, None));

    // r2 = { x: Int, y: Int }
    let fields2 = ctx
        .arena()
        .alloc_slice_copy(&[("x", int_ty as &MonoType), ("y", int_ty as &MonoType)]);
    let r2 = ctx.alloc(MonoType::Record(fields2, None));

    let args = vec![
        field_name_ty as &MonoType,
        int_ty as &MonoType,
        r1 as &MonoType,
        r2 as &MonoType,
    ];
    let constraint = Constraint {
        class_name: "ConsRecord",
        arguments: ctx.alloc_slice_clone(&args),
    };

    assert!(unq.refine(&ctx, &constraint));
    let dict = unq.satisfy(&ctx, &constraint, &[]).expect("Should satisfy");
    assert!(matches!(dict, Expr::Var("dict_ConsRecord")));
}

#[test]
fn test_variant_unqualifiers() {
    let ctx = TypeContext::new();
    let has_ctor = HasCtorUnqualifier;
    let cons_var = ConsVariantUnqualifier;

    assert!(has_ctor.handles("HasCtor"));
    assert!(cons_var.handles("ConsVariant"));

    let ctor_name_ty = ctx.alloc(MonoType::Prim(Prim::Unit));
    let int_ty = ctx.alloc(MonoType::Prim(Prim::Int));

    // | Foo: Int |
    let cases = ctx
        .arena()
        .alloc_slice_copy(&[("Foo", int_ty as &MonoType)]);
    let variant_ty = ctx.alloc(MonoType::Variant(cases, None));

    let args1 = vec![
        ctor_name_ty as &MonoType,
        variant_ty as &MonoType,
        int_ty as &MonoType,
    ];
    let constraint1 = Constraint {
        class_name: "HasCtor",
        arguments: ctx.alloc_slice_clone(&args1),
    };

    assert!(has_ctor.refine(&ctx, &constraint1));
    let dict1 = has_ctor
        .satisfy(&ctx, &constraint1, &[])
        .expect("Should satisfy");
    assert!(matches!(dict1, Expr::Var("dict_HasCtor")));
}

#[test]
fn test_record_and_variant_formatting() {
    use calvin_core::runtime::value::format_runtime_value;

    let ctx = TypeContext::new();
    let int_ty = ctx.alloc(MonoType::Prim(Prim::Int));
    let fields = ctx
        .arena()
        .alloc_slice_copy(&[("x", int_ty as &MonoType), ("y", int_ty as &MonoType)]);
    let rec_ty = ctx.alloc(MonoType::Record(fields, None));

    // 2 64-bit integer fields: x=1, y=2
    let data: [u64; 2] = [1, 2];
    let formatted = unsafe { format_runtime_value(data.as_ptr() as u64, rec_ty) };
    assert_eq!(formatted, "{x=1, y=2}");

    // Variant: tag 0, payload 42
    let cases = ctx.arena().alloc_slice_copy(&[("x", int_ty as &MonoType)]);
    let var_ty = ctx.alloc(MonoType::Variant(cases, None));
    let var_data: [u64; 2] = [0, 42];
    let formatted_var = unsafe { format_runtime_value(var_data.as_ptr() as u64, var_ty) };
    assert_eq!(formatted_var, "|x=42|");

    // Unit variant: tag 0, unit payload
    let unit_ty = ctx.alloc(MonoType::Prim(Prim::Unit));
    let unit_cases = ctx
        .arena()
        .alloc_slice_copy(&[("Foo", unit_ty as &MonoType)]);
    let unit_var_ty = ctx.alloc(MonoType::Variant(unit_cases, None));
    let unit_var_data: [u64; 2] = [0, 0];
    let formatted_unit =
        unsafe { format_runtime_value(unit_var_data.as_ptr() as u64, unit_var_ty) };
    assert_eq!(formatted_unit, "|Foo|");
}
