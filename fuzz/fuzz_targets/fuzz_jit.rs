#![no_main]
use libfuzzer_sys::fuzz_target;
use calvin_codegen_cranelift::jit::JITCompiler;

fuzz_target!(|_data: &[u8]| {
    // Basic JIT compiler fuzzer setup. Arbitrary bytes can't just be blindly executed safely,
    // so we just fuzz JIT engine instantiation for regressions in cranelift configuration.
    let _jit = JITCompiler::new();
});
