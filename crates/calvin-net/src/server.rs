use crate::connection::Connection;
use std::collections::HashMap;
use std::io;
use std::sync::{Arc, Mutex};

// A simple representation of what a JIT-compiled closure would look like
type JittedFn = Arc<dyn Fn(&[u8]) -> Vec<u8> + Send + Sync>;

#[derive(Clone)]
pub struct ServerState {
    expressions: Arc<Mutex<HashMap<u32, JittedFn>>>,
    next_id: Arc<Mutex<u32>>,
}

impl ServerState {
    pub fn new() -> Self {
        Self {
            expressions: Arc::new(Mutex::new(HashMap::new())),
            next_id: Arc::new(Mutex::new(1)),
        }
    }

    pub fn register(&self, func: JittedFn) -> u32 {
        let mut id_guard = self.next_id.lock().unwrap();
        let id = *id_guard;
        *id_guard += 1;
        self.expressions.lock().unwrap().insert(id, func);
        id
    }

    pub fn invoke(&self, id: u32, payload: &[u8]) -> Option<Vec<u8>> {
        let func = {
            let guard = self.expressions.lock().unwrap();
            guard.get(&id).cloned()
        };
        func.map(|f| f(payload))
    }
}

impl Default for ServerState {
    fn default() -> Self {
        Self::new()
    }
}

pub async fn handle_connection(mut conn: Connection, _state: ServerState) -> io::Result<()> {
    conn.handshake_server().await?;

    // Simplistic command loop
    loop {
        std::thread::sleep(std::time::Duration::from_millis(100));
        // Read 1-byte command (simulating for DEFEXPR / INVOKE)
        // Note: For INVOKE, Hobbes uses cmd=2.
        // For DEFEXPR, cmd=0.
    }
}
