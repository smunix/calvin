use crate::connection::Connection;
use crate::protocol::ExprId;
use std::collections::HashMap;
use std::io;
use std::sync::{Arc, Mutex};

// A simple representation of what a JIT-compiled closure would look like
type JittedFn = Arc<dyn Fn(&[u8]) -> Vec<u8> + Send + Sync>;

#[derive(Clone)]
pub struct ServerState {
    expressions: Arc<Mutex<HashMap<ExprId, JittedFn>>>,
    next_id: Arc<Mutex<u32>>,
}

impl ServerState {
    pub fn new() -> Self {
        Self {
            expressions: Arc::new(Mutex::new(HashMap::new())),
            next_id: Arc::new(Mutex::new(1)),
        }
    }

    pub fn register(&self, func: JittedFn) -> ExprId {
        let mut id_guard = self.next_id.lock().unwrap();
        let id = ExprId(*id_guard);
        *id_guard += 1;
        self.expressions.lock().unwrap().insert(id, func);
        id
    }

    pub fn invoke(&self, id: impl Into<ExprId>, payload: &[u8]) -> Option<Vec<u8>> {
        let expr_id = id.into();
        let func = {
            let guard = self.expressions.lock().unwrap();
            guard.get(&expr_id).cloned()
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
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
        // Read 1-byte command (simulating for DEFEXPR / INVOKE)
        // Note: For INVOKE, Hobbes uses cmd=2.
        // For DEFEXPR, cmd=0.
    }
}
