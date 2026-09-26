use calvin_core::lang::expr::Expr;
use calvin_core::lang::types::Prim;

#[derive(Debug, Clone)]
pub struct Module<'a> {
    pub name: Option<&'a str>,
    pub defs: Vec<ModuleDef<'a>>,
}

#[derive(Debug, Clone)]
pub enum ModuleDef<'a> {
    Class(ClassDef<'a>),
    Instance(InstanceDef<'a>),
    VarDef(VarDef<'a>),
    VarType(VarTypeDef<'a>),
}

#[derive(Debug, Clone)]
pub struct ClassDef<'a> {
    pub name: &'a str,
    pub params: Vec<&'a str>,
    pub fundeps: Vec<(Vec<&'a str>, Vec<&'a str>)>,
    pub members: Vec<VarTypeDef<'a>>,
}

#[derive(Debug, Clone)]
pub struct InstanceDef<'a> {
    pub context: Vec<TypeConstraint<'a>>,
    pub class_name: &'a str,
    pub types: Vec<TypeExpr<'a>>,
    pub members: Vec<VarDef<'a>>,
}

#[derive(Debug, Clone)]
pub struct VarDef<'a> {
    pub name: &'a str,
    pub args: Vec<&'a str>,
    pub body: &'a Expr<'a>,
}

#[derive(Debug, Clone)]
pub struct VarTypeDef<'a> {
    pub name: &'a str,
    pub ty: QualTypeExpr<'a>,
}

#[derive(Debug, Clone)]
pub struct QualTypeExpr<'a> {
    pub context: Vec<TypeConstraint<'a>>,
    pub ty: TypeExpr<'a>,
}

#[derive(Debug, Clone)]
pub enum TypeConstraint<'a> {
    Class(&'a str, Vec<TypeExpr<'a>>),
    NotEq(TypeExpr<'a>, TypeExpr<'a>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TypeExpr<'a> {
    Var(&'a str),
    Prim(Prim),
    Tuple(Vec<TypeExpr<'a>>),
    Fn(Box<TypeExpr<'a>>, Box<TypeExpr<'a>>),
}
