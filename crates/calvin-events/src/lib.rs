pub mod events {
    use tokio::runtime::Runtime;

    pub struct EventLoop {
        rt: Runtime,
    }

    impl EventLoop {
        pub fn new() -> std::io::Result<Self> {
            Ok(Self {
                rt: tokio::runtime::Builder::new_multi_thread()
                    .enable_all()
                    .build()?,
            })
        }

        pub fn block_on<F: std::future::Future>(&self, future: F) -> F::Output {
            self.rt.block_on(future)
        }

        pub fn spawn<F>(&self, future: F) -> tokio::task::JoinHandle<F::Output>
        where
            F: std::future::Future + Send + 'static,
            F::Output: Send + 'static,
        {
            self.rt.spawn(future)
        }
    }
}
pub mod httpd;

#[cfg(test)]
mod domain_tests {
    use super::httpd::{HttpServer, Port};

    #[test]
    fn test_port_domain_invariants() {
        assert!(Port::new(0).is_err());
        let port = Port::new(8080).unwrap();
        assert_eq!(port.as_u16(), 8080);
        assert_eq!(format!("{}", port), "8080");

        let server = HttpServer::new(port);
        assert_eq!(server.port(), Port(8080));

        let server2 = HttpServer::new(9090u16);
        assert_eq!(server2.port().as_u16(), 9090);
    }
}

