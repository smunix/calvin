use itertools::Itertools;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use crate::context::TypeContext;
use crate::lang::expr::{Expr, ExprVisitor, Literal, Pattern};
use crate::lang::types::{MonoType, Prim};

#[derive(Debug, PartialEq, Eq, Clone)]
pub enum TypeError {
    TypeMismatch,
    OccursCheckFailed,
    UnboundVariable(String),
    UnsatisfiableConstraint {
        class_name: String,
        args: Vec<String>,
        explanation: Option<String>,
    },
}

#[derive(Clone)]
pub struct TypeEnv<'a> {
    parent: Option<Rc<TypeEnv<'a>>>,
    bindings: HashMap<String, &'a MonoType<'a>>,
}

impl<'a> TypeEnv<'a> {
    pub fn new() -> Self {
        TypeEnv {
            parent: None,
            bindings: HashMap::new(),
        }
    }

    pub fn insert(&mut self, name: &str, ty: &'a MonoType<'a>) {
        self.bindings.insert(name.to_string(), ty);
    }

    pub fn extend(parent: Rc<TypeEnv<'a>>, name: &str, ty: &'a MonoType<'a>) -> Self {
        let mut bindings = HashMap::new();
        bindings.insert(name.to_string(), ty);
        TypeEnv {
            parent: Some(parent),
            bindings,
        }
    }

    pub fn lookup(&self, name: &str) -> Option<&'a MonoType<'a>> {
        if let Some(ty) = self.bindings.get(name) {
            Some(ty)
        } else if let Some(ref p) = self.parent {
            p.lookup(name)
        } else {
            None
        }
    }

    pub fn free_tvars(&self, vars: &mut HashSet<usize>) {
        self.bindings.values().for_each(|ty| ty.free_tvars(vars));
        if let Some(ref p) = self.parent {
            p.free_tvars(vars);
        }
    }
}

impl<'a> Default for TypeEnv<'a> {
    fn default() -> Self {
        Self::new()
    }
}

pub struct TypeInference<'a> {
    pub map: crate::lang::typemap::TypeMap<'a, &'a Expr<'a>>,
    pub ctx: &'a TypeContext,
    pub env: Rc<TypeEnv<'a>>,
    pub classes: Rc<crate::lang::typeclass::TypeClassRegistry<'a>>,
    pub constraints: std::cell::RefCell<Vec<&'a MonoType<'a>>>,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum RowKind {
    Record,
    Variant,
}

#[inline]
fn is_arithmetic_op(name: &str) -> bool {
    matches!(name, "Add" | "Subtract" | "Multiply" | "Divide")
}

#[inline]
fn is_arithmetic_prim(prim: Prim) -> bool {
    matches!(
        prim,
        Prim::Int | Prim::Float | Prim::Double | Prim::Long | Prim::Short | Prim::Byte | Prim::Char
    )
}

fn is_ground_arithmetic_satisfied<'a>(name: &str, args: &[&'a MonoType<'a>]) -> bool {
    if args.len() != 3 || !is_arithmetic_op(name) {
        return false;
    }
    match (args[0].chase(), args[1].chase(), args[2].chase()) {
        (MonoType::Prim(p1), MonoType::Prim(p2), MonoType::Prim(p3)) => {
            (p1 == p2 && p2 == p3 && is_arithmetic_prim(*p1))
                || (*p1 == Prim::DateTime && *p2 == Prim::Time && *p3 == Prim::DateTime)
                || (*p1 == Prim::Time && *p2 == Prim::TimeSpan && *p3 == Prim::Time)
        }
        _ => false,
    }
}

impl<'a> TypeInference<'a> {
    pub fn solve_constraints(&self) -> Result<(), TypeError> {
        let constraints = self.constraints.borrow().clone();
        constraints.iter().try_for_each(|constraint| {
            if let MonoType::Constraint(name, args, _) = constraint.chase() {
                // 1. First consult registry functional dependencies
                let unifications = self.classes.refine_fundeps(name, args);
                unifications
                    .into_iter()
                    .try_for_each(|(target, resolved)| self.unify(target, resolved))?;

                // 2. Fallback built-in arithmetic solver across primitives
                if is_arithmetic_op(name) && args.len() == 3 {
                    let a = args[0].chase();
                    let b = args[1].chase();
                    let result_type = args[2].chase();

                    if let (MonoType::Prim(p1), MonoType::Prim(p2)) = (a, b) {
                        if p1 == p2 && is_arithmetic_prim(*p1) {
                            self.unify(result_type, &*self.ctx.alloc(MonoType::Prim(*p1)))?;
                        }
                    }
                }
            }
            Ok(())
        })?;

        self.constraints.borrow().iter().try_for_each(|constraint| {
            if let MonoType::Constraint(name, args, _) = constraint.chase() {
                if !self.classes.is_satisfied(name, args)
                    && !is_ground_arithmetic_satisfied(name, args)
                {
                    if let Some(explanation) = self.check_unsatisfiable(name, args) {
                        let arg_strs: Vec<String> = args
                            .iter()
                            .map(|arg| {
                                let chased = arg.chase();
                                if crate::lang::typeclass::has_free_tvars(chased) {
                                    "a".to_string()
                                } else {
                                    crate::lang::types::format_mono_no_simpl(chased)
                                }
                            })
                            .collect();
                        return Err(TypeError::UnsatisfiableConstraint {
                            class_name: name.to_string(),
                            args: arg_strs,
                            explanation,
                        });
                    }
                }
            }
            Ok(())
        })
    }

    fn check_unsatisfiable(&self, name: &str, args: &[&'a MonoType<'a>]) -> Option<Option<String>> {
        if let Some(explanation) = self.classes.explain_unsatisfiable(name, args) {
            return Some(Some(explanation));
        }

        let class_def = self.classes.classes.get(name)?;
        if !class_def.fundeps.is_empty() {
            let from_closed = class_def.fundeps.iter().any(|(from, _)| {
                from.iter().all(|&idx| {
                    idx < args.len() && !crate::lang::typeclass::has_free_tvars(args[idx].chase())
                })
            });
            if from_closed {
                return Some(None);
            }
        }
        None
    }

    pub fn residual_constraints(&self) -> Vec<(&'a str, Vec<&'a MonoType<'a>>)> {
        self.constraints
            .borrow()
            .iter()
            .filter_map(|constraint| {
                if let MonoType::Constraint(name, args, _) = constraint.chase() {
                    if !self.classes.is_satisfied(name, args)
                        && !is_ground_arithmetic_satisfied(name, args)
                    {
                        let chased_args: Vec<&'a MonoType<'a>> =
                            args.iter().map(|arg| arg.chase()).collect();
                        return Some((*name, chased_args));
                    }
                }
                None
            })
            .sorted_by(crate::lang::types::compare_constraint)
            .dedup_by(|a, b| a.0 == b.0 && a.1 == b.1)
            .collect()
    }

    pub fn bind(&mut self, name: &str, ty: &'a MonoType<'a>) {
        self.env = std::rc::Rc::new(TypeEnv::extend(self.env.clone(), name, ty));
    }

    pub fn lookup(&self, name: &str) -> Option<&'a MonoType<'a>> {
        self.env.lookup(name)
    }

    pub fn with_env_and_classes(
        ctx: &'a TypeContext,
        env: Rc<TypeEnv<'a>>,
        classes: Rc<crate::lang::typeclass::TypeClassRegistry<'a>>,
    ) -> Self {
        let mut map = crate::lang::typemap::TypeMap::new();
        map.insert(
            crate::lang::typemap::EdgeKey::Class("Convert"),
            &*ctx.alloc(Expr::Var("dict_Convert")),
        );
        map.insert(
            crate::lang::typemap::EdgeKey::Class("Show"),
            &*ctx.alloc(Expr::Var("dict_Show")),
        );

        TypeInference {
            ctx,
            env,
            map,
            classes,
            constraints: std::cell::RefCell::new(Vec::new()),
        }
    }

    pub fn new(ctx: &'a TypeContext) -> Self {
        Self::with_env_and_classes(
            ctx,
            Rc::new(TypeEnv::new()),
            Rc::new(crate::lang::typeclass::TypeClassRegistry::new()),
        )
    }

    pub fn peel_constraints(
        ty: &'a MonoType<'a>,
    ) -> (Vec<(&'a str, &'a [&'a MonoType<'a>])>, &'a MonoType<'a>) {
        let mut curr = ty.chase();
        let constraints = std::iter::from_fn(|| {
            if let MonoType::Constraint(name, args, inner) = curr {
                curr = inner.chase();
                Some((*name, *args))
            } else {
                None
            }
        })
        .collect();
        (constraints, curr)
    }

    pub fn wrap_constraints(
        &self,
        constraints: Vec<(&'a str, &'a [&'a MonoType<'a>])>,
        ty: &'a MonoType<'a>,
    ) -> &'a MonoType<'a> {
        constraints.into_iter().rev().fold(ty, |acc, (name, args)| {
            &*self.ctx.alloc(MonoType::Constraint(name, args, acc))
        })
    }

    fn occurs(tvar_id: usize, ty: &'a MonoType<'a>) -> bool {
        let ty = ty.chase();
        if let MonoType::TVar(id, _) = ty {
            if *id == tvar_id {
                return true;
            }
        }
        match ty {
            MonoType::Array(inner) => Self::occurs(tvar_id, inner),
            MonoType::FixedArray(inner, _) => Self::occurs(tvar_id, inner),
            MonoType::Fn(arg, ret) => Self::occurs(tvar_id, arg) || Self::occurs(tvar_id, ret),
            MonoType::Tuple(elems) => elems.iter().any(|e| Self::occurs(tvar_id, e)),
            MonoType::Record(fields, tail) => {
                fields.iter().any(|(_, t)| Self::occurs(tvar_id, t))
                    || tail.is_some_and(|t| Self::occurs(tvar_id, t))
            }
            MonoType::Variant(cases, tail) => {
                cases.iter().any(|(_, t)| Self::occurs(tvar_id, t))
                    || tail.is_some_and(|t| Self::occurs(tvar_id, t))
            }
            MonoType::App(f, args) => {
                Self::occurs(tvar_id, f) || args.iter().any(|a| Self::occurs(tvar_id, a))
            }
            MonoType::Constraint(_, args, inner) => {
                args.iter().any(|a| Self::occurs(tvar_id, a)) || Self::occurs(tvar_id, inner)
            }
            _ => false,
        }
    }

    pub fn unify(&self, t1: &'a MonoType<'a>, t2: &'a MonoType<'a>) -> Result<(), TypeError> {
        let mut t1 = t1.chase();
        let mut t2 = t2.chase();
        if let MonoType::Constraint(_, _, inner) = t1 {
            t1 = inner.chase();
        }
        if let MonoType::Constraint(_, _, inner) = t2 {
            t2 = inner.chase();
        }

        if std::ptr::eq(t1, t2) {
            return Ok(());
        }

        match (t1, t2) {
            (MonoType::TVar(id1, cell1), MonoType::TVar(id2, _cell2)) => {
                if id1 != id2 {
                    cell1.set(Some(t2));
                }
                Ok(())
            }
            (MonoType::TVar(id, cell), other) | (other, MonoType::TVar(id, cell)) => {
                if Self::occurs(*id, other) {
                    return Err(TypeError::OccursCheckFailed);
                }
                cell.set(Some(other));
                Ok(())
            }
            (MonoType::Prim(p1), MonoType::Prim(p2)) if p1 == p2 => Ok(()),
            (MonoType::Array(in1), MonoType::Array(in2)) => self.unify(in1, in2),
            (MonoType::Fn(a1, r1), MonoType::Fn(a2, r2)) => {
                self.unify(a1, a2)?;
                self.unify(r1, r2)
            }
            (MonoType::Tuple(ts1), MonoType::Tuple(ts2)) => {
                if ts1.len() != ts2.len() {
                    return Err(TypeError::TypeMismatch);
                }
                ts1.iter().zip(ts2.iter()).try_for_each(|(a, b)| self.unify(a, b))
            }
            (MonoType::Record(fs1, tail1), MonoType::Record(fs2, tail2)) => {
                self.unify_rows(RowKind::Record, fs1, *tail1, fs2, *tail2)
            }
            (MonoType::Variant(fs1, tail1), MonoType::Variant(fs2, tail2)) => {
                self.unify_rows(RowKind::Variant, fs1, *tail1, fs2, *tail2)
            }
            _ => Err(TypeError::TypeMismatch),
        }
    }

    fn unify_rows(
        &self,
        kind: RowKind,
        fs1: &'a [(&'a str, &'a MonoType<'a>)],
        tail1: Option<&'a MonoType<'a>>,
        fs2: &'a [(&'a str, &'a MonoType<'a>)],
        tail2: Option<&'a MonoType<'a>>,
    ) -> Result<(), TypeError> {
        let map1: std::collections::HashMap<&'a str, &'a MonoType<'a>> =
            fs1.iter().copied().collect();
        let mut map2: std::collections::HashMap<&'a str, &'a MonoType<'a>> =
            fs2.iter().copied().collect();

        let mut diff1 = Vec::new();
        fs1.iter().try_for_each(|(n, t1_f)| {
            if let Some(t2_f) = map2.remove(n) {
                self.unify(t1_f, t2_f)
            } else {
                diff1.push((*n, *t1_f));
                Ok(())
            }
        })?;

        let diff2: Vec<_> = fs2
            .iter()
            .copied()
            .filter(|(n, _)| !map1.contains_key(n))
            .collect();

        if diff1.is_empty() && diff2.is_empty() {
            if let (Some(r1), Some(r2)) = (tail1, tail2) {
                self.unify(r1, r2)?;
            }
            return Ok(());
        }

        let new_tail = self.fresh_tvar();

        if !diff1.is_empty() {
            if let Some(r2) = tail2 {
                let diff1_slice = self.ctx.arena().alloc_slice_copy(&diff1);
                let ext2 = match kind {
                    RowKind::Record => self
                        .ctx
                        .alloc(MonoType::Record(diff1_slice, Some(new_tail))),
                    RowKind::Variant => self
                        .ctx
                        .alloc(MonoType::Variant(diff1_slice, Some(new_tail))),
                };
                self.unify(r2, ext2)?;
            } else {
                return Err(TypeError::TypeMismatch);
            }
        }

        if !diff2.is_empty() {
            if let Some(r1) = tail1 {
                let diff2_slice = self.ctx.arena().alloc_slice_copy(&diff2);
                let ext1 = match kind {
                    RowKind::Record => self
                        .ctx
                        .alloc(MonoType::Record(diff2_slice, Some(new_tail))),
                    RowKind::Variant => self
                        .ctx
                        .alloc(MonoType::Variant(diff2_slice, Some(new_tail))),
                };
                self.unify(r1, ext1)?;
            } else {
                return Err(TypeError::TypeMismatch);
            }
        }

        Ok(())
    }

    pub fn fresh_tvar(&self) -> &'a MonoType<'a> {
        let id = self.ctx.fresh_tvar_id();
        &*self
            .ctx
            .alloc(MonoType::TVar(id, std::cell::Cell::new(None)))
    }

    fn with_bindings<R>(
        &mut self,
        bindings: Vec<(&'a str, &'a MonoType<'a>)>,
        f: impl FnOnce(&mut Self) -> R,
    ) -> R {
        let prev_env = self.env.clone();
        let new_env = bindings.into_iter().fold(prev_env.clone(), |acc, (name, ty)| {
            Rc::new(TypeEnv::extend(acc, name, ty))
        });
        self.env = new_env;
        let result = f(self);
        self.env = prev_env;
        result
    }

    fn visit_pattern(
        &mut self,
        pat: &Pattern<'a>,
        expected_ty: &'a MonoType<'a>,
        bindings: &mut Vec<(&'a str, &'a MonoType<'a>)>,
    ) -> Result<(), TypeError> {
        match pat {
            Pattern::Any => Ok(()),
            Pattern::Var(name) => {
                bindings.push((name, expected_ty));
                Ok(())
            }
            Pattern::Literal(lit) => {
                let lit_ty = match lit {
                    Literal::Unit => Prim::Unit,
                    Literal::Bool(_) => Prim::Bool,
                    Literal::Char(_) => Prim::Char,
                    Literal::Int(_) => Prim::Int,
                    Literal::Float(_) => Prim::Float,
                    Literal::Double(_) => Prim::Double,
                    Literal::String(_) => {
                        return self.unify(
                            expected_ty,
                            &*self.ctx.alloc(MonoType::Array(
                                &*self.ctx.alloc(MonoType::Prim(Prim::Char)),
                            )),
                        )
                    }
                };
                self.unify(expected_ty, &*self.ctx.alloc(MonoType::Prim(lit_ty)))
            }
            Pattern::Tuple(pats) => {
                let elem_tys: Vec<_> = (0..pats.len()).map(|_| self.fresh_tvar()).collect();
                let tup_ty = &*self.ctx.alloc(MonoType::Tuple(
                    self.ctx.arena().alloc_slice_clone(&elem_tys),
                ));
                self.unify(expected_ty, tup_ty)?;
                pats.iter()
                    .zip(elem_tys.iter())
                    .try_for_each(|(p, elem_ty)| self.visit_pattern(p, elem_ty, bindings))
            }
            Pattern::Record(fields) => {
                let field_tys: Vec<_> = fields
                    .iter()
                    .map(|(name, _)| (*name, self.fresh_tvar()))
                    .collect();
                let rec_ty = &*self.ctx.alloc(MonoType::Record(
                    self.ctx.arena().alloc_slice_clone(&field_tys),
                    None,
                ));
                self.unify(expected_ty, rec_ty)?;
                fields
                    .iter()
                    .zip(field_tys.iter())
                    .try_for_each(|((_, p), (_, f_ty))| self.visit_pattern(p, f_ty, bindings))
            }
            Pattern::Variant(tag, payload) => {
                let payload_ty = self.fresh_tvar();
                self.visit_pattern(payload, payload_ty, bindings)?;
                let var_ty = &*self.ctx.alloc(MonoType::Variant(
                    self.ctx.arena().alloc_slice_clone(&[(*tag, payload_ty)]),
                    None,
                ));
                self.unify(expected_ty, var_ty)
            }
        }
    }

    fn apply_tuple_domain(
        &mut self,
        elem_tys: &'a [&'a MonoType<'a>],
        ret: &'a MonoType<'a>,
        constraints: Vec<(&'a str, &'a [&'a MonoType<'a>])>,
        args: &'a [&'a Expr<'a>],
    ) -> Result<&'a MonoType<'a>, TypeError> {
        let chased_dom = &*self.ctx.alloc(MonoType::Tuple(elem_tys));

        // If single argument passed:
        if args.len() == 1 {
            let arg_ty = self.visit(args[0])?;
            if let MonoType::Tuple(arg_elem_tys) = arg_ty.chase() {
                if arg_elem_tys.len() == elem_tys.len() {
                    self.unify(arg_ty, chased_dom)?;
                    return Ok(self.wrap_constraints(constraints, ret));
                }
            }

            // Otherwise auto-curry: apply single argument to elem_tys[0]
            self.unify(arg_ty, elem_tys[0])?;
            let rem_tys = &elem_tys[1..];
            let rem_dom = if rem_tys.len() == 1 {
                rem_tys[0]
            } else {
                &*self
                    .ctx
                    .alloc(MonoType::Tuple(self.ctx.arena().alloc_slice_clone(rem_tys)))
            };
            let rem_fn = &*self.ctx.alloc(MonoType::Fn(rem_dom, ret));
            return Ok(self.wrap_constraints(constraints, rem_fn));
        }

        // If multiple arguments matching tuple domain length:
        if elem_tys.len() == args.len() && args.len() > 1 {
            args.iter()
                .zip(elem_tys.iter())
                .try_for_each(|(arg, param_ty)| {
                    let arg_ty = self.visit(arg)?;
                    self.unify(arg_ty, param_ty)
                })?;
            return Ok(self.wrap_constraints(constraints, ret));
        }

        // If multiple arguments passed, but fewer than tuple domain length (partial application):
        if args.len() > 1 && args.len() < elem_tys.len() {
            args.iter()
                .zip(elem_tys.iter())
                .try_for_each(|(arg, param_ty)| {
                    let arg_ty = self.visit(arg)?;
                    self.unify(arg_ty, param_ty)
                })?;
            let rem_tys = &elem_tys[args.len()..];
            let rem_dom = if rem_tys.len() == 1 {
                rem_tys[0]
            } else {
                &*self
                    .ctx
                    .alloc(MonoType::Tuple(self.ctx.arena().alloc_slice_clone(rem_tys)))
            };
            let rem_fn = &*self.ctx.alloc(MonoType::Fn(rem_dom, ret));
            return Ok(self.wrap_constraints(constraints, rem_fn));
        }

        // If more arguments passed than tuple domain length:
        args[..elem_tys.len()]
            .iter()
            .zip(elem_tys.iter())
            .try_for_each(|(arg, param_ty)| {
                let arg_ty = self.visit(arg)?;
                self.unify(arg_ty, param_ty)
            })?;
        let init_ty = self.wrap_constraints(constraints, ret);
        args[elem_tys.len()..]
            .iter()
            .try_fold(init_ty, |curr_ty, arg| {
                let ret_ty = self.fresh_tvar();
                let arg_ty = self.visit(arg)?;
                let expected_f_ty = &*self.ctx.alloc(MonoType::Fn(arg_ty, ret_ty));
                self.unify(curr_ty, expected_f_ty)?;
                Ok(ret_ty)
            })
    }

    fn try_apply_curried_to_tuple(
        &mut self,
        inner_ty: &'a MonoType<'a>,
        constraints: &[(&'a str, &'a [&'a MonoType<'a>])],
        arg_expr: &'a Expr<'a>,
    ) -> Result<Option<&'a MonoType<'a>>, TypeError> {
        let arg_ty = self.visit(arg_expr)?;
        if let MonoType::Tuple(tup_elems) = arg_ty.chase() {
            if tup_elems.len() > 1 {
                let mut cur = inner_ty;
                let curried_params = std::iter::from_fn(|| {
                    if let MonoType::Fn(param, next) = cur {
                        cur = next.chase();
                        Some(*param)
                    } else {
                        None
                    }
                })
                .take(tup_elems.len())
                .collect_vec();

                if curried_params.len() == tup_elems.len() {
                    tup_elems
                        .iter()
                        .zip(curried_params.iter())
                        .try_for_each(|(elem, param)| self.unify(elem, param))?;
                    return Ok(Some(self.wrap_constraints(constraints.to_vec(), cur)));
                }
            }
        }
        Ok(None)
    }
}

impl<'a> ExprVisitor<'a, Result<&'a MonoType<'a>, TypeError>> for TypeInference<'a> {
    fn visit_literal(&mut self, lit: &Literal<'a>) -> Result<&'a MonoType<'a>, TypeError> {
        let p = match lit {
            Literal::Unit => Prim::Unit,
            Literal::Bool(_) => Prim::Bool,
            Literal::Char(_) => Prim::Char,
            Literal::Int(_) => Prim::Int,
            Literal::Float(_) => Prim::Float,
            Literal::Double(_) => Prim::Double,
            Literal::String(_) => {
                return Ok(&*self.ctx.alloc(MonoType::Array(
                    &*self.ctx.alloc(MonoType::Prim(Prim::Char)),
                )))
            }
        };
        Ok(&*self.ctx.alloc(MonoType::Prim(p)))
    }

    fn visit_var(&mut self, name: &'a str) -> Result<&'a MonoType<'a>, TypeError> {
        if let Some(ty) = self.env.lookup(name) {
            let max_tgen = ty.max_tgen();
            if let Some(m) = max_tgen {
                let fresh_vars: Vec<_> = (0..=m).map(|_| self.fresh_tvar()).collect();
                let fresh = self.ctx.arena().alloc_slice_clone(&fresh_vars);
                let inst = ty.instantiate(self.ctx, fresh);
                if let MonoType::Constraint(_, _, _) = inst {
                    self.constraints.borrow_mut().push(inst);
                }
                Ok(inst)
            } else {
                Ok(ty)
            }
        } else {
            Err(TypeError::UnboundVariable(name.to_string()))
        }
    }

    fn visit_let(
        &mut self,
        pat: &Pattern<'a>,
        def: &'a Expr<'a>,
        body: &'a Expr<'a>,
    ) -> Result<&'a MonoType<'a>, TypeError> {
        let def_ty = self.visit(def)?;

        let mut env_vars = HashSet::new();
        self.env.free_tvars(&mut env_vars);
        let mut mapping = HashMap::new();
        let generalized_ty = def_ty.generalize(self.ctx, &env_vars, &mut mapping);

        let mut bindings = Vec::new();
        self.visit_pattern(pat, generalized_ty, &mut bindings)?;

        self.with_bindings(bindings, |this| this.visit(body))
    }

    fn visit_fn(
        &mut self,
        pat: &Pattern<'a>,
        body: &'a Expr<'a>,
    ) -> Result<&'a MonoType<'a>, TypeError> {
        let arg_ty = self.fresh_tvar();

        let mut bindings = Vec::new();
        self.visit_pattern(pat, arg_ty, &mut bindings)?;

        let ret_ty = self.with_bindings(bindings, |this| this.visit(body))?;

        Ok(&*self.ctx.alloc(MonoType::Fn(arg_ty, ret_ty)))
    }

    fn visit_app(
        &mut self,
        f: &'a Expr<'a>,
        args: &'a [&'a Expr<'a>],
    ) -> Result<&'a MonoType<'a>, TypeError> {
        let curr_f_ty = self.visit(f)?;
        let (constraints, inner_ty) = Self::peel_constraints(curr_f_ty);

        if let MonoType::Fn(dom, ret) = inner_ty {
            let chased_dom = dom.chase();
            if let MonoType::Tuple(elem_tys) = chased_dom {
                return self.apply_tuple_domain(elem_tys, ret, constraints, args);
            } else if args.len() == 1 {
                if let Some(res) =
                    self.try_apply_curried_to_tuple(inner_ty, &constraints, args[0])?
                {
                    return Ok(res);
                }
                let arg_ty = self.visit(args[0])?;
                let ret_ty = self.fresh_tvar();
                let expected_f_ty = &*self.ctx.alloc(MonoType::Fn(arg_ty, ret_ty));
                self.unify(curr_f_ty, expected_f_ty)?;
                return Ok(ret_ty);
            }
        }

        args.iter().try_fold(curr_f_ty, |curr_ty, arg| {
            let ret_ty = self.fresh_tvar();
            let arg_ty = self.visit(arg)?;
            let expected_f_ty = &*self.ctx.alloc(MonoType::Fn(arg_ty, ret_ty));
            self.unify(curr_ty, expected_f_ty)?;
            Ok(ret_ty)
        })
    }

    fn visit_if(
        &mut self,
        cond: &'a Expr<'a>,
        then_e: &'a Expr<'a>,
        else_e: &'a Expr<'a>,
    ) -> Result<&'a MonoType<'a>, TypeError> {
        let cond_ty = self.visit(cond)?;
        let bool_ty = &*self.ctx.alloc(MonoType::Prim(Prim::Bool));
        self.unify(cond_ty, bool_ty)?;

        let t_ty = self.visit(then_e)?;
        let e_ty = self.visit(else_e)?;
        self.unify(t_ty, e_ty)?;

        Ok(t_ty)
    }

    fn visit_tuple(&mut self, exprs: &'a [&'a Expr<'a>]) -> Result<&'a MonoType<'a>, TypeError> {
        let tys: Vec<_> = exprs
            .iter()
            .map(|e| self.visit(e))
            .collect::<Result<_, _>>()?;
        Ok(&*self
            .ctx
            .alloc(MonoType::Tuple(self.ctx.arena().alloc_slice_clone(&tys))))
    }

    fn visit_record(
        &mut self,
        fields: &'a [(&'a str, &'a Expr<'a>)],
    ) -> Result<&'a MonoType<'a>, TypeError> {
        let f_tys: Vec<_> = fields
            .iter()
            .map(|(name, expr)| Ok((*name, self.visit(expr)?)))
            .collect::<Result<_, _>>()?;
        Ok(&*self.ctx.alloc(MonoType::Record(
            self.ctx.arena().alloc_slice_clone(&f_tys),
            None,
        )))
    }

    fn visit_field_access(
        &mut self,
        _expr: &'a Expr<'a>,
        _field: &'a str,
    ) -> Result<&'a MonoType<'a>, TypeError> {
        panic!("Not yet fully implemented for Phase 7")
    }

    fn visit_variant(
        &mut self,
        tag: &'a str,
        payload: &'a Expr<'a>,
    ) -> Result<&'a MonoType<'a>, TypeError> {
        let payload_ty = self.visit(payload)?;
        Ok(&*self.ctx.alloc(MonoType::Variant(
            self.ctx.arena().alloc_slice_clone(&[(tag, payload_ty)]),
            None,
        )))
    }

    fn visit_case(
        &mut self,
        expr: &'a Expr<'a>,
        branches: &'a [(Pattern<'a>, &'a Expr<'a>)],
    ) -> Result<&'a MonoType<'a>, TypeError> {
        let scrutinee_ty = self.visit(expr)?;
        let ret_ty = self.fresh_tvar();

        branches.iter().try_for_each(|(pat, body)| {
            let mut bindings = Vec::new();
            self.visit_pattern(pat, scrutinee_ty, &mut bindings)?;

            let branch_ty = self.with_bindings(bindings, |this| this.visit(body))?;
            self.unify(ret_ty, branch_ty)
        })?;

        Ok(ret_ty)
    }

    fn visit_array(&mut self, exprs: &'a [&'a Expr<'a>]) -> Result<&'a MonoType<'a>, TypeError> {
        let elem_ty = self.fresh_tvar();
        exprs.iter().try_for_each(|e| {
            let ty = self.visit(e)?;
            self.unify(elem_ty, ty)
        })?;
        Ok(&*self.ctx.alloc(MonoType::Array(elem_ty)))
    }

    fn visit_array_index(
        &mut self,
        arr: &'a Expr<'a>,
        idx: &'a Expr<'a>,
    ) -> Result<&'a MonoType<'a>, TypeError> {
        let arr_ty = self.visit(arr)?;
        let idx_ty = self.visit(idx)?;

        let int_ty = &*self.ctx.alloc(MonoType::Prim(Prim::Int));
        self.unify(idx_ty, int_ty)?;

        let elem_ty = self.fresh_tvar();
        let expected_arr_ty = &*self.ctx.alloc(MonoType::Array(elem_ty));
        self.unify(arr_ty, expected_arr_ty)?;

        Ok(elem_ty)
    }

    fn visit_annotate(
        &mut self,
        expr: &'a Expr<'a>,
        ty: &'a MonoType<'a>,
    ) -> Result<&'a MonoType<'a>, TypeError> {
        let e_ty = self.visit(expr)?;
        self.unify(e_ty, ty)?;
        Ok(ty)
    }
}
