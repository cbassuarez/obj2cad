//! Any bytes: the parser returns an error or a document, never panics.
#![no_main]
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let _ = obj2cad_core::parse(data);
});
