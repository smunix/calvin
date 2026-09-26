use crate::context::TypeContext;
use crate::lang::types::{Constraint, MonoType};

/// A Qualifier manages the extraction and resolution of constraint classes
/// (e.g. standard typeclasses) from the environment.
pub trait Qualifier<'a> {
    fn handles(&self, class_name: &str) -> bool;

    /// Returns true if the constraint is fully resolved by the environment.
    fn is_satisfied(&self, ctx: &'a TypeContext, constraint: &Constraint<'a>) -> bool;

    /// Qualifies a type signature by extracting any implicit class constraints
    /// it implies, based on instance matching in the environment.
    fn qualify(&self, ctx: &'a TypeContext, ty: &'a MonoType<'a>) -> Vec<Constraint<'a>>;
}

pub struct QualifierSet<'a> {
    qualifiers: Vec<Box<dyn Qualifier<'a> + 'a>>,
}

impl<'a> QualifierSet<'a> {
    pub fn new() -> Self {
        Self {
            qualifiers: Vec::new(),
        }
    }

    pub fn add_qualifier(&mut self, q: Box<dyn Qualifier<'a> + 'a>) {
        self.qualifiers.push(q);
    }
}

impl<'a> Default for QualifierSet<'a> {
    fn default() -> Self {
        Self::new()
    }
}
