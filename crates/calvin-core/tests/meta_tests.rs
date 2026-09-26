use calvin_core::context::TypeContext;
use calvin_core::lang::expr::Expr;
use calvin_core::lang::types::{Constraint, MonoType, Prim};
use calvin_core::lang::unqualifier::{
    AppendsToUnqualifier, EqualUnqualifier, NotUnqualifier, SizeOfUnqualifier, Unqualifier,
};

#[test]
fn test_appendsto_unqualifier() {
    let ctx = TypeContext::new();
    let unq = AppendsToUnqualifier;

    assert!(unq.handles("AppendsTo"));

    let int_ty = ctx.alloc(MonoType::Prim(Prim::Int));
    let arr_ty = ctx.alloc(MonoType::Array(int_ty));

    let args = vec![
        arr_ty as &MonoType,
        arr_ty as &MonoType,
        arr_ty as &MonoType,
    ];
    let constraint = Constraint {
        class_name: "AppendsTo",
        arguments: ctx.alloc_slice_clone(&args),
    };

    assert!(unq.refine(&ctx, &constraint));
    let dict = unq.satisfy(&ctx, &constraint, &[]).expect("Should satisfy");
    assert!(matches!(dict, Expr::Var("dict_AppendsTo")));
}

#[test]
fn test_equal_unqualifier() {
    let ctx = TypeContext::new();
    let unq = EqualUnqualifier;

    assert!(unq.handles("Equal"));

    let int_ty1 = ctx.alloc(MonoType::Prim(Prim::Int));
    let int_ty2 = ctx.alloc(MonoType::Prim(Prim::Int));

    let args = vec![int_ty1 as &MonoType, int_ty2 as &MonoType];
    let constraint = Constraint {
        class_name: "Equal",
        arguments: ctx.alloc_slice_clone(&args),
    };

    assert!(unq.refine(&ctx, &constraint));
    let dict = unq.satisfy(&ctx, &constraint, &[]).expect("Should satisfy");
    assert!(matches!(dict, Expr::Var("dict_Equal")));
}

#[test]
fn test_not_unqualifier() {
    let ctx = TypeContext::new();
    let unq = NotUnqualifier;

    assert!(unq.handles("Not"));

    let int_ty = ctx.alloc(MonoType::Prim(Prim::Int));

    let args = vec![int_ty as &MonoType];
    let constraint = Constraint {
        class_name: "Not",
        arguments: ctx.alloc_slice_clone(&args),
    };

    assert!(unq.refine(&ctx, &constraint));
    let dict = unq.satisfy(&ctx, &constraint, &[]).expect("Should satisfy");
    assert!(matches!(dict, Expr::Var("dict_Not")));
}

#[test]
fn test_sizeof_unqualifier() {
    let ctx = TypeContext::new();
    let unq = SizeOfUnqualifier;

    assert!(unq.handles("SizeOf"));

    let int_ty = ctx.alloc(MonoType::Prim(Prim::Int));
    let byte_ty = ctx.alloc(MonoType::Prim(Prim::Byte));

    let args_int = vec![int_ty as &MonoType, int_ty as &MonoType];
    let constraint_int = Constraint {
        class_name: "SizeOf",
        arguments: ctx.alloc_slice_clone(&args_int),
    };

    let args_byte = vec![byte_ty as &MonoType, int_ty as &MonoType];
    let constraint_byte = Constraint {
        class_name: "SizeOf",
        arguments: ctx.alloc_slice_clone(&args_byte),
    };

    assert!(unq.refine(&ctx, &constraint_int));
    let dict_int = unq
        .satisfy(&ctx, &constraint_int, &[])
        .expect("Should satisfy");
    if let Expr::Literal(calvin_core::lang::expr::Literal::Int(size)) = dict_int {
        assert_eq!(*size, 8);
    } else {
        panic!("Expected SizeOf to return literal Int");
    }

    assert!(unq.refine(&ctx, &constraint_byte));
    let dict_byte = unq
        .satisfy(&ctx, &constraint_byte, &[])
        .expect("Should satisfy");
    if let Expr::Literal(calvin_core::lang::expr::Literal::Int(size)) = dict_byte {
        assert_eq!(*size, 1);
    } else {
        panic!("Expected SizeOf to return literal Int");
    }
}
