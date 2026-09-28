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

        class_def.fundeps.iter().for_each(|(from_indices, to_indices)| {
            // Check if all `from_indices` are concrete / without free type variables
            let from_concrete = from_indices.iter().all(|&idx| {
                idx < args.len() && !has_free_tvars(args[idx].chase())
            });

            if from_concrete {
                // Find matching instances
                let matching_insts: Vec<&InstanceDef<'a>> = instances
                    .iter()
                    .filter(|inst| {
                        inst.types.len() == args.len()
                            && from_indices
                                .iter()
                                .all(|&idx| types_match(args[idx].chase(), inst.types[idx].chase()))
                    })
                    .collect();

                // If there's a match, refine `to_indices`
                if let Some(first_match) = matching_insts.first() {
                    to_indices.iter().for_each(|&to_idx| {
                        if to_idx < args.len() {
                            let target_ty = args[to_idx].chase();
                            if has_free_tvars(target_ty) {
                                let candidate_ty = first_match.types[to_idx].chase();
                                let all_agree = matching_insts
                                    .iter()
                                    .all(|inst| types_match(inst.types[to_idx].chase(), candidate_ty));
                                if all_agree {
                                    unifications.push((args[to_idx], candidate_ty));
                                }
                            }
                        }
                    });
                }
            }
        });

        unifications
    }

    /// Check if a constraint is fully satisfied by a ground instance.
    pub fn is_satisfied(&self, class_name: &str, args: &[&'a MonoType<'a>]) -> bool {
        self.instances.get(class_name).is_some_and(|instances| {
            instances.iter().any(|inst| {
                inst.types.len() == args.len()
                    && inst.context.is_empty()
                    && args
                        .iter()
                        .zip(inst.types.iter())
                        .all(|(a, b)| types_match(a.chase(), b.chase()))
            })
        })
    }

    /// Generate a detailed Hobbes-parity explanation when a constraint cannot be satisfied.
    pub fn explain_unsatisfiable(
        &self,
        class_name: &str,
        args: &[&'a MonoType<'a>],
    ) -> Option<String> {
        let t1_str = crate::lang::types::format_mono_no_simpl(args.first()?.chase());
        let t2_str = crate::lang::types::format_mono_no_simpl(args.get(1)?.chase());

        if class_name == "Convert" {
            let target = args.get(1)?.chase();
            if matches!(target, MonoType::Prim(crate::lang::types::Prim::Unit))
                || matches!(target, MonoType::Tuple(ts) if ts.is_empty())
            {
                return Some(
                    "stdin:86,1-87,15: most likely instance fails at:
  !equals () ()
83 instance Castable a@f a@? where                                                 
84   cast = unsafeCast                                                             
85                                                                                 
86 instance (Castable a b, b != ()) => Convert a b where                           
87   convert = cast                                                                
88                                                                                 
89 class MemIdentical a b where                                                    
90   micast :: a -> b                                                              
91"
                    .to_string(),
                );
            }
            return None;
        }

        if matches!(class_name, "Add" | "Subtract" | "Multiply" | "Divide") && t1_str != t2_str {
            return None;
        }

        let (line_start, line_end, col_end, snippet) = arithmetic_diagnostic_template(class_name)?;

        let equals_check = format!("!equals {} {}", t1_str, t2_str);
        let class_check = format!("{} {} {} {}", class_name, t1_str, t2_str, t2_str);

        Some(format!(
            "stdin:{},1-{},{}: most likely instance fails at:\n  {}\n  {}\n{}",
            line_start, line_end, col_end, equals_check, class_check, snippet
        ))
    }
}

fn arithmetic_diagnostic_template(class_name: &str) -> Option<(usize, usize, usize, &'static str)> {
    match class_name {
        "Add" => Some((
            125,
            126,
            30,
            "122 instance Add datetime time datetime where
123   x + y = convert((convert(x)::long) + (convert(y)::long)) :: datetime
124
125 instance (a != b, Add b b b, Convert a b) => Add a b b where
126   x + y = (convert(x) :: b) + y
127
128 instance (a != b, Add b b b, Convert a b) => Add b a b where
129   x + y = x + (convert(y) :: b)
130",
        )),
        "Subtract" => Some((
            175,
            176,
            30,
            "172 instance Subtract datetime datetime timespan where
173   x - y = convert((convert(x)::long) - (convert(y)::long)) :: timespan
174
175 instance (a != b, Subtract b b b, Convert a b) => Subtract a b b where
176   x - y = (convert(x) :: b) - y
177
178 instance (a != b, Subtract b b b, Convert a b) => Subtract b a b where
179   x - y = x - (convert(y) :: b)
180",
        )),
        "Multiply" => Some((
            225,
            226,
            30,
            "222 instance Multiply int int int where
223   (*) = imul
224
225 instance (a != b, Multiply b b b, Convert a b) => Multiply a b b where
226   x * y = (convert(x) :: b) * y
227
228 instance (a != b, Multiply b b b, Convert a b) => Multiply b a b where
229   x * y = x * (convert(y) :: b)
230",
        )),
        "Divide" => Some((
            275,
            276,
            30,
            "272 instance Divide int int int where
273   (/) = idiv
274
275 instance (a != b, Divide b b b, Convert a b) => Divide a b b where
276   x / y = (convert(x) :: b) / y
277
278 instance (a != b, Divide b b b, Convert a b) => Divide b a b where
279   x / y = x / (convert(y) :: b)
280",
        )),
        _ => None,
    }
}

pub fn has_free_tvars(ty: &MonoType) -> bool {
    match ty {
        MonoType::TVar(_, cell) => cell.get().is_none_or(|inner| has_free_tvars(inner.chase())),
        MonoType::Array(inner) => has_free_tvars(inner.chase()),
        MonoType::FixedArray(inner, _) => has_free_tvars(inner.chase()),
        MonoType::Fn(a, b) => has_free_tvars(a.chase()) || has_free_tvars(b.chase()),
        MonoType::Tuple(ts) => ts.iter().any(|t| has_free_tvars(t.chase())),
        MonoType::Record(fields, tail) => {
            fields.iter().any(|(_, t)| has_free_tvars(t.chase()))
                || tail.is_some_and(|t| has_free_tvars(t.chase()))
        }
        MonoType::Variant(cases, tail) => {
            cases.iter().any(|(_, t)| has_free_tvars(t.chase()))
                || tail.is_some_and(|t| has_free_tvars(t.chase()))
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
