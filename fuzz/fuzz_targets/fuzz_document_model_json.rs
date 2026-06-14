//! Fuzz target: DocumentModel JSON deserialization
//!
//! Feeds arbitrary bytes to the DocumentModel JSON parser to find panics,
//! hangs, or unbounded memory usage.

#![no_main]

use libfuzzer_sys::fuzz_target;
use perfect_print_core::document::DocumentModel;

fuzz_target!(|data: &[u8]| {
    // Try to deserialize arbitrary bytes as JSON into DocumentModel
    if let Ok(s) = std::str::from_utf8(data) {
        let _ = serde_json::from_str::<DocumentModel>(s);
    }
});
