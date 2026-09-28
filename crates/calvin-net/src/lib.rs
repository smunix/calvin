pub mod connection;
pub mod protocol;
pub mod server;

#[cfg(test)]
mod domain_tests {
    use super::protocol::{DefExprPayload, ExprId, HNetCmd, InvokePayload, ProtocolVersion};
    use super::server::ServerState;
    use std::sync::Arc;

    #[test]
    fn test_protocol_value_objects() {
        assert!(ProtocolVersion::V1.is_compatible());
        assert!(!ProtocolVersion(0x9999).is_compatible());

        let id = ExprId::new(42);
        assert_eq!(id.as_u32(), 42);
        assert_eq!(format!("{}", id), "42");
        assert_eq!(u32::from(id), 42);

        let def_payload = DefExprPayload::new(id, "1 + 2", "()", "int");
        assert_eq!(def_payload.id(), id);
        assert_eq!(def_payload.expr_str, "1 + 2");

        let invoke_payload = InvokePayload::new(id, vec![1, 2, 3]);
        assert_eq!(invoke_payload.id(), id);
        assert_eq!(invoke_payload.payload, vec![1, 2, 3]);

        assert_eq!(HNetCmd::try_from(0).unwrap(), HNetCmd::DefExpr);
        assert_eq!(HNetCmd::try_from(2).unwrap(), HNetCmd::Invoke);
        assert!(HNetCmd::try_from(99).is_err());
    }

    #[test]
    fn test_server_state_entity() {
        let state = ServerState::new();
        let registered_id = state.register(Arc::new(|payload| {
            let mut res = payload.to_vec();
            res.reverse();
            res
        }));

        let out = state.invoke(registered_id, &[1, 2, 3]);
        assert_eq!(out, Some(vec![3, 2, 1]));

        // Invoke with raw u32 also works seamlessly via Into<ExprId>
        let out_raw = state.invoke(registered_id.as_u32(), &[4, 5]);
        assert_eq!(out_raw, Some(vec![5, 4]));
    }
}
