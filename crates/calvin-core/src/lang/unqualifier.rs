use crate::context::TypeContext;
use crate::lang::expr::{Expr, ExprVisitor, Pattern};
use crate::lang::types::{Constraint, MonoType};

pub trait Unqualifier<'a> {
    fn handles(&self, class_name: &str) -> bool;
    fn refine(&self, ctx: &'a TypeContext, constraint: &Constraint<'a>) -> bool;
    fn satisfy(
        &self,
        ctx: &'a TypeContext,
        constraint: &Constraint<'a>,
        args: &[&'a Expr<'a>],
    ) -> Option<&'a Expr<'a>>;
}

use std::collections::HashMap;

pub struct UnqualifierSet<'a> {
    unqualifiers: Vec<Box<dyn Unqualifier<'a> + 'a>>,
    pub class_members: HashMap<&'a str, &'a str>,
}

impl<'a> UnqualifierSet<'a> {
    pub fn new() -> Self {
        Self {
            unqualifiers: Vec::new(),
            class_members: HashMap::new(),
        }
    }

    pub fn register_member(&mut self, member: &'a str, class: &'a str) {
        self.class_members.insert(member, class);
    }

    pub fn add_unqualifier(&mut self, u: Box<dyn Unqualifier<'a> + 'a>) {
        self.unqualifiers.push(u);
    }
}

impl<'a> Default for UnqualifierSet<'a> {
    fn default() -> Self {
        Self::new()
    }
}

pub struct UnqualifierPass<'a, 'map> {
    ctx: &'a TypeContext,
    #[allow(dead_code)]
    map: &'map crate::lang::typemap::TypeMap<'a, &'a Expr<'a>>,
    #[allow(dead_code)]
    set: UnqualifierSet<'a>,
}

impl<'a, 'map> UnqualifierPass<'a, 'map> {
    pub fn new(
        ctx: &'a TypeContext,
        map: &'map crate::lang::typemap::TypeMap<'a, &'a Expr<'a>>,
        set: UnqualifierSet<'a>,
    ) -> Self {
        Self { ctx, map, set }
    }
}

impl<'a, 'map> ExprVisitor<'a, &'a Expr<'a>> for UnqualifierPass<'a, 'map> {
    fn visit_literal(&mut self, lit: &crate::lang::expr::Literal<'a>) -> &'a Expr<'a> {
        self.ctx.arena().alloc(Expr::Literal(lit.clone()))
    }

    fn visit_var(&mut self, name: &'a str) -> &'a Expr<'a> {
        self.ctx.arena().alloc(Expr::Var(name))
    }

    fn visit_let(
        &mut self,
        pat: &Pattern<'a>,
        def: &'a Expr<'a>,
        body: &'a Expr<'a>,
    ) -> &'a Expr<'a> {
        let new_def = self.visit(def);
        let new_body = self.visit(body);
        self.ctx
            .arena()
            .alloc(Expr::Let(pat.clone(), new_def, new_body))
    }

    fn visit_fn(&mut self, pat: &Pattern<'a>, body: &'a Expr<'a>) -> &'a Expr<'a> {
        let new_body = self.visit(body);
        self.ctx.arena().alloc(Expr::Fn(pat.clone(), new_body))
    }

    fn visit_app(&mut self, f: &'a Expr<'a>, args: &'a [&'a Expr<'a>]) -> &'a Expr<'a> {
        let new_f = self.visit(f);
        let mut new_args = Vec::new();

        // Dictionary passing removed: Hobbes does not use dictionary passing.
        // It relies on monomorphization and inline structural resolution.

        for arg in args {
            new_args.push(self.visit(arg));
        }
        let new_args_slice = self.ctx.arena().alloc_slice_copy(&new_args);
        self.ctx.arena().alloc(Expr::App(new_f, new_args_slice))
    }

    fn visit_if(
        &mut self,
        cond: &'a Expr<'a>,
        then_e: &'a Expr<'a>,
        else_e: &'a Expr<'a>,
    ) -> &'a Expr<'a> {
        let c = self.visit(cond);
        let t = self.visit(then_e);
        let e = self.visit(else_e);
        self.ctx.arena().alloc(Expr::If(c, t, e))
    }

    fn visit_tuple(&mut self, fields: &'a [&'a Expr<'a>]) -> &'a Expr<'a> {
        let mut new_fields = Vec::with_capacity(fields.len());
        for f in fields {
            new_fields.push(self.visit(f));
        }
        let nf_slice = self.ctx.arena().alloc_slice_copy(&new_fields);
        self.ctx.arena().alloc(Expr::Tuple(nf_slice))
    }

    fn visit_record(&mut self, fields: &'a [(&'a str, &'a Expr<'a>)]) -> &'a Expr<'a> {
        let mut new_fields = Vec::with_capacity(fields.len());
        for (k, v) in fields {
            new_fields.push((*k, self.visit(v)));
        }
        let nf_slice = self.ctx.arena().alloc_slice_copy(&new_fields);
        self.ctx.arena().alloc(Expr::Record(nf_slice))
    }

    fn visit_field_access(&mut self, expr: &'a Expr<'a>, field: &'a str) -> &'a Expr<'a> {
        let new_expr = self.visit(expr);
        self.ctx.arena().alloc(Expr::FieldAccess(new_expr, field))
    }

    fn visit_variant(&mut self, tag: &'a str, payload: &'a Expr<'a>) -> &'a Expr<'a> {
        let new_payload = self.visit(payload);
        self.ctx.arena().alloc(Expr::Variant(tag, new_payload))
    }

    fn visit_case(
        &mut self,
        expr: &'a Expr<'a>,
        branches: &'a [(Pattern<'a>, &'a Expr<'a>)],
    ) -> &'a Expr<'a> {
        let new_expr = self.visit(expr);
        let mut new_branches = Vec::with_capacity(branches.len());
        for (p, b) in branches {
            new_branches.push((p.clone(), self.visit(b)));
        }
        let nb_slice = self.ctx.arena().alloc_slice_clone(&new_branches);
        self.ctx.arena().alloc(Expr::Case(new_expr, nb_slice))
    }

    fn visit_array_index(&mut self, arr: &'a Expr<'a>, idx: &'a Expr<'a>) -> &'a Expr<'a> {
        let new_arr = self.visit(arr);
        let new_idx = self.visit(idx);
        self.ctx.arena().alloc(Expr::ArrayIndex(new_arr, new_idx))
    }

    fn visit_annotate(&mut self, expr: &'a Expr<'a>, ty: &'a MonoType<'a>) -> &'a Expr<'a> {
        let new_expr = self.visit(expr);
        self.ctx.arena().alloc(Expr::Annotate(new_expr, ty))
    }

    fn visit_array(&mut self, exprs: &'a [&'a Expr<'a>]) -> &'a Expr<'a> {
        let mut new_exprs = Vec::with_capacity(exprs.len());
        for e in exprs {
            new_exprs.push(self.visit(e));
        }
        let nf_slice = self.ctx.arena().alloc_slice_copy(&new_exprs);
        self.ctx.arena().alloc(Expr::Array(nf_slice))
    }
}

/// Epic 28.2: SubtypeUnqualifier
/// Handles implicit coercions and structural subtyping constraints (e.g., 'Convert a b').
pub struct SubtypeUnqualifier;

impl<'a> Unqualifier<'a> for SubtypeUnqualifier {
    fn handles(&self, class_name: &str) -> bool {
        class_name == "Convert" || class_name == "Subtype"
    }

    fn refine(&self, _ctx: &'a TypeContext, constraint: &Constraint<'a>) -> bool {
        if constraint.arguments.len() != 2 {
            return false;
        }
        let src = constraint.arguments[0].chase();
        let dst = constraint.arguments[1].chase();

        // Very basic structural subtyping:
        // 1. Exact match
        if src == dst {
            return true;
        }

        // 2. Primitive widening (Int -> Float)
        if matches!(src, MonoType::Prim(crate::lang::types::Prim::Int))
            && matches!(dst, MonoType::Prim(crate::lang::types::Prim::Float))
        {
            return true;
        }

        // Future: array of subtypes, records of subtypes, etc.
        false
    }

    fn satisfy(
        &self,
        ctx: &'a TypeContext,
        constraint: &Constraint<'a>,
        _args: &[&'a Expr<'a>],
    ) -> Option<&'a Expr<'a>> {
        if !self.refine(ctx, constraint) {
            return None;
        }

        let src = constraint.arguments[0].chase();
        let dst = constraint.arguments[1].chase();

        if src == dst {
            // Identity conversion dictionary
            Some(ctx.arena().alloc(Expr::Var("dict_Convert_Id")))
        } else if matches!(src, MonoType::Prim(crate::lang::types::Prim::Int))
            && matches!(dst, MonoType::Prim(crate::lang::types::Prim::Float))
        {
            // Int -> Float conversion dictionary
            Some(ctx.arena().alloc(Expr::Var("dict_Convert_Int_Float")))
        } else {
            None
        }
    }
}

/// Epic 28.4: HasFieldUnqualifier
/// Solves constraints generated by record field access (e.g., 'HasField "x" r t').
pub struct HasFieldUnqualifier;

impl<'a> Unqualifier<'a> for HasFieldUnqualifier {
    fn handles(&self, class_name: &str) -> bool {
        class_name == "HasField"
    }

    fn refine(&self, _ctx: &'a TypeContext, constraint: &Constraint<'a>) -> bool {
        if constraint.arguments.len() != 3 {
            return false;
        }

        let _field_name_ty = constraint.arguments[0].chase();
        let record_ty = constraint.arguments[1].chase();
        let _field_ty = constraint.arguments[2].chase();

        // We expect the first argument to be a phantom type or literal string type representing the field name.
        // For simplicity in this mock, we assume it's represented as a Record or just use a placeholder rule.
        // Actually, let's just check if the second argument is a Record and has some field.
        if let MonoType::Record(fields, _tail) = record_ty {
            // In a real implementation, we'd extract the string from `field_name_ty`
            // For now, we just validate it's a record.
            return !fields.is_empty();
        }

        false
    }

    fn satisfy(
        &self,
        ctx: &'a TypeContext,
        constraint: &Constraint<'a>,
        _args: &[&'a Expr<'a>],
    ) -> Option<&'a Expr<'a>> {
        if !self.refine(ctx, constraint) {
            return None;
        }
        // Return a mock dictionary for field extraction
        Some(ctx.arena().alloc(Expr::Var("dict_HasField")))
    }
}

/// Epic 28.4: ConsRecordUnqualifier
/// Solves constraints for record extension (e.g., 'ConsRecord "x" t r1 r2').
pub struct ConsRecordUnqualifier;

impl<'a> Unqualifier<'a> for ConsRecordUnqualifier {
    fn handles(&self, class_name: &str) -> bool {
        class_name == "ConsRecord"
    }

    fn refine(&self, _ctx: &'a TypeContext, constraint: &Constraint<'a>) -> bool {
        if constraint.arguments.len() != 4 {
            return false;
        }

        let r1 = constraint.arguments[2].chase();
        let r2 = constraint.arguments[3].chase();

        // Validate both are records (or one is an open row that can be unified)
        matches!(r1, MonoType::Record(_, _)) && matches!(r2, MonoType::Record(_, _))
    }

    fn satisfy(
        &self,
        ctx: &'a TypeContext,
        constraint: &Constraint<'a>,
        _args: &[&'a Expr<'a>],
    ) -> Option<&'a Expr<'a>> {
        if !self.refine(ctx, constraint) {
            return None;
        }
        Some(ctx.arena().alloc(Expr::Var("dict_ConsRecord")))
    }
}

/// Epic 28.4: HasCtorUnqualifier
/// Solves constraints generated by variant injection (e.g., 'HasCtor "Foo" v t').
pub struct HasCtorUnqualifier;

impl<'a> Unqualifier<'a> for HasCtorUnqualifier {
    fn handles(&self, class_name: &str) -> bool {
        class_name == "HasCtor"
    }

    fn refine(&self, _ctx: &'a TypeContext, constraint: &Constraint<'a>) -> bool {
        if constraint.arguments.len() != 3 {
            return false;
        }

        let variant_ty = constraint.arguments[1].chase();

        if let MonoType::Variant(cases, _tail) = variant_ty {
            return !cases.is_empty();
        }

        false
    }

    fn satisfy(
        &self,
        ctx: &'a TypeContext,
        constraint: &Constraint<'a>,
        _args: &[&'a Expr<'a>],
    ) -> Option<&'a Expr<'a>> {
        if !self.refine(ctx, constraint) {
            return None;
        }
        Some(ctx.arena().alloc(Expr::Var("dict_HasCtor")))
    }
}

/// Epic 28.4: ConsVariantUnqualifier
/// Solves constraints for variant extension (e.g., 'ConsVariant "Foo" t v1 v2').
pub struct ConsVariantUnqualifier;

impl<'a> Unqualifier<'a> for ConsVariantUnqualifier {
    fn handles(&self, class_name: &str) -> bool {
        class_name == "ConsVariant"
    }

    fn refine(&self, _ctx: &'a TypeContext, constraint: &Constraint<'a>) -> bool {
        if constraint.arguments.len() != 4 {
            return false;
        }

        let v1 = constraint.arguments[2].chase();
        let v2 = constraint.arguments[3].chase();

        matches!(v1, MonoType::Variant(_, _)) && matches!(v2, MonoType::Variant(_, _))
    }

    fn satisfy(
        &self,
        ctx: &'a TypeContext,
        constraint: &Constraint<'a>,
        _args: &[&'a Expr<'a>],
    ) -> Option<&'a Expr<'a>> {
        if !self.refine(ctx, constraint) {
            return None;
        }
        Some(ctx.arena().alloc(Expr::Var("dict_ConsVariant")))
    }
}

/// Epic 28.5: AppendsToUnqualifier
/// Solves constraints for array concatenation (e.g., 'AppendsTo a b c' where a ++ b = c).
pub struct AppendsToUnqualifier;

impl<'a> Unqualifier<'a> for AppendsToUnqualifier {
    fn handles(&self, class_name: &str) -> bool {
        class_name == "AppendsTo"
    }

    fn refine(&self, _ctx: &'a TypeContext, constraint: &Constraint<'a>) -> bool {
        if constraint.arguments.len() != 3 {
            return false;
        }

        let a = constraint.arguments[0].chase();
        let b = constraint.arguments[1].chase();
        let c = constraint.arguments[2].chase();

        // In a real implementation, we'd unify the inner types.
        // For simplicity, we just verify they are all Arrays.
        matches!(a, MonoType::Array(_))
            && matches!(b, MonoType::Array(_))
            && matches!(c, MonoType::Array(_))
    }

    fn satisfy(
        &self,
        ctx: &'a TypeContext,
        constraint: &Constraint<'a>,
        _args: &[&'a Expr<'a>],
    ) -> Option<&'a Expr<'a>> {
        if !self.refine(ctx, constraint) {
            return None;
        }
        Some(ctx.arena().alloc(Expr::Var("dict_AppendsTo")))
    }
}

/// Epic 28.5: EqualUnqualifier
/// Type equivalence proofs (e.g., 'Equal a b').
pub struct EqualUnqualifier;

impl<'a> Unqualifier<'a> for EqualUnqualifier {
    fn handles(&self, class_name: &str) -> bool {
        class_name == "Equal"
    }

    fn refine(&self, _ctx: &'a TypeContext, constraint: &Constraint<'a>) -> bool {
        if constraint.arguments.len() != 2 {
            return false;
        }
        let a = constraint.arguments[0].chase();
        let b = constraint.arguments[1].chase();
        a == b
    }

    fn satisfy(
        &self,
        ctx: &'a TypeContext,
        constraint: &Constraint<'a>,
        _args: &[&'a Expr<'a>],
    ) -> Option<&'a Expr<'a>> {
        if !self.refine(ctx, constraint) {
            return None;
        }
        Some(ctx.arena().alloc(Expr::Var("dict_Equal")))
    }
}

/// Epic 28.5: NotUnqualifier
/// Negative constraints (e.g., 'Not c' where c is unsatisfiable).
pub struct NotUnqualifier;

impl<'a> Unqualifier<'a> for NotUnqualifier {
    fn handles(&self, class_name: &str) -> bool {
        class_name == "Not"
    }

    fn refine(&self, _ctx: &'a TypeContext, constraint: &Constraint<'a>) -> bool {
        // Typically checks if the underlying constraint is permanently rejected.
        constraint.arguments.len() == 1
    }

    fn satisfy(
        &self,
        ctx: &'a TypeContext,
        constraint: &Constraint<'a>,
        _args: &[&'a Expr<'a>],
    ) -> Option<&'a Expr<'a>> {
        if !self.refine(ctx, constraint) {
            return None;
        }
        Some(ctx.arena().alloc(Expr::Var("dict_Not")))
    }
}

/// Epic 28.5: SizeOfUnqualifier
/// Resolves the memory layout size of a type at compile-time.
pub struct SizeOfUnqualifier;

impl<'a> Unqualifier<'a> for SizeOfUnqualifier {
    fn handles(&self, class_name: &str) -> bool {
        class_name == "SizeOf"
    }

    fn refine(&self, _ctx: &'a TypeContext, constraint: &Constraint<'a>) -> bool {
        constraint.arguments.len() == 2
    }

    fn satisfy(
        &self,
        ctx: &'a TypeContext,
        constraint: &Constraint<'a>,
        _args: &[&'a Expr<'a>],
    ) -> Option<&'a Expr<'a>> {
        if !self.refine(ctx, constraint) {
            return None;
        }
        let ty = constraint.arguments[0].chase();
        // Calculate size: Int -> 8, Byte -> 1, etc.
        let size = match ty {
            MonoType::Prim(crate::lang::types::Prim::Int) => 8,
            MonoType::Prim(crate::lang::types::Prim::Byte) => 1,
            MonoType::Prim(crate::lang::types::Prim::Double) => 8,
            _ => 0, // Mock fallback
        };
        // The dictionary essentially returns the literal size.
        Some(
            ctx.arena()
                .alloc(Expr::Literal(crate::lang::expr::Literal::Int(size))),
        )
    }
}
