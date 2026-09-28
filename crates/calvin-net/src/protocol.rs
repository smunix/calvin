use std::io;

pub const HNET_VERSION: u32 = 0x00010000;

/// Value Object representing the wire protocol version for Calvin network RPC.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ProtocolVersion(pub u32);

impl ProtocolVersion {
    pub const V1: Self = Self(HNET_VERSION);

    pub const fn is_compatible(self) -> bool {
        self.0 == HNET_VERSION
    }

    pub const fn as_u32(self) -> u32 {
        self.0
    }
}

impl Default for ProtocolVersion {
    fn default() -> Self {
        Self::V1
    }
}

/// Value Object representing a unique identifier for a remotely defined Calvin expression.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ExprId(pub u32);

impl ExprId {
    pub const fn new(id: u32) -> Self {
        Self(id)
    }

    pub const fn as_u32(self) -> u32 {
        self.0
    }
}

impl std::fmt::Display for ExprId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<u32> for ExprId {
    fn from(id: u32) -> Self {
        Self(id)
    }
}

impl From<ExprId> for u32 {
    fn from(id: ExprId) -> Self {
        id.0
    }
}

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HNetCmd {
    DefExpr = 0,
    Prepare = 1, // Optional in Calvin right now
    Invoke = 2,
}

impl TryFrom<u8> for HNetCmd {
    type Error = io::Error;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(HNetCmd::DefExpr),
            1 => Ok(HNetCmd::Prepare),
            2 => Ok(HNetCmd::Invoke),
            _ => Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Unknown HNet command",
            )),
        }
    }
}

/// Domain payload representing a defined expression registration over the network.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DefExprPayload {
    pub exprid: u32,
    pub expr_str: String,
    pub in_type_str: String,
    pub out_type_str: String,
}

impl DefExprPayload {
    pub fn new(
        exprid: impl Into<ExprId>,
        expr_str: impl Into<String>,
        in_type_str: impl Into<String>,
        out_type_str: impl Into<String>,
    ) -> Self {
        Self {
            exprid: exprid.into().as_u32(),
            expr_str: expr_str.into(),
            in_type_str: in_type_str.into(),
            out_type_str: out_type_str.into(),
        }
    }

    pub fn id(&self) -> ExprId {
        ExprId(self.exprid)
    }
}

/// Domain payload representing a remote expression invocation over the network.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvokePayload {
    pub exprid: u32,
    pub payload: Vec<u8>,
}

impl InvokePayload {
    pub fn new(exprid: impl Into<ExprId>, payload: Vec<u8>) -> Self {
        Self {
            exprid: exprid.into().as_u32(),
            payload,
        }
    }

    pub fn id(&self) -> ExprId {
        ExprId(self.exprid)
    }
}

