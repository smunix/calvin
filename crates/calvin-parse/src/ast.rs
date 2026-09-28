use calvin_core::lang::expr::Expr;
use calvin_core::lang::types::Prim;

/// Aggregate Root representing a parsed Calvin/Hobbes compilation unit or module.
#[derive(Debug, Clone)]
pub struct Module<'a> {
    pub name: Option<&'a str>,
    pub defs: Vec<ModuleDef<'a>>,
}

impl<'a> Module<'a> {
    pub fn new(name: Option<&'a str>, defs: Vec<ModuleDef<'a>>) -> Self {
        Self { name, defs }
    }

    pub fn name(&self) -> Option<&'a str> {
        self.name
    }

    pub fn defs(&self) -> &[ModuleDef<'a>] {
        &self.defs
    }

    pub fn add_def(&mut self, def: ModuleDef<'a>) {
        self.defs.push(def);
    }

    pub fn classes(&self) -> impl Iterator<Item = &ClassDef<'a>> {
        self.defs.iter().filter_map(|d| match d {
            ModuleDef::Class(c) => Some(c),
            _ => None,
        })
    }

    pub fn instances(&self) -> impl Iterator<Item = &InstanceDef<'a>> {
        self.defs.iter().filter_map(|d| match d {
            ModuleDef::Instance(i) => Some(i),
            _ => None,
        })
    }

    pub fn var_defs(&self) -> impl Iterator<Item = &VarDef<'a>> {
        self.defs.iter().filter_map(|d| match d {
            ModuleDef::VarDef(v) => Some(v),
            _ => None,
        })
    }

    pub fn var_type_defs(&self) -> impl Iterator<Item = &VarTypeDef<'a>> {
        self.defs.iter().filter_map(|d| match d {
            ModuleDef::VarType(vt) => Some(vt),
            _ => None,
        })
    }

    pub fn data_defs(&self) -> impl Iterator<Item = &DataDef<'a>> {
        self.defs.iter().filter_map(|d| match d {
            ModuleDef::Data(dt) => Some(dt),
            _ => None,
        })
    }
}

#[derive(Debug, Clone)]
pub enum ModuleDef<'a> {
    Class(ClassDef<'a>),
    Instance(InstanceDef<'a>),
    VarDef(VarDef<'a>),
    VarType(VarTypeDef<'a>),
    Data(DataDef<'a>),
}

#[derive(Debug, Clone)]
pub struct DataDef<'a> {
    pub name: &'a str,
    pub params: Vec<&'a str>,
    pub ty: TypeExpr<'a>,
}

#[derive(Debug, Clone)]
pub struct ClassDef<'a> {
    pub context: Vec<TypeConstraint<'a>>,
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

impl<'a> VarDef<'a> {
    pub fn is_function(&self) -> bool {
        !self.args.is_empty()
    }
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

impl<'a> QualTypeExpr<'a> {
    pub fn is_monomorphic(&self) -> bool {
        self.context.is_empty()
    }
}

#[derive(Debug, Clone)]
pub enum TypeConstraint<'a> {
    Class(&'a str, Vec<TypeExpr<'a>>),
    NotEq(TypeExpr<'a>, TypeExpr<'a>),
    Eq(TypeExpr<'a>, TypeExpr<'a>),
    FieldLookup(&'a str, &'a str, TypeExpr<'a>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TypeExpr<'a> {
    Var(&'a str),
    Prim(Prim),
    Tuple(Vec<TypeExpr<'a>>),
    Fn(Box<TypeExpr<'a>>, Box<TypeExpr<'a>>),
    Array(Box<TypeExpr<'a>>),
    App(Box<TypeExpr<'a>>, Vec<TypeExpr<'a>>),
}

impl<'a> TypeExpr<'a> {
    pub fn is_var(&self) -> bool {
        matches!(self, TypeExpr::Var(_))
    }

    pub fn is_prim(&self) -> bool {
        matches!(self, TypeExpr::Prim(_))
    }

    pub fn is_fn(&self) -> bool {
        matches!(self, TypeExpr::Fn(_, _))
    }

    pub fn is_array(&self) -> bool {
        matches!(self, TypeExpr::Array(_))
    }
}

