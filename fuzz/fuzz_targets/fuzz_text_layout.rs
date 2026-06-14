//! Fuzz target: Text shaping and layout
//!
//! Feeds arbitrary text to the paragraph engine to find panics or unbounded
//! memory usage during text shaping and layout.

#![no_main]

use libfuzzer_sys::fuzz_target;
use perfect_print_core::draw::TextStyle;
use perfect_print_core::font::FontRef;
use perfect_print_layout::flow::{ContentBlock, FlowConfig, FlowLayoutEngine};

fuzz_target!(|data: &[u8]| {
    if let Ok(text) = std::str::from_utf8(data) {
        let style = TextStyle::new(FontRef::new("Helvetica"), 12.0);
        let block = ContentBlock::Paragraph {
            text: text.to_string(),
            style,
        };
        let config = FlowConfig::default();
        let mut engine = FlowLayoutEngine::new(config);
        let _ = engine.layout(&[block]);
    }
});
