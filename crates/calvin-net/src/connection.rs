use crate::protocol::{DefExprPayload, HNetCmd, HNET_VERSION};
use std::io;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

pub struct Connection {
    stream: TcpStream,
}

impl Connection {
    pub fn new(stream: TcpStream) -> Self {
        Self { stream }
    }

    pub async fn handshake_client(&mut self) -> io::Result<()> {
        self.stream.write_u32_le(HNET_VERSION).await?;
        let server_version = self.stream.read_u32_le().await?;
        if server_version != HNET_VERSION {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Version mismatch",
            ));
        }
        Ok(())
    }

    pub async fn handshake_server(&mut self) -> io::Result<()> {
        let client_version = self.stream.read_u32_le().await?;
        if client_version != HNET_VERSION {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Version mismatch",
            ));
        }
        self.stream.write_u32_le(HNET_VERSION).await?;
        Ok(())
    }

    // Read a string prefixed by a 32-bit little-endian length
    async fn read_string(&mut self) -> io::Result<String> {
        let len = self.stream.read_u32_le().await? as usize;
        let mut buf = vec![0u8; len];
        self.stream.read_exact(&mut buf).await?;
        String::from_utf8(buf)
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "Invalid UTF-8"))
    }

    async fn write_string(&mut self, s: &str) -> io::Result<()> {
        let bytes = s.as_bytes();
        self.stream.write_u32_le(bytes.len() as u32).await?;
        self.stream.write_all(bytes).await?;
        Ok(())
    }

    pub async fn send_defexpr(&mut self, payload: &DefExprPayload) -> io::Result<()> {
        self.stream.write_u8(HNetCmd::DefExpr as u8).await?;
        self.stream.write_u32_le(payload.exprid).await?;
        self.write_string(&payload.expr_str).await?;
        self.write_string(&payload.in_type_str).await?;
        self.write_string(&payload.out_type_str).await?;
        Ok(())
    }

    pub async fn recv_defexpr(&mut self) -> io::Result<DefExprPayload> {
        let exprid = self.stream.read_u32_le().await?;
        let expr_str = self.read_string().await?;
        let in_type_str = self.read_string().await?;
        let out_type_str = self.read_string().await?;
        Ok(DefExprPayload {
            exprid,
            expr_str,
            in_type_str,
            out_type_str,
        })
    }
}
