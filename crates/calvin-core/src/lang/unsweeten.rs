use crate::context::TypeContext;
use crate::lang::expr::{Expr, Literal, Pattern};
use crate::lang::typeinf::TypeInference;
use crate::lang::types::{format_mono_no_simpl, MonoType, Prim};
use itertools::Itertools;

/// Recursively desugars lambda expressions matching Hobbes pattern compilation.
pub fn desugar_lambda<'ctx>(ctx: &'ctx TypeContext, expr: &'ctx Expr<'ctx>) -> &'ctx Expr<'ctx> {
    match expr {
        Expr::Fn(pat, body) => {
            let desugared_body = desugar_lambda(ctx, body);
            match pat {
                Pattern::Tuple(pats) => desugar_tuple_lambda(ctx, pats, desugared_body),
                Pattern::Var(v) => desugar_var_lambda(ctx, v, desugared_body),
                _ => &*ctx.alloc(Expr::Fn(pat.clone(), desugared_body)),
            }
        }
        Expr::App(f, args) => {
            let new_f = desugar_lambda(ctx, f);
            let new_args: Vec<_> = args.iter().map(|a| desugar_lambda(ctx, a)).collect();
            let new_args_slice = ctx.alloc_slice_clone(&new_args);
            &*ctx.alloc(Expr::App(new_f, new_args_slice))
        }
        Expr::Let(p, d, b) => {
            let new_d = desugar_lambda(ctx, d);
            let new_b = desugar_lambda(ctx, b);
            &*ctx.alloc(Expr::Let(p.clone(), new_d, new_b))
        }
        Expr::If(c, t, e) => {
            let new_c = desugar_lambda(ctx, c);
            let new_t = desugar_lambda(ctx, t);
            let new_e = desugar_lambda(ctx, e);
            &*ctx.alloc(Expr::If(new_c, new_t, new_e))
        }
        Expr::Tuple(elems) => {
            let new_elems: Vec<_> = elems.iter().map(|e| desugar_lambda(ctx, e)).collect();
            &*ctx.alloc(Expr::Tuple(ctx.alloc_slice_clone(&new_elems)))
        }
        Expr::Record(fields) => {
            let new_fields: Vec<_> = fields
                .iter()
                .map(|(k, v)| (*k, desugar_lambda(ctx, v)))
                .collect();
            &*ctx.alloc(Expr::Record(ctx.alloc_slice_clone(&new_fields)))
        }
        _ => expr,
    }
}

fn desugar_tuple_lambda<'ctx>(
    ctx: &'ctx TypeContext,
    pats: &[Pattern<'ctx>],
    desugared_body: &'ctx Expr<'ctx>,
) -> &'ctx Expr<'ctx> {
    let bindings: Vec<_> = pats
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let arg_name: &'ctx str = ctx.arena().alloc_str(&format!(".arg{}", i));
            let tvar_id = ctx.fresh_tvar_id();
            let rv_name: &'ctx str = ctx.arena().alloc_str(&format!(".t{}.rv{}", tvar_id, i));
            let orig_name = match p {
                Pattern::Var(v) => *v,
                _ => "_",
            };
            (rv_name, arg_name, orig_name)
        })
        .collect();

    // Substitute in body
    let curr_body = bindings.iter().fold(desugared_body, |acc, (rv_name, _, orig_name)| {
        if *orig_name != "_" {
            subst_var(ctx, acc, orig_name, rv_name)
        } else {
            acc
        }
    });

    // Nest let expressions from inside out
    let final_body = bindings
        .into_iter()
        .rev()
        .fold(curr_body, |acc, (rv_name, arg_name, _)| {
            let arg_var = &*ctx.alloc(Expr::Var(arg_name));
            &*ctx.alloc(Expr::Let(Pattern::Var(rv_name), arg_var, acc))
        });

    let arg_pats: Vec<_> = (0..pats.len())
        .map(|i| Pattern::Var(ctx.arena().alloc_str(&format!(".arg{}", i))))
        .collect();
    let arg_pats_slice = ctx.alloc_slice_clone(&arg_pats);
    &*ctx.alloc(Expr::Fn(Pattern::Tuple(arg_pats_slice), final_body))
}

fn desugar_var_lambda<'ctx>(
    ctx: &'ctx TypeContext,
    var_name: &'ctx str,
    desugared_body: &'ctx Expr<'ctx>,
) -> &'ctx Expr<'ctx> {
    let arg_name: &'ctx str = ctx.arena().alloc_str(".arg0");
    let tvar_id = ctx.fresh_tvar_id();
    let rv_name: &'ctx str = ctx.arena().alloc_str(&format!(".t{}.rv0", tvar_id));

    let curr_body = subst_var(ctx, desugared_body, var_name, rv_name);
    let arg_var = &*ctx.alloc(Expr::Var(arg_name));
    let let_body = &*ctx.alloc(Expr::Let(Pattern::Var(rv_name), arg_var, curr_body));

    let arg_pats = ctx.alloc_slice_clone(&[Pattern::Var(arg_name)]);
    &*ctx.alloc(Expr::Fn(Pattern::Tuple(arg_pats), let_body))
}

fn pattern_shadows(pattern: &Pattern, target: &str) -> bool {
    match pattern {
        Pattern::Var(v) => *v == target,
        Pattern::Tuple(pats) => pats.iter().any(|p| pattern_shadows(p, target)),
        _ => false,
    }
}

/// Substitute variable occurrences in expression.
pub fn subst_var<'ctx>(
    ctx: &'ctx TypeContext,
    expr: &'ctx Expr<'ctx>,
    target: &str,
    replacement: &'ctx str,
) -> &'ctx Expr<'ctx> {
    match expr {
        Expr::Var(v) if *v == target => &*ctx.alloc(Expr::Var(replacement)),
        Expr::App(f, args) => {
            let new_f = subst_var(ctx, f, target, replacement);
            let new_args: Vec<_> = args
                .iter()
                .map(|a| subst_var(ctx, a, target, replacement))
                .collect();
            let new_args_slice = ctx.alloc_slice_clone(&new_args);
            &*ctx.alloc(Expr::App(new_f, new_args_slice))
        }
        Expr::Let(p, d, b) => {
            let new_d = subst_var(ctx, d, target, replacement);
            let new_b = if pattern_shadows(p, target) {
                b
            } else {
                subst_var(ctx, b, target, replacement)
            };
            &*ctx.alloc(Expr::Let(p.clone(), new_d, new_b))
        }
        Expr::Fn(p, b) => {
            let new_b = if pattern_shadows(p, target) {
                b
            } else {
                subst_var(ctx, b, target, replacement)
            };
            &*ctx.alloc(Expr::Fn(p.clone(), new_b))
        }
        Expr::If(c, t, e) => {
            let new_c = subst_var(ctx, c, target, replacement);
            let new_t = subst_var(ctx, t, target, replacement);
            let new_e = subst_var(ctx, e, target, replacement);
            &*ctx.alloc(Expr::If(new_c, new_t, new_e))
        }
        Expr::Tuple(elems) => {
            let new_elems: Vec<_> = elems
                .iter()
                .map(|e| subst_var(ctx, e, target, replacement))
                .collect();
            &*ctx.alloc(Expr::Tuple(ctx.alloc_slice_clone(&new_elems)))
        }
        Expr::Record(fields) => {
            let new_fields: Vec<_> = fields
                .iter()
                .map(|(k, v)| (*k, subst_var(ctx, v, target, replacement)))
                .collect();
            &*ctx.alloc(Expr::Record(ctx.alloc_slice_clone(&new_fields)))
        }
        _ => expr,
    }
}

fn format_csts_ty<'a>(
    csts: &[(&'static str, Vec<&'a MonoType<'a>>)],
    ty: &'a MonoType<'a>,
) -> String {
    let ty_str = format_mono_no_simpl(ty);
    if csts.is_empty() {
        ty_str
    } else {
        let cst_strs: Vec<String> = csts
            .iter()
            .cloned()
            .sorted_by(crate::lang::types::compare_constraint)
            .map(|(name, args)| {
                let args_str = args.iter().copied().map(format_mono_no_simpl).join(" ");
                format!("{} {}", name, args_str)
            })
            .unique()
            .collect();
        if cst_strs.len() == 1 {
            format!("{} => {}", cst_strs[0], ty_str)
        } else {
            format!("({}) => {}", cst_strs.join(", "), ty_str)
        }
    }
}

fn normalize_constraints<'a>(constraints: &mut [(&'static str, Vec<&'a MonoType<'a>>)]) {
    constraints.iter_mut().for_each(|(_, args)| {
        args.iter_mut().for_each(|arg| *arg = arg.chase());
    });
    constraints.sort_by(crate::lang::types::compare_constraint);
}

fn merge_constraints<'a>(
    target: &mut Vec<(&'static str, Vec<&'a MonoType<'a>>)>,
    source: Vec<(&'static str, Vec<&'a MonoType<'a>>)>,
) {
    source.into_iter().for_each(|constraint| {
        if !target.contains(&constraint) {
            target.push(constraint);
        }
    });
}

/// Recursively infers types and formats the expression in Hobbes annotated syntax (`showAnnotated`).
type AnnotatedExpr<'ctx> = (
    String,
    &'ctx MonoType<'ctx>,
    Vec<(&'static str, Vec<&'ctx MonoType<'ctx>>)>,
);

fn show_annotated_internal<'ctx>(
    ctx: &'ctx TypeContext,
    expr: &'ctx Expr<'ctx>,
    type_inf: &mut TypeInference<'ctx>,
) -> Result<AnnotatedExpr<'ctx>, String> {
    match expr {
        Expr::Literal(lit) => match lit {
            Literal::Int(n) => {
                let ty = &*ctx.alloc(MonoType::Prim(Prim::Int));
                Ok((format!("{}:int", n), ty, Vec::new()))
            }
            Literal::Bool(b) => {
                let ty = &*ctx.alloc(MonoType::Prim(Prim::Bool));
                Ok((format!("{}:bool", b), ty, Vec::new()))
            }
            Literal::Char(c) => {
                let ty = &*ctx.alloc(MonoType::Prim(Prim::Char));
                Ok((format!("'{}':char", c), ty, Vec::new()))
            }
            Literal::Float(f) => {
                let ty = &*ctx.alloc(MonoType::Prim(Prim::Float));
                Ok((format!("{}:float", f), ty, Vec::new()))
            }
            Literal::Double(d) => {
                let ty = &*ctx.alloc(MonoType::Prim(Prim::Double));
                Ok((format!("{}:double", d), ty, Vec::new()))
            }
            Literal::Unit => {
                let ty = &*ctx.alloc(MonoType::Prim(Prim::Unit));
                Ok(("():unit".to_string(), ty, Vec::new()))
            }
            Literal::String(s) => {
                let char_ty = &*ctx.alloc(MonoType::Prim(Prim::Char));
                let str_ty = &*ctx.alloc(MonoType::Array(char_ty));
                Ok((format!("\"{}\":[char]", s), str_ty, Vec::new()))
            }
        },
        Expr::Var(name) => {
            if *name == "+" || *name == "-" || *name == "*" || *name == "/" {
                let class_name = match *name {
                    "+" => "Add",
                    "-" => "Subtract",
                    "*" => "Multiply",
                    "/" => "Divide",
                    _ => unreachable!(),
                };
                let t1 = type_inf.fresh_tvar();
                let t2 = type_inf.fresh_tvar();
                let t3 = type_inf.fresh_tvar();
                let tup = &*ctx.alloc(MonoType::Tuple(ctx.alloc_slice_clone(&[t1, t2])));
                let fn_ty = &*ctx.alloc(MonoType::Fn(tup, t3));
                let cst_args: &[&MonoType] = ctx.alloc_slice_clone(&[t1, t2, t3]);
                let cst = &*ctx.alloc(MonoType::Constraint(class_name, cst_args, fn_ty));
                type_inf.constraints.borrow_mut().push(cst);

                let t1_str = format_mono_no_simpl(t1);
                let t2_str = format_mono_no_simpl(t2);
                let t3_str = format_mono_no_simpl(t3);
                let s = format!(
                    "*:{} {} {} {} => ({} * {}) -> {}",
                    class_name, t1_str, t2_str, t3_str, t1_str, t2_str, t3_str
                );
                // Operator Var node format matches Hobbes:
                let s = if *name == "*" {
                    s
                } else {
                    format!(
                        "{}:{} {} {} {} => ({} * {}) -> {}",
                        name, class_name, t1_str, t2_str, t3_str, t1_str, t2_str, t3_str
                    )
                };
                Ok((s, fn_ty, vec![(class_name, vec![t1, t2, t3])]))
            } else if let Some(ty) = type_inf.lookup(name) {
                let ty = ty.chase();
                Ok((
                    format!("{}:{}", name, format_mono_no_simpl(ty)),
                    ty,
                    Vec::new(),
                ))
            } else {
                Err(format!("Unbound variable: {}", name))
            }
        }
        Expr::App(f, args) => {
            let (f_str, f_ty, mut csts) = show_annotated_internal(ctx, f, type_inf)?;
            let (arg_strs, arg_tys): (Vec<_>, Vec<_>) = args
                .iter()
                .map(|arg| {
                    let (a_str, a_ty, a_csts) = show_annotated_internal(ctx, arg, type_inf)?;
                    merge_constraints(&mut csts, a_csts);
                    Ok((a_str, a_ty))
                })
                .collect::<Result<Vec<_>, String>>()?
                .into_iter()
                .unzip();
            let ret_ty = type_inf.fresh_tvar();
            let expected_arg_ty = if arg_tys.len() == 1 {
                arg_tys[0]
            } else {
                &*ctx.alloc(MonoType::Tuple(ctx.alloc_slice_clone(&arg_tys)))
            };
            let expected_fn_ty = &*ctx.alloc(MonoType::Fn(expected_arg_ty, ret_ty));
            type_inf
                .unify(f_ty, expected_fn_ty)
                .map_err(|e| format!("{:?}", e))?;

            normalize_constraints(&mut csts);

            let app_body = format!("({})({})", f_str, arg_strs.join(", "));
            let qual_ty_str = format_csts_ty(&csts, ret_ty.chase());
            Ok((
                format!("{}:{}", app_body, qual_ty_str),
                ret_ty.chase(),
                csts,
            ))
        }
        Expr::Let(pat, def, body) => {
            let (def_str, def_ty, mut csts) = show_annotated_internal(ctx, def, type_inf)?;
            let var_name = match pat {
                Pattern::Var(v) => *v,
                _ => unimplemented!("Complex patterns in unsweeten let"),
            };

            type_inf.bind(var_name, def_ty);
            let (body_str, body_ty, body_csts) = show_annotated_internal(ctx, body, type_inf)?;
            merge_constraints(&mut csts, body_csts);
            normalize_constraints(&mut csts);

            let qual_ty_str = format_csts_ty(&csts, body_ty.chase());
            let s = format!(
                "(let {} = {} in {}):{}",
                var_name, def_str, body_str, qual_ty_str
            );
            Ok((s, body_ty.chase(), csts))
        }
        Expr::Fn(pat, body) => {
            let (arg_names, arg_tys): (Vec<_>, Vec<_>) = match pat {
                Pattern::Tuple(pats) => pats
                    .iter()
                    .map(|p| {
                        let n = match p {
                            Pattern::Var(v) => *v,
                            _ => "_",
                        };
                        let tvar = type_inf.fresh_tvar();
                        type_inf.bind(n, tvar);
                        (n, tvar)
                    })
                    .unzip(),
                Pattern::Var(v) => {
                    let tvar = type_inf.fresh_tvar();
                    type_inf.bind(v, tvar);
                    (vec![*v], vec![tvar])
                }
                _ => unimplemented!(),
            };

            let (body_str, body_ty, mut body_csts) = show_annotated_internal(ctx, body, type_inf)?;

            let fn_arg_ty = if arg_tys.len() == 1 {
                arg_tys[0]
            } else {
                &*ctx.alloc(MonoType::Tuple(ctx.alloc_slice_clone(&arg_tys)))
            };
            let fn_ty = &*ctx.alloc(MonoType::Fn(fn_arg_ty, body_ty));

            normalize_constraints(&mut body_csts);

            let qual_ty_str = format_csts_ty(&body_csts, fn_ty.chase());
            let s = format!(
                "(\\({}).({})):{}",
                arg_names.join(", "),
                body_str,
                qual_ty_str
            );
            Ok((s, fn_ty.chase(), body_csts))
        }
        _ => Err("Unsupported expression in unsweeten".to_string()),
    }
}

/// Public API for `:u` command: unsweeten and format annotated expression.
pub fn unsweeten<'ctx>(ctx: &'ctx TypeContext, expr: &'ctx Expr<'ctx>) -> Result<String, String> {
    let desugared = desugar_lambda(ctx, expr);
    let mut type_inf = TypeInference::new(ctx);
    let (s, ty, _) = show_annotated_internal(ctx, desugared, &mut type_inf)?;
    let top_ty_str = format_mono_no_simpl(ty.chase());
    Ok(format!("({})::{}", s, top_ty_str))
}
