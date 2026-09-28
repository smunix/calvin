use itertools::Itertools;
use std::cell::Cell;

/// Value Object representing a unique Hindley-Milner type variable identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TVarId(pub usize);

impl TVarId {
    pub const fn new(id: usize) -> Self {
        Self(id)
    }

    pub const fn as_usize(self) -> usize {
        self.0
    }
}

impl std::fmt::Display for TVarId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<usize> for TVarId {
    fn from(id: usize) -> Self {
        Self(id)
    }
}

impl From<TVarId> for usize {
    fn from(id: TVarId) -> Self {
        id.0
    }
}

/// Value Object representing a Skolemized type generator variable identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TGenId(pub usize);

impl TGenId {
    pub const fn new(id: usize) -> Self {
        Self(id)
    }

    pub const fn as_usize(self) -> usize {
        self.0
    }
}

impl std::fmt::Display for TGenId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<usize> for TGenId {
    fn from(id: usize) -> Self {
        Self(id)
    }
}

impl From<TGenId> for usize {
    fn from(id: TGenId) -> Self {
        id.0
    }
}

/// Primitive types supported by Calvin, matching Hobbes parity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Prim {
    Unit,
    Bool,
    Char,
    Byte,
    Short,
    Int,
    Long,
    Int128,
    Float,
    Double,
    Time,
    TimeSpan,
    DateTime,
}

/// A structural monotype, allocated in a bumpalo arena.
#[derive(PartialEq, Eq)]
pub enum MonoType<'a> {
    /// A primitive type.
    Prim(Prim),
    /// A type variable for Hindley-Milner inference.
    /// It uses interior mutability (Cell) for zero-cost unification and holds a sequential creation id.
    TVar(usize, Cell<Option<&'a MonoType<'a>>>),
    /// A Skolemized type generator variable (used during instantiation and in the TypeMap).
    TGen(usize),
    /// An array type: [T].
    Array(&'a MonoType<'a>),
    /// A fixed-size array type: [T; N].
    FixedArray(&'a MonoType<'a>, usize),
    /// A function type: A -> B.
    Fn(&'a MonoType<'a>, &'a MonoType<'a>),
    /// A tuple type: (T1, T2, ...).
    Tuple(&'a [&'a MonoType<'a>]),
    Constraint(&'a str, &'a [&'a MonoType<'a>], &'a MonoType<'a>),
    /// A record type: { f1: T1, f2: T2, ... }.
    Record(&'a [(&'a str, &'a MonoType<'a>)], Option<&'a MonoType<'a>>),
    /// A variant type: | v1: T1, v2: T2, ... |.
    Variant(&'a [(&'a str, &'a MonoType<'a>)], Option<&'a MonoType<'a>>),
    /// A type application (e.g. for user-defined types or higher-kinded types).
    App(&'a MonoType<'a>, &'a [&'a MonoType<'a>]),
}

impl<'a> MonoType<'a> {
    /// Dereference a type variable to its unified value, if any.
    /// This follows the chain of unifications to find the representative type.
    pub fn chase<'s>(&'s self) -> &'s MonoType<'a> {
        std::iter::successors(Some(self), |&ty| match ty {
            MonoType::TVar(_, cell) => cell.get(),
            _ => None,
        })
        .last()
        .unwrap()
    }
    pub fn is_primitive(&'a self) -> bool {
        matches!(self.chase(), MonoType::Prim(_))
    }

    pub fn is_function(&'a self) -> bool {
        matches!(self.chase(), MonoType::Fn(_, _))
    }

    pub fn is_record(&'a self) -> bool {
        matches!(self.chase(), MonoType::Record(_, _))
    }

    pub fn is_variant(&'a self) -> bool {
        matches!(self.chase(), MonoType::Variant(_, _))
    }

    pub fn is_tvar(&'a self) -> bool {
        matches!(self.chase(), MonoType::TVar(_, _))
    }
}

/// A type class constraint, e.g., `(Add a b c)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Constraint<'a> {
    pub class_name: &'a str,
    pub arguments: &'a [&'a MonoType<'a>],
}

impl<'a> Constraint<'a> {
    pub fn new(class_name: &'a str, arguments: &'a [&'a MonoType<'a>]) -> Self {
        Self {
            class_name,
            arguments,
        }
    }

    pub fn class_name(&self) -> &'a str {
        self.class_name
    }

    pub fn arguments(&self) -> &'a [&'a MonoType<'a>] {
        self.arguments
    }
}

/// A qualified type, which is a structural monotype guarded by a set of constraints.
/// e.g., `(Add a b c) => a -> b -> c`
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QualType<'a> {
    pub constraints: &'a [Constraint<'a>],
    pub ty: &'a MonoType<'a>,
}

impl<'a> QualType<'a> {
    pub fn new(constraints: &'a [Constraint<'a>], ty: &'a MonoType<'a>) -> Self {
        Self { constraints, ty }
    }

    pub fn constraints(&self) -> &'a [Constraint<'a>] {
        self.constraints
    }

    pub fn ty(&self) -> &'a MonoType<'a> {
        self.ty
    }

    pub fn is_monomorphic(&self) -> bool {
        self.constraints.is_empty()
    }
}

use crate::context::TypeContext;
use std::collections::{HashMap, HashSet};

impl<'a> MonoType<'a> {
    pub fn free_tvars(&'a self, vars: &mut HashSet<usize>) {
        let ty = self.chase();
        match ty {
            MonoType::TVar(id, _) => {
                vars.insert(*id);
            }
            MonoType::Array(inner) => inner.free_tvars(vars),
            MonoType::FixedArray(inner, _) => inner.free_tvars(vars),
            MonoType::Fn(a, b) => {
                a.free_tvars(vars);
                b.free_tvars(vars);
            }
            MonoType::Tuple(elems) => {
                elems.iter().for_each(|e| e.free_tvars(vars));
            }
            MonoType::Record(fields, tail) => {
                fields.iter().for_each(|(_, t)| t.free_tvars(vars));
                if let Some(r) = tail {
                    r.free_tvars(vars);
                }
            }
            MonoType::Variant(cases, tail) => {
                cases.iter().for_each(|(_, t)| t.free_tvars(vars));
                if let Some(r) = tail {
                    r.free_tvars(vars);
                }
            }
            MonoType::App(f, args) => {
                f.free_tvars(vars);
                args.iter().for_each(|a| a.free_tvars(vars));
            }
            MonoType::Constraint(_, args, inner) => {
                args.iter().for_each(|a| a.free_tvars(vars));
                inner.free_tvars(vars);
            }
            _ => {}
        }
    }

    pub fn generalize(
        &'a self,
        ctx: &'a TypeContext,
        env_vars: &HashSet<usize>,
        mapping: &mut HashMap<usize, usize>,
    ) -> &'a MonoType<'a> {
        let ty = self.chase();
        match ty {
            MonoType::TVar(id, _) => {
                if env_vars.contains(id) {
                    ty
                } else {
                    let gen_id = if let Some(&i) = mapping.get(id) {
                        i
                    } else {
                        let i = mapping.len();
                        mapping.insert(*id, i);
                        i
                    };
                    &*ctx.alloc(MonoType::TGen(gen_id))
                }
            }
            MonoType::Array(inner) => {
                &*ctx.alloc(MonoType::Array(inner.generalize(ctx, env_vars, mapping)))
            }
            MonoType::FixedArray(inner, n) => &*ctx.alloc(MonoType::FixedArray(
                inner.generalize(ctx, env_vars, mapping),
                *n,
            )),
            MonoType::Fn(a, b) => &*ctx.alloc(MonoType::Fn(
                a.generalize(ctx, env_vars, mapping),
                b.generalize(ctx, env_vars, mapping),
            )),
            MonoType::Tuple(elems) => {
                let new_elems: Vec<_> = elems
                    .iter()
                    .map(|e| e.generalize(ctx, env_vars, mapping))
                    .collect();
                &*ctx.alloc(MonoType::Tuple(ctx.alloc(new_elems)))
            }
            MonoType::Record(fields, tail) => {
                let new_fields: Vec<_> = fields
                    .iter()
                    .map(|(name, e)| (*name, e.generalize(ctx, env_vars, mapping)))
                    .collect();
                let new_tail = tail.map(|t| t.generalize(ctx, env_vars, mapping));
                &*ctx.alloc(MonoType::Record(ctx.alloc(new_fields), new_tail))
            }
            MonoType::Variant(cases, tail) => {
                let new_cases: Vec<_> = cases
                    .iter()
                    .map(|(name, e)| (*name, e.generalize(ctx, env_vars, mapping)))
                    .collect();
                let new_tail = tail.map(|t| t.generalize(ctx, env_vars, mapping));
                &*ctx.alloc(MonoType::Variant(ctx.alloc(new_cases), new_tail))
            }
            MonoType::App(f, args) => {
                let f_new = f.generalize(ctx, env_vars, mapping);
                let new_args: Vec<_> = args
                    .iter()
                    .map(|a| a.generalize(ctx, env_vars, mapping))
                    .collect();
                &*ctx.alloc(MonoType::App(f_new, ctx.alloc(new_args)))
            }
            MonoType::Constraint(name, args, inner) => {
                let new_args: Vec<_> = args
                    .iter()
                    .map(|a| a.generalize(ctx, env_vars, mapping))
                    .collect();
                &*ctx.alloc(MonoType::Constraint(
                    name,
                    ctx.alloc(new_args),
                    inner.generalize(ctx, env_vars, mapping),
                ))
            }
            MonoType::TGen(_) | MonoType::Prim(_) => ty,
        }
    }

    pub fn instantiate(
        &'a self,
        ctx: &'a TypeContext,
        fresh: &[&'a MonoType<'a>],
    ) -> &'a MonoType<'a> {
        let ty = self.chase();
        match ty {
            MonoType::TGen(i) => fresh[*i],
            MonoType::Array(inner) => &*ctx.alloc(MonoType::Array(inner.instantiate(ctx, fresh))),
            MonoType::FixedArray(inner, n) => {
                &*ctx.alloc(MonoType::FixedArray(inner.instantiate(ctx, fresh), *n))
            }
            MonoType::Fn(a, b) => &*ctx.alloc(MonoType::Fn(
                a.instantiate(ctx, fresh),
                b.instantiate(ctx, fresh),
            )),
            MonoType::Tuple(elems) => {
                let new_elems: Vec<_> = elems
                    .iter()
                    .map(|e| e.instantiate(ctx, fresh))
                    .collect();
                &*ctx.alloc(MonoType::Tuple(ctx.alloc(new_elems)))
            }
            MonoType::Record(fields, tail) => {
                let new_fields: Vec<_> = fields
                    .iter()
                    .map(|(name, e)| (*name, e.instantiate(ctx, fresh)))
                    .collect();
                let new_tail = tail.map(|t| t.instantiate(ctx, fresh));
                &*ctx.alloc(MonoType::Record(ctx.alloc(new_fields), new_tail))
            }
            MonoType::Variant(cases, tail) => {
                let new_cases: Vec<_> = cases
                    .iter()
                    .map(|(name, e)| (*name, e.instantiate(ctx, fresh)))
                    .collect();
                let new_tail = tail.map(|t| t.instantiate(ctx, fresh));
                &*ctx.alloc(MonoType::Variant(ctx.alloc(new_cases), new_tail))
            }
            MonoType::App(f, args) => {
                let f_new = f.instantiate(ctx, fresh);
                let new_args: Vec<_> = args
                    .iter()
                    .map(|a| a.instantiate(ctx, fresh))
                    .collect();
                &*ctx.alloc(MonoType::App(f_new, ctx.alloc(new_args)))
            }
            MonoType::Constraint(name, args, inner) => {
                let new_args: Vec<_> = args
                    .iter()
                    .map(|a| a.instantiate(ctx, fresh))
                    .collect();
                &*ctx.alloc(MonoType::Constraint(
                    name,
                    ctx.alloc(new_args),
                    inner.instantiate(ctx, fresh),
                ))
            }
            MonoType::TVar(_, _) | MonoType::Prim(_) => ty,
        }
    }

    pub fn max_tgen(&'a self) -> Option<usize> {
        let ty = self.chase();
        match ty {
            MonoType::TGen(i) => Some(*i),
            MonoType::Array(inner) => inner.max_tgen(),
            MonoType::FixedArray(inner, _) => inner.max_tgen(),
            MonoType::Fn(a, b) => a.max_tgen().into_iter().chain(b.max_tgen()).max(),
            MonoType::Tuple(elems) => elems.iter().filter_map(|e| e.max_tgen()).max(),
            MonoType::Record(fields, tail) => fields
                .iter()
                .filter_map(|(_, e)| e.max_tgen())
                .chain(tail.and_then(|t| t.max_tgen()))
                .max(),
            MonoType::Variant(cases, tail) => cases
                .iter()
                .filter_map(|(_, e)| e.max_tgen())
                .chain(tail.and_then(|t| t.max_tgen()))
                .max(),
            MonoType::App(f, args) => f
                .max_tgen()
                .into_iter()
                .chain(args.iter().filter_map(|e| e.max_tgen()))
                .max(),
            MonoType::Constraint(_, args, inner) => args
                .iter()
                .filter_map(|e| e.max_tgen())
                .chain(inner.max_tgen())
                .max(),
            _ => None,
        }
    }
}

use std::fmt;
impl<'a> fmt::Debug for MonoType<'a> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let ty = self.chase();
        match ty {
            MonoType::Prim(p) => write!(f, "{:?}", p),
            MonoType::TVar(id, _) => write!(f, ".t{}", id),
            MonoType::TGen(i) => write!(f, "{}", (b'a' + (*i as u8)) as char),
            MonoType::Fn(a, b) => write!(f, "({:?}) -> {:?}", a, b),
            MonoType::Tuple(elems) => {
                write!(f, "({})", elems.iter().map(|e| format!("{:?}", e)).join(" * "))
            }
            MonoType::Constraint(name, args, inner) => {
                let args_str = args.iter().map(|arg| format!("{:?}", arg)).join(" ");
                write!(
                    f,
                    "{} {}, packsto (((a) * b) -> c * (a)) exists E.((E * b) -> c * E) => {:?}",
                    name, args_str, inner
                )
            }
            _ => write!(f, "Other"),
        }
    }
}

pub fn compare_monotype<'a, 'b>(a: &'a MonoType<'a>, b: &'b MonoType<'b>) -> std::cmp::Ordering {
    let a = a.chase();
    let b = b.chase();
    fn case_id(ty: &MonoType) -> usize {
        match ty {
            MonoType::Prim(_) => 0,
            MonoType::TVar(_, _) => 2,
            MonoType::TGen(_) => 3,
            MonoType::FixedArray(_, _) => 4,
            MonoType::Array(_) => 5,
            MonoType::Variant(_, _) => 6,
            MonoType::Record(_, _) => 7,
            MonoType::Fn(_, _) => 8,
            MonoType::Tuple(_) => 9,
            MonoType::App(_, _) => 13,
            MonoType::Constraint(_, _, _) => 16,
        }
    }

    match (a, b) {
        (MonoType::TVar(id1, _), MonoType::TVar(id2, _)) => id1.cmp(id2),
        (MonoType::TGen(id1), MonoType::TGen(id2)) => id1.cmp(id2),
        (MonoType::Prim(p1), MonoType::Prim(p2)) => format!("{:?}", p1).cmp(&format!("{:?}", p2)),
        (MonoType::Fn(a1, r1), MonoType::Fn(a2, r2)) => {
            compare_monotype(a1, a2).then_with(|| compare_monotype(r1, r2))
        }
        (MonoType::Tuple(t1), MonoType::Tuple(t2)) => t1
            .iter()
            .zip(t2.iter())
            .map(|(e1, e2)| compare_monotype(e1, e2))
            .find(|ord| *ord != std::cmp::Ordering::Equal)
            .unwrap_or_else(|| t1.len().cmp(&t2.len())),
        _ => case_id(a).cmp(&case_id(b)),
    }
}

pub fn compare_constraint<'a, 'b>(
    c1: &(&str, Vec<&'a MonoType<'a>>),
    c2: &(&str, Vec<&'b MonoType<'b>>),
) -> std::cmp::Ordering {
    match c1.0.cmp(c2.0) {
        std::cmp::Ordering::Equal => c1
            .1
            .iter()
            .zip(c2.1.iter())
            .map(|(a1, a2)| compare_monotype(a1, a2))
            .find(|ord| *ord != std::cmp::Ordering::Equal)
            .unwrap_or_else(|| c1.1.len().cmp(&c2.1.len())),
        ord => ord,
    }
}

pub fn format_qual_type<'a>(
    ty: &'a MonoType<'a>,
    constraints: &[(&'a str, Vec<&'a MonoType<'a>>)],
) -> String {
    let mut set = std::collections::HashSet::new();
    ty.free_tvars(&mut set);
    constraints.iter().for_each(|(_, cargs)| {
        cargs.iter().for_each(|arg| arg.free_tvars(&mut set));
    });

    let var_ids: std::collections::BTreeSet<usize> = set.into_iter().collect();

    let names: std::collections::HashMap<usize, String> = var_ids
        .into_iter()
        .enumerate()
        .map(|(idx, id)| {
            let name = if idx < 26 {
                ((b'a' + idx as u8) as char).to_string()
            } else {
                format!("t{}", idx - 26)
            };
            (id, name)
        })
        .collect();

    let cst_strs: Vec<String> = constraints
        .iter()
        .cloned()
        .sorted_by(compare_constraint)
        .map(|(name, cargs)| {
            let args_str = cargs.iter().map(|arg| format_mono(arg, &names)).join(" ");
            format!("{} {}", name, args_str)
        })
        .unique()
        .collect();

    let ty_str = format_mono(ty, &names);
    if cst_strs.is_empty() {
        ty_str
    } else {
        format!("{} => {}", cst_strs.join(", "), ty_str)
    }
}

pub fn format_mono<'a>(
    ty: &'a MonoType<'a>,
    names: &std::collections::HashMap<usize, String>,
) -> String {
    let ty = ty.chase();
    match ty {
        MonoType::Prim(p) => match p {
            Prim::Unit => "()".to_string(),
            Prim::Bool => "bool".to_string(),
            Prim::Char => "char".to_string(),
            Prim::Byte => "byte".to_string(),
            Prim::Short => "short".to_string(),
            Prim::Int => "int".to_string(),
            Prim::Long => "long".to_string(),
            Prim::Int128 => "int128".to_string(),
            Prim::Float => "float".to_string(),
            Prim::Double => "double".to_string(),
            Prim::Time => "time".to_string(),
            Prim::TimeSpan => "timespan".to_string(),
            Prim::DateTime => "datetime".to_string(),
        },
        MonoType::TVar(id, _) => names
            .get(id)
            .cloned()
            .unwrap_or_else(|| format!(".t{}", id)),
        MonoType::TGen(i) => {
            if *i < 26 {
                ((b'a' + *i as u8) as char).to_string()
            } else {
                format!("t{}", *i - 26)
            }
        }
        MonoType::Fn(a, b) => {
            let a_str = format_mono(a, names);
            let b_str = format_mono(b, names);
            let a_formatted = if a_str.starts_with('(') && a_str.ends_with(')') {
                a_str
            } else {
                format!("({})", a_str)
            };
            format!("{} -> {}", a_formatted, b_str)
        }
        MonoType::Tuple(elems) => {
            let parts = elems.iter().map(|e| format_mono(e, names)).join(" * ");
            format!("({})", parts)
        }
        MonoType::Array(inner) => format!("[{}]", format_mono(inner, names)),
        MonoType::FixedArray(inner, len) => format!("[:{}|{}:]", format_mono(inner, names), len),
        MonoType::Record(fields, _) => {
            if fields.is_empty() {
                "{}".to_string()
            } else {
                let parts = fields
                    .iter()
                    .map(|(n, t)| format!("{}:{}", n, format_mono(t, names)))
                    .join(", ");
                format!("{{ {} }}", parts)
            }
        }
        MonoType::Variant(cases, _) => {
            let parts = cases
                .iter()
                .map(|(n, t)| format!("{}:{}", n, format_mono(t, names)))
                .join(", ");
            format!("|{}|", parts)
        }
        MonoType::Constraint(_, _, inner) => format_mono(inner, names),
        _ => "unknown".to_string(),
    }
}

pub fn format_qual_type_no_simpl<'a>(
    ty: &'a MonoType<'a>,
    constraints: &[(&'a str, Vec<&'a MonoType<'a>>)],
) -> String {
    let cst_strs: Vec<String> = constraints
        .iter()
        .cloned()
        .sorted_by(compare_constraint)
        .map(|(name, cargs)| {
            let args_str = cargs.iter().copied().map(format_mono_no_simpl).join(" ");
            format!("{} {}", name, args_str)
        })
        .unique()
        .collect();

    let ty_str = format_mono_no_simpl(ty);
    match cst_strs.len() {
        0 => ty_str,
        1 => format!("{} => {}", cst_strs[0], ty_str),
        _ => format!("({}) => {}", cst_strs.join(", "), ty_str),
    }
}

pub fn format_mono_no_simpl<'a>(ty: &'a MonoType<'a>) -> String {
    let ty = ty.chase();
    match ty {
        MonoType::Prim(p) => match p {
            Prim::Unit => "()".to_string(),
            Prim::Bool => "bool".to_string(),
            Prim::Char => "char".to_string(),
            Prim::Byte => "byte".to_string(),
            Prim::Short => "short".to_string(),
            Prim::Int => "int".to_string(),
            Prim::Long => "long".to_string(),
            Prim::Int128 => "int128".to_string(),
            Prim::Float => "float".to_string(),
            Prim::Double => "double".to_string(),
            Prim::Time => "time".to_string(),
            Prim::TimeSpan => "timespan".to_string(),
            Prim::DateTime => "datetime".to_string(),
        },
        MonoType::TVar(id, _) => format!(".t{}", id),
        MonoType::TGen(i) => format!(".tgen{}", i),
        MonoType::Fn(a, b) => {
            let a_str = format_mono_no_simpl(a);
            let b_str = format_mono_no_simpl(b);
            let a_formatted = if a_str.starts_with('(') && a_str.ends_with(')') {
                a_str
            } else {
                format!("({})", a_str)
            };
            format!("{} -> {}", a_formatted, b_str)
        }
        MonoType::Tuple(elems) => {
            let parts: Vec<_> = elems.iter().map(|e| format_mono_no_simpl(e)).collect();
            format!("({})", parts.join(" * "))
        }
        MonoType::Array(inner) => format!("[{}]", format_mono_no_simpl(inner)),
        MonoType::FixedArray(inner, len) => format!("[:{}|{}:]", format_mono_no_simpl(inner), len),
        MonoType::Record(fields, _) => {
            if fields.is_empty() {
                "{}".to_string()
            } else {
                let parts: Vec<_> = fields
                    .iter()
                    .map(|(n, t)| format!("{}:{}", n, format_mono_no_simpl(t)))
                    .collect();
                format!("{{ {} }}", parts.join(", "))
            }
        }
        MonoType::Variant(cases, _) => {
            let parts: Vec<_> = cases
                .iter()
                .map(|(n, t)| format!("{}:{}", n, format_mono_no_simpl(t)))
                .collect();
            format!("|{}|", parts.join(", "))
        }
        MonoType::Constraint(name, args, inner) => {
            let args_str = args
                .iter()
                .map(|a| format_mono_no_simpl(a))
                .collect::<Vec<_>>()
                .join(" ");
            format!("{} {} => {}", name, args_str, format_mono_no_simpl(inner))
        }
        _ => "unknown".to_string(),
    }
}
