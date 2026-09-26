use std::io;

pub const HNET_VERSION: u32 = 0x00010000;

#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq)]
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

pub struct DefExprPayload {
    pub exprid: u32,
    pub expr_str: String,
    pub in_type_str: String,
    pub out_type_str: String,
}

pub struct InvokePayload {
    pub exprid: u32,
    pub payload: Vec<u8>,
}
