#![no_main]
use libfuzzer_sys::fuzz_target;
use std::io::Write;

fuzz_target!(|data: &[u8]| {
    // Write fuzz data to a temporary file, then try to load it via FRegion::open
    if let Ok(mut tmp) = tempfile::NamedTempFile::new() {
        let _ = tmp.write_all(data);
        let _ = calvin_storage::fregion::FRegion::open(tmp.path());
    }
});
