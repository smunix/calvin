use std::io;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

/// Value Object representing a network port for Calvin event services.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Port(pub u16);

impl Port {
    pub fn new(port: u16) -> io::Result<Self> {
        if port == 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Port must be greater than zero",
            ));
        }
        Ok(Self(port))
    }

    pub const fn as_u16(self) -> u16 {
        self.0
    }
}

impl From<u16> for Port {
    fn from(p: u16) -> Self {
        Self(p)
    }
}

impl From<Port> for u16 {
    fn from(p: Port) -> Self {
        p.0
    }
}

impl std::fmt::Display for Port {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

pub struct HttpServer {
    port: Port,
}

impl HttpServer {
    pub fn new(port: impl Into<Port>) -> Self {
        Self { port: port.into() }
    }

    pub fn port(&self) -> Port {
        self.port
    }

    pub async fn run(&self) -> io::Result<()> {
        let addr = format!("0.0.0.0:{}", self.port);
        let listener = TcpListener::bind(&addr).await?;
        println!("HTTP server listening on {}", addr);

        loop {
            let (socket, _) = listener.accept().await?;
            tokio::spawn(handle_http_connection(socket));
        }
    }
}

async fn handle_http_connection(mut socket: tokio::net::TcpStream) {
    let mut request_buffer = [0; 1024];
    if let Ok(bytes_read) = socket.read(&mut request_buffer).await {
        if bytes_read == 0 {
            return;
        }
        // Very basic HTTP 200 OK response
        let response = "HTTP/1.1 200 OK\r\nContent-Length: 13\r\n\r\nHello, Calvin!";
        let _ = socket.write_all(response.as_bytes()).await;
    }
}
