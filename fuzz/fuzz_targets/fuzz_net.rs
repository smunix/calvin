#![no_main]
use libfuzzer_sys::fuzz_target;
use calvin_net::protocol::HNetCmd;

fuzz_target!(|data: &[u8]| {
    if let Some(&cmd_byte) = data.first() {
        let _ = HNetCmd::try_from(cmd_byte);
    }
});
