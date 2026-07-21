//! Integration test: on systems where a font resolves to a TrueType
//! Collection (`.ttc`, e.g. Helvetica on macOS), the PDF's embedded
//! `/FontFile2` stream must be a standalone sfnt for the selected face, not
//! the whole collection.
//!
//! Skips gracefully (with an explanatory `eprintln!`) on platforms/fonts
//! where the resolved source isn't a `.ttc`, so it doesn't rot on Linux/CI
//! images without Helvetica.ttc.

use perfect_print_core::document::{DocumentBuilder, PageBuilder};
use perfect_print_core::draw::DrawCommand;
use perfect_print_core::font::FontRef;
use perfect_print_core::page::PageSize;
use perfect_print_core::units::Point;
use perfect_print_pdf::PdfRenderer;

/// Find a system font family that resolves to a `.ttc` source, returning
/// (family_name, face_index) for a query fontdb actually satisfies. We
/// probe a handful of common macOS TTC-backed families since exact
/// availability depends on the OS/version running the test.
fn find_ttc_backed_family() -> Option<(String, u32)> {
    let mut db = fontdb::Database::new();
    db.load_system_fonts();

    for candidate in ["Helvetica", "Arial", "Times New Roman", "Georgia"] {
        let query = fontdb::Query {
            families: &[fontdb::Family::Name(candidate)],
            ..Default::default()
        };
        if let Some(face_id) = db.query(&query) {
            if let Some(face_info) = db.face(face_id) {
                if let fontdb::Source::File(path) = &face_info.source {
                    if path.extension().and_then(|e| e.to_str()) == Some("ttc") {
                        return Some((candidate.to_string(), face_info.index));
                    }
                }
            }
        }
    }
    None
}

#[test]
fn ttc_backed_font_embeds_single_face_not_whole_collection() {
    let Some((family, face_index)) = find_ttc_backed_family() else {
        eprintln!(
            "Skipping: no system font on this machine resolves to a .ttc source \
             (test only meaningful on macOS with Helvetica.ttc-style collections)."
        );
        return;
    };
    eprintln!(
        "Testing TTC embedding with family '{}' at face index {}",
        family, face_index
    );

    let mut page = perfect_print_core::page::Page::new(PageSize::Letter);
    page.add(DrawCommand::Text {
        run: perfect_print_core::draw::TextRun {
            text: "TTC embedding test".to_string(),
            glyphs: vec![],
            style: perfect_print_core::draw::TextStyle::new(FontRef::new(&family), 14.0),
        },
        position: Point::new(72.0, 72.0),
        max_width: None,
    });

    let model = DocumentBuilder::new().add_page(page).build().unwrap();
    let bytes = PdfRenderer::new().render_to_bytes(&model).unwrap();

    let doc = lopdf::Document::load_mem(&bytes).expect("produced PDF must be loadable");

    let mut found_font_file = false;
    for (_id, object) in doc.objects.iter() {
        if let lopdf::Object::Stream(stream) = object {
            if stream.dict.has(b"Length1") {
                // This is a /FontFile2 stream (the only stream type in this
                // crate's output that sets Length1).
                found_font_file = true;
                let content = &stream.content;
                assert!(
                    content.len() >= 4,
                    "embedded font stream should not be empty/truncated"
                );
                assert_ne!(
                    &content[0..4],
                    b"ttcf",
                    "embedded /FontFile2 must NOT start with the ttcf TrueType Collection magic"
                );
                let sfnt_version = u32::from_be_bytes([content[0], content[1], content[2], content[3]]);
                let is_valid_sfnt_tag = sfnt_version == 0x0001_0000
                    || &content[0..4] == b"OTTO"
                    || &content[0..4] == b"true"
                    || &content[0..4] == b"typ1";
                assert!(
                    is_valid_sfnt_tag,
                    "embedded /FontFile2 should start with a valid sfnt version tag, got {:?}",
                    &content[0..4]
                );

                // Extracted bytes should be dramatically smaller than a
                // whole multi-face collection (sanity bound, not exact).
                assert!(
                    content.len() < 20_000_000,
                    "embedded font stream suspiciously large ({} bytes) — looks like a whole collection",
                    content.len()
                );
            }
        }
    }

    assert!(
        found_font_file,
        "expected at least one /FontFile2 (Length1-bearing) stream in the PDF"
    );
}
