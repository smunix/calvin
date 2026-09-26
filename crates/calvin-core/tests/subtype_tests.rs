use calvin_core::context::TypeContext;
use calvin_core::lang::expr::Expr;
use calvin_core::lang::types::{Constraint, MonoType, Prim};
use calvin_core::lang::unqualifier::{SubtypeUnqualifier, Unqualifier};

#[test]
fn test_subtype_unqualifier_identity() {
    let ctx = TypeContext::new();
    let unq = SubtypeUnqualifier;

    assert!(unq.handles("Convert"));
    assert!(unq.handles("Subtype"));

    // Check Int -> Int identity conversion
    let int_ty = ctx.alloc(MonoType::Prim(Prim::Int));
    let args = vec![int_ty as &MonoType, int_ty as &MonoType];
    let constraint = Constraint {
        class_name: "Convert",
        arguments: ctx.alloc_slice_clone(&args),
    };

    assert!(unq.refine(&ctx, &constraint));
    let dict = unq.satisfy(&ctx, &constraint, &[]).expect("Should satisfy");
    assert!(matches!(dict, Expr::Var("dict_Convert_Id")));
}

#[test]
fn test_subtype_unqualifier_widening() {
    let ctx = TypeContext::new();
    let unq = SubtypeUnqualifier;

    // Check Int -> Float widening conversion
    let int_ty = ctx.alloc(MonoType::Prim(Prim::Int));
    let float_ty = ctx.alloc(MonoType::Prim(Prim::Float));

    let args = vec![int_ty as &MonoType, float_ty as &MonoType];
    let constraint = Constraint {
        class_name: "Convert",
        arguments: ctx.alloc_slice_clone(&args),
    };

    assert!(unq.refine(&ctx, &constraint));
    let dict = unq.satisfy(&ctx, &constraint, &[]).expect("Should satisfy");
    assert!(matches!(dict, Expr::Var("dict_Convert_Int_Float")));
}

#[test]
fn test_subtype_unqualifier_rejection() {
    let ctx = TypeContext::new();
    let unq = SubtypeUnqualifier;

    // Check Float -> Int (should reject)
    let int_ty = ctx.alloc(MonoType::Prim(Prim::Int));
    let float_ty = ctx.alloc(MonoType::Prim(Prim::Float));

    let args = vec![float_ty as &MonoType, int_ty as &MonoType];
    let constraint = Constraint {
        class_name: "Convert",
        arguments: ctx.alloc_slice_clone(&args),
    };

    assert!(!unq.refine(&ctx, &constraint));
    assert!(unq.satisfy(&ctx, &constraint, &[]).is_none());
}
