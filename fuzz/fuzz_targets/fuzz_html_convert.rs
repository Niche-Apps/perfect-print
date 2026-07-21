//! Fuzz target: HTML/CSS pipeline (`perfect-print-html`)
//!
//! Feeds arbitrary bytes, interpreted as HTML source, through the full
//! `HtmlDocument::render()` pipeline (parse → CSS cascade → convert →
//! flow layout → `DocumentModel`) to find panics, hangs, or unbounded
//! memory usage. The pipeline is designed to never hard-error on
//! unsupported markup/CSS (see `docs/html-css-support.md`), so a `render()`
//! failure here is not itself a bug — only a panic is.

#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(s) = std::str::from_utf8(data) {
        let _ = perfect_print_html::HtmlDocument::new(s).render();
    }
});
