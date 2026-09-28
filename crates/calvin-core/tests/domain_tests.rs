use calvin_core::context::TypeContext;
use calvin_core::lang::types::{Constraint, MonoType, Prim, QualType, TGenId, TVarId};
use calvin_core::runtime::value::RawValue;

#[test]
fn test_type_domain_value_objects() {
    let tvar = TVarId::new(42);
    assert_eq!(tvar.as_usize(), 42);
    assert_eq!(format!("{}", tvar), "42");
    assert_eq!(usize::from(tvar), 42);

    let tgen = TGenId::new(7);
    assert_eq!(tgen.as_usize(), 7);
    assert_eq!(format!("{}", tgen), "7");
    assert_eq!(usize::from(tgen), 7);
}

#[test]
fn test_monotype_domain_queries() {
    let ctx = TypeContext::new();

    let int_ty = ctx.alloc(MonoType::Prim(Prim::Int));
    assert!(int_ty.is_primitive());
    assert!(!int_ty.is_function());

    let fn_ty = ctx.alloc(MonoType::Fn(int_ty, int_ty));
    assert!(fn_ty.is_function());
    assert!(!fn_ty.is_primitive());

    let rec_fields = ctx.alloc_slice_clone(&[("x", &*int_ty)]);
    let rec_ty = ctx.alloc(MonoType::Record(rec_fields, None));
    assert!(rec_ty.is_record());

    let var_cases = ctx.alloc_slice_clone(&[("A", &*int_ty)]);
    let var_ty = ctx.alloc(MonoType::Variant(var_cases, None));
    assert!(var_ty.is_variant());

    let tvar_ty = ctx.alloc(MonoType::TVar(ctx.fresh_tvar_id(), std::cell::Cell::new(None)));
    assert!(tvar_ty.is_tvar());

    let constraint_args = ctx.alloc_slice_clone(&[&*int_ty]);
    let constraint = Constraint::new("Num", constraint_args);
    assert_eq!(constraint.class_name(), "Num");
    assert_eq!(constraint.arguments().len(), 1);

    let constraints = ctx.alloc_slice_clone(&[constraint]);
    let qual = QualType::new(constraints, fn_ty);
    assert!(!qual.is_monomorphic());
    assert_eq!(qual.constraints().len(), 1);
    assert_eq!(qual.ty(), fn_ty);

    let mono_qual = QualType::new(&[], int_ty);
    assert!(mono_qual.is_monomorphic());
}

#[test]
fn test_raw_value_domain_object() {
    let val_bool = RawValue::from_raw(1);
    assert!(val_bool.as_bool());
    assert!(!val_bool.is_null());

    let val_int = RawValue::from_raw(12345);
    assert_eq!(val_int.as_i64(), 12345);
    assert_eq!(val_int.as_u64(), 12345);

    let val_float = RawValue::from_raw(3.14159f64.to_bits());
    assert!((val_float.as_f64() - 3.14159).abs() < 1e-5);
}
