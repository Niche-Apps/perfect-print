//! Fuzz target: PDF rendering
//!
//! Feeds arbitrary bytes to the PDF renderer to find panics or unbounded
//! memory usage when processing malformed input.

#![no_main]

use libfuzzer_sys::fuzz_target;
use perfect_print_core::document::DocumentModel;
use perfect_print_pdf::PdfRenderer;

fuzz_target!(|data: &[u8]| {
    // Try to deserialize arbitrary bytes as a DocumentModel, then render to PDF
    if let Ok(s) = std::str::from_utf8(data) {
        if let Ok(model) = serde_json::from_str::<DocumentModel>(s) {
            let renderer = PdfRenderer::new();
            let _ = renderer.render_to_pdf(&model, std::path::Path::new("/tmp/fuzz_output.pdf"));
        }
    }
});
