pub mod jit;
pub mod types;

/// Value Object representing a named symbol registered in the JIT execution engine.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct JitSymbolName(pub String);

impl JitSymbolName {
    pub fn new(name: impl Into<String>) -> Self {
        Self(name.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for JitSymbolName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<&str> for JitSymbolName {
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}

impl From<String> for JitSymbolName {
    fn from(s: String) -> Self {
        Self(s)
    }
}

#[cfg(test)]
mod domain_tests {
    use super::JitSymbolName;

    #[test]
    fn test_cranelift_jit_symbol_name_value_object() {
        let sym = JitSymbolName::new("cranelift_func");
        assert_eq!(sym.as_str(), "cranelift_func");
        assert_eq!(format!("{}", sym), "cranelift_func");

        let sym2: JitSymbolName = "calvin_alloc".into();
        assert_eq!(sym2.as_str(), "calvin_alloc");
    }
}

