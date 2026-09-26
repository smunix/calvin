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
