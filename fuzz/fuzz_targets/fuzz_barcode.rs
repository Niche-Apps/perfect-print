//! Fuzz target: Barcode encoding
//!
//! Feeds arbitrary data to the barcode encoder to find panics or unbounded
//! memory usage.

#![no_main]

use libfuzzer_sys::fuzz_target;
use perfect_print_barcode::QrCode;

fuzz_target!(|data: &[u8]| {
    if let Ok(text) = std::str::from_utf8(data) {
        let qr = QrCode::new(text);
        let _ = qr.render(4);
    }
});
