use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use crate::context::TypeContext;
use crate::lang::expr::{Expr, ExprVisitor, Literal, Pattern};
use crate::lang::types::{MonoType, Prim};

#[derive(Debug)]
pub enum TypeError {
    TypeMismatch,
    OccursCheckFailed,
    UnboundVariable(String),
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
        for ty in self.bindings.values() {
            ty.free_tvars(vars);
        }
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

impl<'a> TypeInference<'a> {
    pub fn solve_constraints(&self) -> Result<(), TypeError> {
        let constraints = self.constraints.borrow().clone();
        for c in constraints {
            if let MonoType::Constraint(name, args, _) = c.chase() {
                // 1. First consult registry functional dependencies
                let unifications = self.classes.refine_fundeps(name, args);
                for (target, resolved) in unifications {
                    self.unify(target, resolved)?;
                }

                // 2. Fallback built-in arithmetic solver across primitives
                if *name == "Add" || *name == "Subtract" || *name == "Multiply" || *name == "Divide" {
                    if args.len() == 3 {
                        let a = args[0].chase();
                        let b = args[1].chase();
                        let c_arg = args[2].chase();

                        match (a, b) {
                            (MonoType::Prim(Prim::Int), MonoType::Prim(Prim::Int)) => {
                                self.unify(c_arg, &*self.ctx.alloc(MonoType::Prim(Prim::Int)))?;
                            }
                            (MonoType::Prim(Prim::Float), MonoType::Prim(Prim::Float)) => {
                                self.unify(c_arg, &*self.ctx.alloc(MonoType::Prim(Prim::Float)))?;
                            }
                            (MonoType::Prim(Prim::Double), MonoType::Prim(Prim::Double)) => {
                                self.unify(c_arg, &*self.ctx.alloc(MonoType::Prim(Prim::Double)))?;
                            }
                            (MonoType::Prim(Prim::Long), MonoType::Prim(Prim::Long)) => {
                                self.unify(c_arg, &*self.ctx.alloc(MonoType::Prim(Prim::Long)))?;
                            }
                            (MonoType::Prim(Prim::Short), MonoType::Prim(Prim::Short)) => {
                                self.unify(c_arg, &*self.ctx.alloc(MonoType::Prim(Prim::Short)))?;
                            }
                            (MonoType::Prim(Prim::Byte), MonoType::Prim(Prim::Byte)) => {
                                self.unify(c_arg, &*self.ctx.alloc(MonoType::Prim(Prim::Byte)))?;
                            }
                            (MonoType::Prim(Prim::Char), MonoType::Prim(Prim::Char)) => {
                                self.unify(c_arg, &*self.ctx.alloc(MonoType::Prim(Prim::Char)))?;
                            }
                            _ => {}
                        }
                    }
                }
            }
        }
        Ok(())
    }

    pub fn residual_constraints(&self) -> Vec<(&'a str, Vec<&'a MonoType<'a>>)> {
        let mut residuals = Vec::new();
        let constraints = self.constraints.borrow().clone();
        for c in constraints {
            if let MonoType::Constraint(name, args, _) = c.chase() {
                // If satisfied by the typeclass registry instance database, discharge constraint
                if self.classes.is_satisfied(name, args) {
                    continue;
                }

                // Or if satisfied by standard arithmetic ground primitives
                if args.len() == 3 {
                    let a = args[0].chase();
                    let b = args[1].chase();
                    let c_arg = args[2].chase();
                    match (a, b, c_arg) {
                        (MonoType::Prim(Prim::Int), MonoType::Prim(Prim::Int), MonoType::Prim(Prim::Int))
                        | (MonoType::Prim(Prim::Float), MonoType::Prim(Prim::Float), MonoType::Prim(Prim::Float))
                        | (MonoType::Prim(Prim::Double), MonoType::Prim(Prim::Double), MonoType::Prim(Prim::Double))
                        | (MonoType::Prim(Prim::Long), MonoType::Prim(Prim::Long), MonoType::Prim(Prim::Long))
                        | (MonoType::Prim(Prim::Short), MonoType::Prim(Prim::Short), MonoType::Prim(Prim::Short))
                        | (MonoType::Prim(Prim::Byte), MonoType::Prim(Prim::Byte), MonoType::Prim(Prim::Byte))
                        | (MonoType::Prim(Prim::Char), MonoType::Prim(Prim::Char), MonoType::Prim(Prim::Char)) => {
                            continue;
                        }
                        _ => {}
                    }
                }
                let chased_args: Vec<&'a MonoType<'a>> = args.iter().map(|arg| arg.chase()).collect();
                residuals.push((*name, chased_args));
            }
        }
        residuals.sort_by(crate::lang::types::compare_constraint);
        let mut deduped: Vec<(&'a str, Vec<&'a MonoType<'a>>)> = Vec::new();
        for r in residuals {
            if !deduped.iter().any(|(n, a)| *n == r.0 && a == &r.1) {
                deduped.push(r);
            }
        }
        deduped
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
        let mut constraints = Vec::new();
        let mut curr = ty.chase();
        while let MonoType::Constraint(name, args, inner) = curr {
            constraints.push((*name, *args));
            curr = inner.chase();
        }
        (constraints, curr)
    }

    pub fn wrap_constraints(
        &self,
        constraints: Vec<(&'a str, &'a [&'a MonoType<'a>])>,
        mut ty: &'a MonoType<'a>,
    ) -> &'a MonoType<'a> {
        for (name, args) in constraints.into_iter().rev() {
            ty = self.ctx.alloc(MonoType::Constraint(name, args, ty));
        }
        ty
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
                    || tail.map_or(false, |t| Self::occurs(tvar_id, t))
            }
            MonoType::Variant(cases, tail) => {
                cases.iter().any(|(_, t)| Self::occurs(tvar_id, t))
                    || tail.map_or(false, |t| Self::occurs(tvar_id, t))
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
        if let MonoType::Constraint(_, _, inner) = t1 { t1 = inner.chase(); }
        if let MonoType::Constraint(_, _, inner) = t2 { t2 = inner.chase(); }

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
                for (a, b) in ts1.iter().zip(ts2.iter()) {
                    self.unify(a, b)?;
                }
                Ok(())
            }
            (MonoType::Record(fs1, tail1), MonoType::Record(fs2, tail2)) => {
                let mut map1 = std::collections::HashMap::new();
                for (n, t) in *fs1 {
                    map1.insert(*n, *t);
                }
                let mut map2 = std::collections::HashMap::new();
                for (n, t) in *fs2 {
                    map2.insert(*n, *t);
                }

                let mut diff1 = Vec::new();
                let mut diff2 = Vec::new();

                for (n, t1_f) in *fs1 {
                    if let Some(t2_f) = map2.remove(n) {
                        self.unify(t1_f, t2_f)?;
                    } else {
                        diff1.push((*n, *t1_f));
                    }
                }

                for (n, t2_f) in *fs2 {
                    if !map1.contains_key(n) {
                        diff2.push((*n, *t2_f));
                    }
                }

                if diff1.is_empty() && diff2.is_empty() {
                    if let (Some(r1), Some(r2)) = (tail1, tail2) {
                        self.unify(*r1, *r2)?;
                    }
                    return Ok(());
                }

                let new_tail = self.fresh_tvar();

                if !diff1.is_empty() {
                    if let Some(r2) = tail2 {
                        let diff1_slice = self.ctx.arena().alloc_slice_copy(&diff1);
                        let ext2 = self
                            .ctx
                            .alloc(MonoType::Record(diff1_slice, Some(new_tail)));
                        self.unify(*r2, ext2)?;
                    } else {
                        return Err(TypeError::TypeMismatch);
                    }
                }

                if !diff2.is_empty() {
                    if let Some(r1) = tail1 {
                        let diff2_slice = self.ctx.arena().alloc_slice_copy(&diff2);
                        let ext1 = self
                            .ctx
                            .alloc(MonoType::Record(diff2_slice, Some(new_tail)));
                        self.unify(*r1, ext1)?;
                    } else {
                        return Err(TypeError::TypeMismatch);
                    }
                }

                Ok(())
            }
            (MonoType::Variant(fs1, tail1), MonoType::Variant(fs2, tail2)) => {
                let mut map1 = std::collections::HashMap::new();
                for (n, t) in *fs1 {
                    map1.insert(*n, *t);
                }
                let mut map2 = std::collections::HashMap::new();
                for (n, t) in *fs2 {
                    map2.insert(*n, *t);
                }

                let mut diff1 = Vec::new();
                let mut diff2 = Vec::new();

                for (n, t1_f) in *fs1 {
                    if let Some(t2_f) = map2.remove(n) {
                        self.unify(t1_f, t2_f)?;
                    } else {
                        diff1.push((*n, *t1_f));
                    }
                }

                for (n, t2_f) in *fs2 {
                    if !map1.contains_key(n) {
                        diff2.push((*n, *t2_f));
                    }
                }

                if diff1.is_empty() && diff2.is_empty() {
                    if let (Some(r1), Some(r2)) = (tail1, tail2) {
                        self.unify(*r1, *r2)?;
                    }
                    return Ok(());
                }

                let new_tail = self.fresh_tvar();

                if !diff1.is_empty() {
                    if let Some(r2) = tail2 {
                        let diff1_slice = self.ctx.arena().alloc_slice_copy(&diff1);
                        let ext2 = self
                            .ctx
                            .alloc(MonoType::Variant(diff1_slice, Some(new_tail)));
                        self.unify(*r2, ext2)?;
                    } else {
                        return Err(TypeError::TypeMismatch);
                    }
                }

                if !diff2.is_empty() {
                    if let Some(r1) = tail1 {
                        let diff2_slice = self.ctx.arena().alloc_slice_copy(&diff2);
                        let ext1 = self
                            .ctx
                            .alloc(MonoType::Variant(diff2_slice, Some(new_tail)));
                        self.unify(*r1, ext1)?;
                    } else {
                        return Err(TypeError::TypeMismatch);
                    }
                }

                Ok(())
            }
            _ => Err(TypeError::TypeMismatch),
        }
    }

    pub fn fresh_tvar(&self) -> &'a MonoType<'a> {
        let id = self.ctx.fresh_tvar_id();
        &*self.ctx.alloc(MonoType::TVar(id, std::cell::Cell::new(None)))
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
                let mut elem_tys = Vec::new();
                for _ in 0..pats.len() {
                    elem_tys.push(self.fresh_tvar());
                }
                let tup_ty = &*self.ctx.alloc(MonoType::Tuple(
                    self.ctx.arena().alloc_slice_clone(&elem_tys),
                ));
                self.unify(expected_ty, tup_ty)?;
                for (i, p) in pats.iter().enumerate() {
                    self.visit_pattern(p, elem_tys[i], bindings)?;
                }
                Ok(())
            }
            Pattern::Record(fields) => {
                let mut field_tys = Vec::new();
                for (name, p) in *fields {
                    let f_ty = self.fresh_tvar();
                    field_tys.push((*name, f_ty));
                    self.visit_pattern(p, f_ty, bindings)?;
                }
                let rec_ty = &*self.ctx.alloc(MonoType::Record(
                    self.ctx.arena().alloc_slice_clone(&field_tys),
                    None,
                ));
                self.unify(expected_ty, rec_ty)
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
                let mut fresh_vars = Vec::new();
                for _ in 0..=m {
                    fresh_vars.push(self.fresh_tvar());
                }
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

        let prev_env = self.env.clone();
        let mut new_env = prev_env.clone();
        for (name, ty) in bindings {
            new_env = Rc::new(TypeEnv::extend(new_env, name, ty));
        }
        self.env = new_env;

        let body_ty = self.visit(body)?;
        self.env = prev_env; // restore
        Ok(body_ty)
    }

    fn visit_fn(
        &mut self,
        pat: &Pattern<'a>,
        body: &'a Expr<'a>,
    ) -> Result<&'a MonoType<'a>, TypeError> {
        let arg_ty = self.fresh_tvar();

        let mut bindings = Vec::new();
        self.visit_pattern(pat, arg_ty, &mut bindings)?;

        let prev_env = self.env.clone();
        let mut new_env = prev_env.clone();
        for (name, ty) in bindings {
            new_env = Rc::new(TypeEnv::extend(new_env, name, ty));
        }
        self.env = new_env;

        let ret_ty = self.visit(body)?;
        self.env = prev_env;

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
                // If single argument passed:
                if args.len() == 1 {
                    let arg_ty = self.visit(args[0])?;
                    // If the single argument is a tuple matching domain length, unify directly
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
                    for (arg, param_ty) in args.iter().zip(elem_tys.iter()) {
                        let arg_ty = self.visit(*arg)?;
                        self.unify(arg_ty, param_ty)?;
                    }
                    return Ok(self.wrap_constraints(constraints, ret));
                }

                // If multiple arguments passed, but fewer than tuple domain length (partial application):
                if args.len() > 1 && args.len() < elem_tys.len() {
                    for (arg, param_ty) in args.iter().zip(elem_tys.iter()) {
                        let arg_ty = self.visit(*arg)?;
                        self.unify(arg_ty, param_ty)?;
                    }
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
                if args.len() > elem_tys.len() {
                    for (arg, param_ty) in args[..elem_tys.len()].iter().zip(elem_tys.iter()) {
                        let arg_ty = self.visit(*arg)?;
                        self.unify(arg_ty, param_ty)?;
                    }
                    let mut curr_ty = self.wrap_constraints(constraints, ret);
                    for arg in &args[elem_tys.len()..] {
                        let ret_ty = self.fresh_tvar();
                        let arg_ty = self.visit(*arg)?;
                        let expected_f_ty = &*self.ctx.alloc(MonoType::Fn(arg_ty, ret_ty));
                        self.unify(curr_ty, expected_f_ty)?;
                        curr_ty = ret_ty;
                    }
                    return Ok(curr_ty);
                }
            } else if args.len() == 1 {
                // Curried function applied to a tuple argument: e.g. (\x -> \y -> x + y) (1.0, 2.0)
                let arg_ty = self.visit(args[0])?;
                if let MonoType::Tuple(tup_elems) = arg_ty.chase() {
                    if tup_elems.len() > 1 {
                        let mut curried_params: Vec<&'a MonoType<'a>> = Vec::new();
                        let mut cur = inner_ty;
                        while let MonoType::Fn(param, next) = cur {
                            curried_params.push(*param);
                            cur = next.chase();
                            if curried_params.len() == tup_elems.len() {
                                break;
                            }
                        }
                        if curried_params.len() == tup_elems.len() {
                            for (elem, param) in tup_elems.iter().zip(curried_params.into_iter()) {
                                self.unify(elem, param)?;
                            }
                            return Ok(self.wrap_constraints(constraints, cur));
                        }
                    }
                }
                let ret_ty = self.fresh_tvar();
                let expected_f_ty = &*self.ctx.alloc(MonoType::Fn(arg_ty, ret_ty));
                self.unify(curr_f_ty, expected_f_ty)?;
                return Ok(ret_ty);
            }
        }

        let mut curr_ty = curr_f_ty;
        for arg in args {
            let ret_ty = self.fresh_tvar();
            let arg_ty = self.visit(*arg)?;
            let expected_f_ty = &*self.ctx.alloc(MonoType::Fn(arg_ty, ret_ty));
            self.unify(curr_ty, expected_f_ty)?;
            curr_ty = ret_ty;
        }
        Ok(curr_ty)
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
        let mut tys = Vec::new();
        for e in exprs {
            tys.push(self.visit(e)?);
        }
        Ok(&*self
            .ctx
            .alloc(MonoType::Tuple(self.ctx.arena().alloc_slice_clone(&tys))))
    }

    fn visit_record(
        &mut self,
        fields: &'a [(&'a str, &'a Expr<'a>)],
    ) -> Result<&'a MonoType<'a>, TypeError> {
        let mut f_tys = Vec::new();
        for (name, expr) in fields {
            f_tys.push((*name, self.visit(expr)?));
        }
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

        for (pat, body) in branches {
            let mut bindings = Vec::new();
            self.visit_pattern(pat, scrutinee_ty, &mut bindings)?;

            let prev_env = self.env.clone();
            let mut new_env = prev_env.clone();
            for (name, ty) in bindings {
                new_env = Rc::new(TypeEnv::extend(new_env, name, ty));
            }
            self.env = new_env;

            let branch_ty = self.visit(body)?;
            self.unify(ret_ty, branch_ty)?;

            self.env = prev_env;
        }

        Ok(ret_ty)
    }

    fn visit_array(&mut self, exprs: &'a [&'a Expr<'a>]) -> Result<&'a MonoType<'a>, TypeError> {
        let elem_ty = self.fresh_tvar();
        for e in exprs {
            let ty = self.visit(e)?;
            self.unify(elem_ty, ty)?;
        }
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
