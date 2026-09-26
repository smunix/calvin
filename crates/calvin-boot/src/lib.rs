use calvin_core::context::TypeContext;
use calvin_core::lang::typeclass::TypeClassRegistry;
use calvin_core::lang::typeinf::TypeEnv;
use calvin_core::lang::types::MonoType;
use calvin_parse::ast::{ModuleDef, TypeExpr};
use calvin_parse::parse_module;
use std::rc::Rc;

pub const BOOT_ARITH: &str = include_str!("../boot/arith.hob");

pub fn init_bootstrap<'ctx>(
    ctx: &'ctx TypeContext,
) -> Result<(Rc<TypeEnv<'ctx>>, Rc<TypeClassRegistry<'ctx>>), String> {
    let mut base_env = TypeEnv::new();
    let mut registry = TypeClassRegistry::new();

    // 1. Parse arith.hob module
    let module = parse_module(ctx, BOOT_ARITH).map_err(|e| format!("Failed to parse arith.hob: {}", e))?;

    for def in module.defs {
        match def {
            ModuleDef::Class(class_def) => {
                let mut fundeps = Vec::new();
                for (from_names, to_names) in &class_def.fundeps {
                    let from_indices: Vec<usize> = from_names
                        .iter()
                        .filter_map(|name| class_def.params.iter().position(|p| p == name))
                        .collect();
                    let to_indices: Vec<usize> = to_names
                        .iter()
                        .filter_map(|name| class_def.params.iter().position(|p| p == name))
                        .collect();
                    fundeps.push((from_indices, to_indices));
                }

                // Register class into registry
                registry.register_class(
                    class_def.name,
                    class_def.params.clone(),
                    fundeps,
                    std::collections::HashMap::new(),
                );

                // Build type parameters as TGen(0), TGen(1), ...
                let gen_tys: Vec<&'ctx MonoType<'ctx>> = (0..class_def.params.len())
                    .map(|i| &*ctx.alloc(MonoType::TGen(i)))
                    .collect();
                let gen_slice = ctx.arena().alloc_slice_clone(&gen_tys);

                // For each member, bind its qualified constraint type in base_env
                for member in &class_def.members {
                    let mut op_name = member.name;
                    if op_name.starts_with('(') && op_name.ends_with(')') && op_name.len() >= 3 {
                        op_name = &op_name[1..op_name.len() - 1];
                    }

                    // Create function type:
                    // If class has 3 params (e.g. a, b, c), function is a -> b -> c
                    // If class has 1 param (e.g. a), function is a -> a (for neg) or a (for zero/one)
                    let inner_fn = if class_def.params.len() == 3 {
                        let b_to_c = ctx.alloc(MonoType::Fn(gen_tys[1], gen_tys[2]));
                        ctx.alloc(MonoType::Fn(gen_tys[0], b_to_c))
                    } else if class_def.params.len() == 1 {
                        if op_name == "neg" {
                            ctx.alloc(MonoType::Fn(gen_tys[0], gen_tys[0]))
                        } else {
                            gen_tys[0]
                        }
                    } else {
                        gen_tys[0]
                    };

                    let cst_ty = ctx.alloc(MonoType::Constraint(class_def.name, gen_slice, inner_fn));
                    base_env.insert(op_name, cst_ty);
                    if op_name != member.name {
                        base_env.insert(member.name, cst_ty);
                    }
                }
            }
            ModuleDef::Instance(inst_def) => {
                let mut ground_tys = Vec::new();
                for te in &inst_def.types {
                    let mty = lower_type_expr(ctx, te);
                    ground_tys.push(mty);
                }
                registry.register_instance(
                    inst_def.class_name,
                    ground_tys,
                    Vec::new(),
                    std::collections::HashMap::new(),
                );
            }
            _ => {}
        }
    }

    Ok((Rc::new(base_env), Rc::new(registry)))
}

fn lower_type_expr<'ctx>(ctx: &'ctx TypeContext, te: &TypeExpr) -> &'ctx MonoType<'ctx> {
    match te {
        TypeExpr::Prim(p) => ctx.alloc(MonoType::Prim(*p)),
        TypeExpr::Var(_) => ctx.alloc(MonoType::TVar(ctx.fresh_tvar_id(), std::cell::Cell::new(None))),
        TypeExpr::Tuple(ts) => {
            let lowered: Vec<&'ctx MonoType<'ctx>> = ts.iter().map(|t| lower_type_expr(ctx, t)).collect();
            ctx.alloc(MonoType::Tuple(ctx.arena().alloc_slice_clone(&lowered)))
        }
        TypeExpr::Fn(dom, codom) => {
            let d = lower_type_expr(ctx, dom);
            let c = lower_type_expr(ctx, codom);
            ctx.alloc(MonoType::Fn(d, c))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use calvin_core::lang::expr::ExprVisitor;

    #[test]
    fn test_bootstrap_arith() {
        let ctx = TypeContext::new();
        let (env, registry) = init_bootstrap(&ctx).expect("bootstrap failed");

        assert!(env.lookup("+").is_some());
        assert!(env.lookup("-").is_some());
        assert!(env.lookup("*").is_some());
        assert!(env.lookup("/").is_some());

        assert!(registry.classes.contains_key("Add"));
        assert!(registry.classes.contains_key("Subtract"));
        assert!(registry.classes.contains_key("Multiply"));
        assert!(registry.classes.contains_key("Divide"));

        let add_insts = registry.instances.get("Add").expect("Add instances missing");
        assert!(!add_insts.is_empty());
    }

    #[test]
    fn test_hobbes_bootstrap_type_inference() {
        let ctx = TypeContext::new();
        let (env, registry) = init_bootstrap(&ctx).expect("bootstrap failed");

        let mut typeinf = calvin_core::lang::typeinf::TypeInference::with_env_and_classes(
            &ctx,
            env.clone(),
            registry.clone(),
        );

        // 1. :t (\x y. x + y) (1.0, 2.0)
        let ast1 = calvin_parse::parse_expr(&ctx, r"(\x y. x + y) (1.0, 2.0)").expect("parse failed");
        let ty1 = typeinf.visit(ast1).expect("infer failed");
        typeinf.solve_constraints().expect("solve failed");
        let residuals1 = typeinf.residual_constraints();
        let formatted1 = calvin_core::lang::types::format_qual_type(ty1, &residuals1);
        assert_eq!(formatted1, "double");

        // 2. :t (\x -> \y -> x + y) 1.0 2.0
        let mut typeinf2 = calvin_core::lang::typeinf::TypeInference::with_env_and_classes(
            &ctx,
            env.clone(),
            registry.clone(),
        );
        let ast2 = calvin_parse::parse_expr(&ctx, r"(\x -> \y -> x + y) 1.0 2.0").expect("parse failed");
        let ty2 = typeinf2.visit(ast2).expect("infer failed");
        typeinf2.solve_constraints().expect("solve failed");
        let residuals2 = typeinf2.residual_constraints();
        let formatted2 = calvin_core::lang::types::format_qual_type(ty2, &residuals2);
        assert_eq!(formatted2, "double");
    }
}
