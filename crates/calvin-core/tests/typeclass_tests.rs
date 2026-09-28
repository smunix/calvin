use calvin_core::context::TypeContext;
use calvin_core::lang::expr::ExprVisitor;
use calvin_core::lang::typeinf::TypeInference;
use calvin_core::lang::types::{format_qual_type, MonoType};
use calvin_core::lang::unsweeten::unsweeten;
use calvin_parse::parse_expr;

fn setup_typeinf<'a>(ctx: &'a TypeContext) -> TypeInference<'a> {
    let mut type_inf = TypeInference::new(ctx);
    let a = ctx.alloc(MonoType::TGen(0));
    let b = ctx.alloc(MonoType::TGen(1));
    let c = ctx.alloc(MonoType::TGen(2));
    let args: &[&MonoType] = ctx.alloc_slice_clone(&[&*a, &*b, &*c]);
    let b_to_c = ctx.alloc(MonoType::Fn(b, c));
    let a_to_b_to_c = ctx.alloc(MonoType::Fn(a, b_to_c));

    let add_c = ctx.alloc(MonoType::Constraint("Add", args, a_to_b_to_c));
    let sub_c = ctx.alloc(MonoType::Constraint("Subtract", args, a_to_b_to_c));
    let mul_c = ctx.alloc(MonoType::Constraint("Multiply", args, a_to_b_to_c));
    let div_c = ctx.alloc(MonoType::Constraint("Divide", args, a_to_b_to_c));

    type_inf.bind("+", add_c);
    type_inf.bind("-", sub_c);
    type_inf.bind("*", mul_c);
    type_inf.bind("/", div_c);
    type_inf
}

#[test]
fn test_hobbes_type_parity_multi_operators() {
    let ctx = TypeContext::new();

    // 1. \x y z. x * y
    let mut typeinf = setup_typeinf(&ctx);
    let ast = parse_expr(&ctx, r"\x y z. x * y").expect("parse failed");
    let ty = typeinf.visit(ast).expect("infer failed");
    typeinf.solve_constraints().expect("solve failed");
    let residuals = typeinf.residual_constraints();
    let formatted = format_qual_type(ty, &residuals);
    assert_eq!(formatted, "Multiply a b d => (a * b * c) -> d");

    // 2. \x y z. x * y * z
    let mut typeinf = setup_typeinf(&ctx);
    let ast = parse_expr(&ctx, r"\x y z. x * y * z").expect("parse failed");
    let ty = typeinf.visit(ast).expect("infer failed");
    typeinf.solve_constraints().expect("solve failed");
    let residuals = typeinf.residual_constraints();
    let formatted = format_qual_type(ty, &residuals);
    assert_eq!(
        formatted,
        "Multiply a b d, Multiply d c e => (a * b * c) -> e"
    );

    // 3. \x -> \y -> \z -> x * y * z
    let mut typeinf = setup_typeinf(&ctx);
    let ast = parse_expr(&ctx, r"\x -> \y -> \z -> x * y * z").expect("parse failed");
    let ty = typeinf.visit(ast).expect("infer failed");
    typeinf.solve_constraints().expect("solve failed");
    let residuals = typeinf.residual_constraints();
    let formatted = format_qual_type(ty, &residuals);
    assert_eq!(
        formatted,
        "Multiply a b d, Multiply d c e => (a) -> (b) -> (c) -> e"
    );
}

#[test]
fn test_hobbes_unsweeten_parity() {
    let ctx = TypeContext::new();

    let ast1 = parse_expr(&ctx, "1").expect("parse failed");
    let u1 = unsweeten(&ctx, ast1).expect("unsweeten failed");
    assert_eq!(u1, "(1:int)::int");

    let ast2 = parse_expr(&ctx, r"\x. x").expect("parse failed");
    let u2 = unsweeten(&ctx, ast2).expect("unsweeten failed");
    assert!(u2.starts_with("((\\(.arg0).((let .t"), "u2: {}", u2);
    assert!(u2.ends_with(")):(.t1) -> .t1)::(.t1) -> .t1"), "u2: {}", u2);

    let ast3 = parse_expr(&ctx, r"\x y z. x * y * z").expect("parse failed");
    let u3 = unsweeten(&ctx, ast3).expect("unsweeten failed");
    assert!(
        u3.contains("(Multiply .t5 .t6 .t14, Multiply .t14 .t7 .t15) => (.t5 * .t6 * .t7) -> .t15)::(.t5 * .t6 * .t7) -> .t15"),
        "u3: {}",
        u3
    );
}

#[test]
fn test_hobbes_curried_and_tuple_application() {
    let ctx = TypeContext::new();

    // 1. Hobbes tuple function with space-separated args: (\x y. x * y) 1.0 2.0
    let mut typeinf = setup_typeinf(&ctx);
    let ast = parse_expr(&ctx, r"(\x y. x * y) 1.0 2.0").expect("parse failed");
    let ty = typeinf.visit(ast).expect("infer failed");
    typeinf.solve_constraints().expect("solve failed");
    let residuals = typeinf.residual_constraints();
    assert_eq!(format_qual_type(ty, &residuals), "double");

    // 2. Hobbes tuple function with tuple arg: (\x y. x * y) (1.0, 2.0)
    let mut typeinf = setup_typeinf(&ctx);
    let ast = parse_expr(&ctx, r"(\x y. x * y) (1.0, 2.0)").expect("parse failed");
    let ty = typeinf.visit(ast).expect("infer failed");
    typeinf.solve_constraints().expect("solve failed");
    let residuals = typeinf.residual_constraints();
    assert_eq!(format_qual_type(ty, &residuals), "double");

    // 3. Curried function with space-separated args: (\x -> \y -> x * y) 1.0 2.0
    let mut typeinf = setup_typeinf(&ctx);
    let ast = parse_expr(&ctx, r"(\x -> \y -> x * y) 1.0 2.0").expect("parse failed");
    let ty = typeinf.visit(ast).expect("infer failed");
    typeinf.solve_constraints().expect("solve failed");
    let residuals = typeinf.residual_constraints();
    assert_eq!(format_qual_type(ty, &residuals), "double");

    // 4. Curried function with tuple arg: (\x -> \y -> x * y) (1.0, 2.0)
    let mut typeinf = setup_typeinf(&ctx);
    let ast = parse_expr(&ctx, r"(\x -> \y -> x * y) (1.0, 2.0)").expect("parse failed");
    let ty = typeinf.visit(ast).expect("infer failed");
    typeinf.solve_constraints().expect("solve failed");
    let residuals = typeinf.residual_constraints();
    assert_eq!(format_qual_type(ty, &residuals), "double");

    // 5. Hobbes `fn` syntax: (fn x y. x * y) 1.0 2.0
    let mut typeinf = setup_typeinf(&ctx);
    let ast = parse_expr(&ctx, r"(fn x y. x * y) 1.0 2.0").expect("parse failed");
    let ty = typeinf.visit(ast).expect("infer failed");
    typeinf.solve_constraints().expect("solve failed");
    let residuals = typeinf.residual_constraints();
    assert_eq!(format_qual_type(ty, &residuals), "double");
}

#[test]
fn test_hobbes_record_and_variant_parsing_and_typing() {
    let ctx = TypeContext::new();

    // 1. Calvin record: {x: 1, y: 2}
    let mut typeinf = setup_typeinf(&ctx);
    let ast1 = parse_expr(&ctx, "{x: 1, y: 2}").expect("parse failed");
    let ty1 = typeinf.visit(ast1).expect("infer failed");
    let residuals1 = typeinf.residual_constraints();
    assert_eq!(format_qual_type(ty1, &residuals1), "{ x:int, y:int }");

    // 2. Hobbes record: {x=1, y=2}
    let mut typeinf = setup_typeinf(&ctx);
    let ast2 = parse_expr(&ctx, "{x=1, y=2}").expect("parse failed");
    let ty2 = typeinf.visit(ast2).expect("infer failed");
    let residuals2 = typeinf.residual_constraints();
    assert_eq!(format_qual_type(ty2, &residuals2), "{ x:int, y:int }");

    // 3. Variant: |x=1|
    let mut typeinf = setup_typeinf(&ctx);
    let ast3 = parse_expr(&ctx, "|x=1|").expect("parse failed");
    let ty3 = typeinf.visit(ast3).expect("infer failed");
    let residuals3 = typeinf.residual_constraints();
    assert_eq!(format_qual_type(ty3, &residuals3), "|x:int|");

    // 4. Unit variant: |x|
    let mut typeinf = setup_typeinf(&ctx);
    let ast4 = parse_expr(&ctx, "|x|").expect("parse failed");
    let ty4 = typeinf.visit(ast4).expect("infer failed");
    let residuals4 = typeinf.residual_constraints();
    assert_eq!(format_qual_type(ty4, &residuals4), "|x:()|");
}

#[test]
fn test_unsatisfiable_constraint_error() {
    let ctx = TypeContext::new();
    let mut typeinf = setup_typeinf(&ctx);
    let ast = parse_expr(&ctx, "{x: 1, y: 2} + {x: 3, y: 3}").expect("parse failed");
    let _ty = typeinf.visit(ast).expect("infer failed");
    let err = typeinf
        .solve_constraints()
        .expect_err("should fail to solve");
    match err {
        calvin_core::lang::typeinf::TypeError::UnsatisfiableConstraint {
            class_name,
            args,
            explanation,
        } => {
            assert_eq!(class_name, "Add");
            assert_eq!(args, vec!["{ x:int, y:int }", "{ x:int, y:int }", "a"]);
            assert!(explanation.is_some());
            let expl = explanation.unwrap();
            assert!(expl.contains("most likely instance fails at:"));
            assert!(expl.contains("!equals { x:int, y:int } { x:int, y:int }"));
            assert!(expl.contains("Add { x:int, y:int } { x:int, y:int } { x:int, y:int }"));
        }
        _ => panic!("Expected UnsatisfiableConstraint error, got {:?}", err),
    }
}
