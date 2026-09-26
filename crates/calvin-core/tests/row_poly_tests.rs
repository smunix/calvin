use calvin_core::context::TypeContext;
use calvin_core::lang::typeinf::TypeInference;
use calvin_core::lang::types::{MonoType, Prim};

#[test]
fn test_row_polymorphism_unification() {
    let ctx = TypeContext::new();
    let typeinf = TypeInference::new(&ctx);

    // Record 1: { x: Int | r1 }
    let int_ty = ctx.alloc(MonoType::Prim(Prim::Int));
    let r1 = typeinf.fresh_tvar();
    let fields1 = ctx.arena().alloc_slice_copy(&[("x", int_ty as &MonoType)]);
    let rec1 = ctx.alloc(MonoType::Record(fields1, Some(r1)));

    // Record 2: { y: Float | r2 }
    let float_ty = ctx.alloc(MonoType::Prim(Prim::Float));
    let r2 = typeinf.fresh_tvar();
    let fields2 = ctx
        .arena()
        .alloc_slice_copy(&[("y", float_ty as &MonoType)]);
    let rec2 = ctx.alloc(MonoType::Record(fields2, Some(r2)));

    // Unify Record 1 and Record 2
    typeinf
        .unify(rec1, rec2)
        .expect("Should unify row polymorphic records");

    // After unification, r1 should be unified with { y: Float | new_tail }
    // and r2 should be unified with { x: Int | new_tail }.
    let r1_chased = r1.chase();
    match r1_chased {
        MonoType::Record(fields, Some(_)) => {
            assert_eq!(fields.len(), 1);
            assert_eq!(fields[0].0, "y");
            assert!(matches!(fields[0].1, MonoType::Prim(Prim::Float)));
        }
        _ => panic!("r1 did not unify correctly: {:?}", r1_chased),
    }

    let r2_chased = r2.chase();
    match r2_chased {
        MonoType::Record(fields, Some(_)) => {
            assert_eq!(fields.len(), 1);
            assert_eq!(fields[0].0, "x");
            assert!(matches!(fields[0].1, MonoType::Prim(Prim::Int)));
        }
        _ => panic!("r2 did not unify correctly: {:?}", r2_chased),
    }
}

#[test]
fn test_row_polymorphism_closed_rejection() {
    let ctx = TypeContext::new();
    let typeinf = TypeInference::new(&ctx);

    // Record 1: { x: Int } (closed row, tail is None)
    let int_ty = ctx.alloc(MonoType::Prim(Prim::Int));
    let fields1 = ctx.arena().alloc_slice_copy(&[("x", int_ty as &MonoType)]);
    let rec1 = ctx.alloc(MonoType::Record(fields1, None));

    // Record 2: { x: Int, y: Float | r2 }
    let float_ty = ctx.alloc(MonoType::Prim(Prim::Float));
    let r2 = typeinf.fresh_tvar();
    let fields2 = ctx
        .arena()
        .alloc_slice_copy(&[("x", int_ty as &MonoType), ("y", float_ty as &MonoType)]);
    let rec2 = ctx.alloc(MonoType::Record(fields2, Some(r2)));

    // Should fail because rec1 cannot accept 'y'
    assert!(typeinf.unify(rec1, rec2).is_err());
}
