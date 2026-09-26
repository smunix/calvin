use super::types::MonoType;

/// A literal value in Calvin.
#[derive(Debug, Clone, PartialEq)]
pub enum Literal<'a> {
    Unit,
    Bool(bool),
    Char(char),
    Int(i64),
    Float(f64),
    Double(f64),
    String(&'a str),
}

/// A pattern for let-bindings and case branches.
#[derive(Debug, Clone, PartialEq)]
pub enum Pattern<'a> {
    /// A wildcard pattern `_`.
    Any,
    /// A variable binding pattern `x`.
    Var(&'a str),
    /// A literal match pattern.
    Literal(Literal<'a>),
    /// A tuple pattern `(p1, p2, ...)`.
    Tuple(&'a [Pattern<'a>]),
    /// A record pattern `{ f1: p1, f2: p2, ... }`.
    Record(&'a [(&'a str, Pattern<'a>)]),
    /// A variant constructor pattern `|Foo x|`.
    Variant(&'a str, &'a Pattern<'a>),
}

/// An expression in the Calvin language.
/// Expressions are allocated in a bumpalo arena.
#[derive(Debug, PartialEq)]
pub enum Expr<'a> {
    /// A literal value.
    Literal(Literal<'a>),
    /// A variable reference.
    Var(&'a str),
    /// A let binding: `let p = e1 in e2`.
    Let(Pattern<'a>, &'a Expr<'a>, &'a Expr<'a>),
    /// A lambda function: `\p -> e`.
    Fn(Pattern<'a>, &'a Expr<'a>),
    /// A function application: `f(x, y, ...)`.
    App(&'a Expr<'a>, &'a [&'a Expr<'a>]),
    /// An if-then-else expression.
    If(&'a Expr<'a>, &'a Expr<'a>, &'a Expr<'a>),
    /// A tuple constructor: `(e1, e2, ...)`.
    Tuple(&'a [&'a Expr<'a>]),
    /// A record constructor: `{ f1: e1, f2: e2, ... }`.
    Record(&'a [(&'a str, &'a Expr<'a>)]),
    /// A record field access: `e.f`.
    FieldAccess(&'a Expr<'a>, &'a str),
    /// A variant constructor: `|Foo=e|`.
    Variant(&'a str, &'a Expr<'a>),
    /// A case expression (pattern matching).
    Case(&'a Expr<'a>, &'a [(Pattern<'a>, &'a Expr<'a>)]),
    /// An array constructor: `[e1, e2, ...]`.
    Array(&'a [&'a Expr<'a>]),
    /// An array index access: `arr[idx]`.
    ArrayIndex(&'a Expr<'a>, &'a Expr<'a>),
    /// A type annotation: `e :: T`.
    Annotate(&'a Expr<'a>, &'a MonoType<'a>),
}

/// A visitor trait for traversing the expression AST.
pub trait ExprVisitor<'a, R> {
    fn visit_literal(&mut self, lit: &Literal<'a>) -> R;
    fn visit_var(&mut self, name: &'a str) -> R;
    fn visit_let(&mut self, pat: &Pattern<'a>, def: &'a Expr<'a>, body: &'a Expr<'a>) -> R;
    fn visit_fn(&mut self, pat: &Pattern<'a>, body: &'a Expr<'a>) -> R;
    fn visit_app(&mut self, f: &'a Expr<'a>, args: &'a [&'a Expr<'a>]) -> R;
    fn visit_if(&mut self, cond: &'a Expr<'a>, then_e: &'a Expr<'a>, else_e: &'a Expr<'a>) -> R;
    fn visit_tuple(&mut self, exprs: &'a [&'a Expr<'a>]) -> R;
    fn visit_record(&mut self, fields: &'a [(&'a str, &'a Expr<'a>)]) -> R;
    fn visit_field_access(&mut self, expr: &'a Expr<'a>, field: &'a str) -> R;
    fn visit_variant(&mut self, tag: &'a str, payload: &'a Expr<'a>) -> R;
    fn visit_case(&mut self, expr: &'a Expr<'a>, branches: &'a [(Pattern<'a>, &'a Expr<'a>)]) -> R;
    fn visit_array(&mut self, exprs: &'a [&'a Expr<'a>]) -> R;
    fn visit_array_index(&mut self, arr: &'a Expr<'a>, idx: &'a Expr<'a>) -> R;
    fn visit_annotate(&mut self, expr: &'a Expr<'a>, ty: &'a MonoType<'a>) -> R;

    /// Default dispatch method.
    fn visit(&mut self, expr: &'a Expr<'a>) -> R {
        match expr {
            Expr::Literal(lit) => self.visit_literal(lit),
            Expr::Var(name) => self.visit_var(name),
            Expr::Let(pat, def, body) => self.visit_let(pat, def, body),
            Expr::Fn(pat, body) => self.visit_fn(pat, body),
            Expr::App(f, args) => self.visit_app(f, args),
            Expr::If(cond, then_e, else_e) => self.visit_if(cond, then_e, else_e),
            Expr::Tuple(exprs) => self.visit_tuple(exprs),
            Expr::Record(fields) => self.visit_record(fields),
            Expr::FieldAccess(e, f) => self.visit_field_access(e, f),
            Expr::Variant(tag, payload) => self.visit_variant(tag, payload),
            Expr::Case(e, branches) => self.visit_case(e, branches),
            Expr::Array(exprs) => self.visit_array(exprs),
            Expr::ArrayIndex(arr, idx) => self.visit_array_index(arr, idx),
            Expr::Annotate(e, ty) => self.visit_annotate(e, ty),
        }
    }
}
