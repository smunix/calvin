use crate::context::TypeContext;
use crate::lang::types::{MonoType, Prim};
use std::collections::HashMap;

/// Determines how edges are keyed in the Trie.
#[derive(Clone, PartialEq, Eq, Hash)]
pub enum EdgeKey<'a> {
    Prim(Prim),
    Array,
    FixedArray(usize),
    Fn,
    Tuple(usize),
    Record(&'a [&'a str]),
    Variant(&'a [&'a str]),
    App,
    Class(&'a str),
}

pub struct TrieNode<'a, T> {
    pub concrete: HashMap<EdgeKey<'a>, Box<TrieNode<'a, T>>>,
    pub wildcards: Vec<Box<TrieNode<'a, T>>>,
    pub value: Option<T>,
}

impl<'a, T> Default for TrieNode<'a, T> {
    fn default() -> Self {
        Self {
            concrete: HashMap::new(),
            wildcards: Vec::new(),
            value: None,
        }
    }
}

pub struct TypeMap<'a, T> {
    root: TrieNode<'a, T>,
}

impl<'a, T> TypeMap<'a, T> {
    pub fn new() -> Self {
        Self {
            root: TrieNode::default(),
        }
    }

    pub fn insert(&mut self, key: EdgeKey<'a>, value: T) {
        let node = self
            .root
            .concrete
            .entry(key)
            .or_insert_with(|| Box::new(TrieNode::default()));
        node.value = Some(value);
    }

    pub fn get(&self, key: &EdgeKey<'a>) -> Option<&T> {
        self.root
            .concrete
            .get(key)
            .and_then(|node| node.value.as_ref())
    }

    pub fn canonicalize<'ctx>(
        &self,
        _ctx: &'ctx TypeContext,
        ty: &'a MonoType<'a>,
    ) -> &'a MonoType<'a> {
        // Simple canonicalizer placeholder for Phase 7 implementation:
        // We return the allocated types into a single arena with deterministic TGen assignments.
        // For now, we return `ty` directly if we don't do deep skolemization in this stub.
        ty
    }
}

impl<'a, T> Default for TypeMap<'a, T> {
    fn default() -> Self {
        Self::new()
    }
}
