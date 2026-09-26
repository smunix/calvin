use crate::lang::expr::Expr;
use crate::lang::types::MonoType;
use std::collections::HashMap;

/// A type class definition with parameters, functional dependencies, and member signatures.
#[derive(Debug, Clone)]
pub struct TypeClassDef<'a> {
    pub name: &'a str,
    pub params: Vec<&'a str>,
    pub fundeps: Vec<(Vec<usize>, Vec<usize>)>, // (from_param_indices, to_param_indices)
    pub members: HashMap<String, &'a MonoType<'a>>,
}

/// An instance definition with ground/generic types, prerequisite context, and member implementations.
#[derive(Debug, Clone)]
pub struct InstanceDef<'a> {
    pub class_name: &'a str,
    pub types: Vec<&'a MonoType<'a>>,
    pub context: Vec<(&'a str, Vec<&'a MonoType<'a>>)>,
    pub member_impls: HashMap<String, &'a Expr<'a>>,
}

/// Central registry storing known type classes, functional dependencies, and instance databases.
#[derive(Debug, Clone, Default)]
pub struct TypeClassRegistry<'a> {
    pub classes: HashMap<String, TypeClassDef<'a>>,
    pub instances: HashMap<String, Vec<InstanceDef<'a>>>,
}

impl<'a> TypeClassRegistry<'a> {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register_class(
        &mut self,
        name: &'a str,
        params: Vec<&'a str>,
        fundeps: Vec<(Vec<usize>, Vec<usize>)>,
        members: HashMap<String, &'a MonoType<'a>>,
    ) {
        self.classes.insert(
            name.to_string(),
            TypeClassDef {
                name,
                params,
                fundeps,
                members,
            },
        );
    }

    pub fn register_instance(
        &mut self,
        class_name: &'a str,
        types: Vec<&'a MonoType<'a>>,
        context: Vec<(&'a str, Vec<&'a MonoType<'a>>)>,
        member_impls: HashMap<String, &'a Expr<'a>>,
    ) {
        self.instances
            .entry(class_name.to_string())
            .or_default()
            .push(InstanceDef {
                class_name,
                types,
                context,
                member_impls,
            });
    }

    /// Match ground instances or apply functional dependencies.
    /// Returns any unifications (target_tvar, resolved_type) to perform.
    pub fn refine_fundeps(
        &self,
        class_name: &str,
        args: &[&'a MonoType<'a>],
    ) -> Vec<(&'a MonoType<'a>, &'a MonoType<'a>)> {
        let mut unifications = Vec::new();
        let class_def = match self.classes.get(class_name) {
            Some(cd) => cd,
            None => return unifications,
        };

        let instances = match self.instances.get(class_name) {
            Some(insts) => insts,
            None => return unifications,
        };

        for (from_indices, to_indices) in &class_def.fundeps {
            // Check if all `from_indices` are concrete / without free type variables
            let from_concrete = from_indices.iter().all(|&idx| {
                if idx < args.len() {
                    !has_free_tvars(args[idx].chase())
                } else {
                    false
                }
            });

            if !from_concrete {
                continue;
            }

            // Find matching instances
            let matching_insts: Vec<&InstanceDef<'a>> = instances
                .iter()
                .filter(|inst| {
                    if inst.types.len() != args.len() {
                        return false;
                    }
                    from_indices.iter().all(|&idx| {
                        types_match(args[idx].chase(), inst.types[idx].chase())
                    })
                })
                .collect();

            // If there's a match, refine `to_indices`
            if !matching_insts.is_empty() {
                for &to_idx in to_indices {
                    if to_idx < args.len() {
                        let target_ty = args[to_idx].chase();
                        if has_free_tvars(target_ty) {
                            let candidate_ty = matching_insts[0].types[to_idx].chase();
                            let all_agree = matching_insts
                                .iter()
                                .all(|inst| types_match(inst.types[to_idx].chase(), candidate_ty));
                            if all_agree {
                                unifications.push((args[to_idx], candidate_ty));
                            }
                        }
                    }
                }
            }
        }

        unifications
    }

    /// Check if a constraint is fully satisfied by a ground instance.
    pub fn is_satisfied(&self, class_name: &str, args: &[&'a MonoType<'a>]) -> bool {
        let instances = match self.instances.get(class_name) {
            Some(insts) => insts,
            None => return false,
        };

        for inst in instances {
            if inst.types.len() != args.len() {
                continue;
            }
            let matches = args
                .iter()
                .zip(inst.types.iter())
                .all(|(a, b)| types_match(a.chase(), b.chase()));
            if matches && inst.context.is_empty() {
                return true;
            }
        }
        false
    }
}

fn has_free_tvars(ty: &MonoType) -> bool {
    match ty {
        MonoType::TVar(_, cell) => cell.get().map_or(true, |inner| has_free_tvars(inner.chase())),
        MonoType::Array(inner) => has_free_tvars(inner.chase()),
        MonoType::FixedArray(inner, _) => has_free_tvars(inner.chase()),
        MonoType::Fn(a, b) => has_free_tvars(a.chase()) || has_free_tvars(b.chase()),
        MonoType::Tuple(ts) => ts.iter().any(|t| has_free_tvars(t.chase())),
        MonoType::Record(fields, tail) => {
            fields.iter().any(|(_, t)| has_free_tvars(t.chase()))
                || tail.map_or(false, |t| has_free_tvars(t.chase()))
        }
        MonoType::Variant(cases, tail) => {
            cases.iter().any(|(_, t)| has_free_tvars(t.chase()))
                || tail.map_or(false, |t| has_free_tvars(t.chase()))
        }
        MonoType::App(f, args) => {
            has_free_tvars(f.chase()) || args.iter().any(|a| has_free_tvars(a.chase()))
        }
        MonoType::Constraint(_, args, inner) => {
            args.iter().any(|a| has_free_tvars(a.chase())) || has_free_tvars(inner.chase())
        }
        _ => false,
    }
}

fn types_match(a: &MonoType, b: &MonoType) -> bool {
    match (a, b) {
        (MonoType::Prim(p1), MonoType::Prim(p2)) => p1 == p2,
        (MonoType::Array(in1), MonoType::Array(in2)) => types_match(in1.chase(), in2.chase()),
        (MonoType::FixedArray(in1, n1), MonoType::FixedArray(in2, n2)) => {
            n1 == n2 && types_match(in1.chase(), in2.chase())
        }
        (MonoType::Fn(a1, r1), MonoType::Fn(a2, r2)) => {
            types_match(a1.chase(), a2.chase()) && types_match(r1.chase(), r2.chase())
        }
        (MonoType::Tuple(ts1), MonoType::Tuple(ts2)) => {
            ts1.len() == ts2.len()
                && ts1
                    .iter()
                    .zip(ts2.iter())
                    .all(|(t1, t2)| types_match(t1.chase(), t2.chase()))
        }
        _ => false,
    }
}
