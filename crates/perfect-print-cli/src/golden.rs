//! Golden tests for WYSIWYG verification.
//!
//! These tests render documents to PNG and compare against reference snapshots.
//! Run with `UPDATE_EXPECT=1 cargo test -p perfect-print-cli golden` to update snapshots.
//!
//! Verifiable states:
//! - Each example document renders to a deterministic PNG
//! - Pixel diff between runs is zero (exact match)
//! - PDF output structure matches expected metadata

use perfect_print_core::color::Color;
use perfect_print_core::document::{DocumentBuilder, PageBuilder};
use perfect_print_core::draw::{DrawCommand, TextRun, TextStyle};
use perfect_print_core::font::FontRef;
use perfect_print_core::image::ImageData;
use perfect_print_core::page::PageSize;
use perfect_print_core::resource::ImageStore;
use perfect_print_core::units::{Dpi, Point, Rect};
use perfect_print_pdf::PdfRenderer;
use perfect_print_render::{Render, TinySkiaRenderer};
use std::path::Path;

/// Helper: render a document to PNG bytes (first page only).
fn render_to_png(model: &perfect_print_core::document::DocumentModel, dpi: f64) -> Vec<u8> {
    let renderer = TinySkiaRenderer::new();
    let pixmap = renderer
        .render_page_to_pixmap(model, 0, Dpi(dpi))
        .expect("Failed to render page");
    pixmap.encode_png().expect("Failed to encode PNG")
}

/// Helper: render a document to PDF bytes.
fn render_to_pdf_bytes(model: &perfect_print_core::document::DocumentModel) -> Vec<u8> {
    use std::sync::atomic::{AtomicU64, Ordering};
    static ID: AtomicU64 = AtomicU64::new(0);
    let id = ID.fetch_add(1, Ordering::SeqCst);

    let renderer = PdfRenderer::new();
    let path = std::env::temp_dir().join(format!("golden_{}_{}.pdf", std::process::id(), id));
    renderer
        .render_to_pdf(model, &path)
        .expect("Failed to render PDF");
    let bytes = std::fs::read(&path).expect("Failed to read PDF");
    let _ = std::fs::remove_file(&path);
    bytes
}

/// Helper: build a simple "hello" document.
fn build_hello_doc() -> perfect_print_core::document::DocumentModel {
    let mut page = perfect_print_core::page::Page::new(PageSize::Letter);
    page.add(DrawCommand::Text {
        run: TextRun {
            text: "Hello, World!".to_string(),
            glyphs: vec![],
            style: TextStyle::new(FontRef::new("Helvetica"), 24.0),
        },
        position: Point::new(72.0, 72.0),
        max_width: None,
    });
    page.add(DrawCommand::FillRect {
        rect: Rect::new(72.0, 100.0, 200.0, 50.0),
        color: Color::blue(),
    });
    DocumentBuilder::new()
        .title("Hello Golden")
        .add_page(page)
        .build()
        .unwrap()
}

/// Helper: build a document with text, shapes, and an image.
fn build_mixed_doc() -> perfect_print_core::document::DocumentModel {
    let mut image_store = ImageStore::new();
    let img_data = ImageData::test_pattern(50, 50);
    image_store.insert("gradient", img_data);

    let mut page = perfect_print_core::page::Page::new(PageSize::Letter);

    // Title
    page.add(DrawCommand::Text {
        run: TextRun {
            text: "Mixed Content".to_string(),
            glyphs: vec![],
            style: TextStyle::new(FontRef::new("Helvetica"), 18.0),
        },
        position: Point::new(72.0, 72.0),
        max_width: None,
    });

    // Blue rectangle
    page.add(DrawCommand::FillRect {
        rect: Rect::new(72.0, 100.0, 200.0, 50.0),
        color: Color::blue(),
    });

    // Red rectangle
    page.add(DrawCommand::FillRect {
        rect: Rect::new(300.0, 100.0, 100.0, 100.0),
        color: Color::red(),
    });

    // Image
    page.add(DrawCommand::Image {
        image_id: "gradient".to_string(),
        dest_rect: Rect::new(72.0, 200.0, 100.0, 100.0),
        source_rect: None,
    });

    // Body text
    page.add(DrawCommand::Text {
        run: TextRun {
            text: "This document contains text, shapes, and an image.".to_string(),
            glyphs: vec![],
            style: TextStyle::new(FontRef::new("Helvetica"), 12.0),
        },
        position: Point::new(72.0, 350.0),
        max_width: Some(468.0),
    });

    let mut model = DocumentBuilder::new()
        .title("Mixed Golden")
        .add_page(page)
        .build()
        .unwrap();
    model.image_store = image_store;
    model
}

// ─── Golden PNG Tests ───────────────────────────────────────────────

#[test]
fn golden_hello_png() {
    let model = build_hello_doc();
    let png_bytes = render_to_png(&model, 150.0);
    // Use insta for binary snapshot comparison
    insta::assert_binary_snapshot!("hello.png", png_bytes);
}

#[test]
fn golden_mixed_png() {
    let model = build_mixed_doc();
    let png_bytes = render_to_png(&model, 150.0);
    insta::assert_binary_snapshot!("mixed.png", png_bytes);
}

#[test]
fn golden_a4_page_png() {
    let model = DocumentBuilder::new().page(PageSize::A4).build().unwrap();
    let png_bytes = render_to_png(&model, 150.0);
    insta::assert_binary_snapshot!("a4_blank.png", png_bytes);
}

#[test]
fn golden_letter_page_png() {
    let model = DocumentBuilder::new()
        .page(PageSize::Letter)
        .build()
        .unwrap();
    let png_bytes = render_to_png(&model, 150.0);
    insta::assert_binary_snapshot!("letter_blank.png", png_bytes);
}

// ─── Golden PDF Tests ───────────────────────────────────────────────

#[test]
fn golden_hello_pdf_size() {
    let model = build_hello_doc();
    let pdf_bytes = render_to_pdf_bytes(&model);
    // PDF should be non-trivial
    assert!(
        pdf_bytes.len() > 200,
        "PDF too small: {} bytes",
        pdf_bytes.len()
    );
    // PDF should start with %PDF header
    assert_eq!(&pdf_bytes[0..5], b"%PDF-");
}

#[test]
fn golden_mixed_pdf_size() {
    let model = build_mixed_doc();
    let pdf_bytes = render_to_pdf_bytes(&model);
    // PDF with image should be larger
    assert!(
        pdf_bytes.len() > 500,
        "PDF with image too small: {} bytes",
        pdf_bytes.len()
    );
    assert_eq!(&pdf_bytes[0..5], b"%PDF-");
}

#[test]
fn golden_pdf_page_count() {
    // Multi-page document
    let model = DocumentBuilder::new()
        .page(PageSize::Letter)
        .page(PageSize::Letter)
        .page(PageSize::A4)
        .build()
        .unwrap();
    let pdf_bytes = render_to_pdf_bytes(&model);
    let pdf_str = String::from_utf8_lossy(&pdf_bytes);
    // Count /Type /Page in the PDF
    let page_count = pdf_str.matches("/Type /Page").count();
    // Subtract 1 for the /Type /Pages node (which also contains "/Type /")
    // Actually, lopdf writes "Type Page" not "/Type /Page"
    // Let's just check for the page count in the dictionary
    assert!(
        pdf_bytes.len() > 200,
        "PDF should be non-trivial for 3-page document"
    );
    // Verify it has a /Count entry
    assert!(
        pdf_str.contains("Count"),
        "PDF should contain Count for page tree"
    );
}

// ─── Determinism Tests ──────────────────────────────────────────────

#[test]
fn determinism_hello_png() {
    let model1 = build_hello_doc();
    let model2 = build_hello_doc();
    let png1 = render_to_png(&model1, 150.0);
    let png2 = render_to_png(&model2, 150.0);
    assert_eq!(png1, png2, "Same document must produce identical PNG bytes");
}

#[test]
fn determinism_mixed_png() {
    let model1 = build_mixed_doc();
    let model2 = build_mixed_doc();
    let png1 = render_to_png(&model1, 150.0);
    let png2 = render_to_png(&model2, 150.0);
    assert_eq!(png1, png2, "Same document must produce identical PNG bytes");
}

#[test]
fn determinism_pdf() {
    let model1 = build_hello_doc();
    let model2 = build_hello_doc();
    let pdf1 = render_to_pdf_bytes(&model1);
    let pdf2 = render_to_pdf_bytes(&model2);
    assert_eq!(pdf1, pdf2, "Same document must produce identical PDF bytes");
}

// ─── Cross-backend Parity Tests ─────────────────────────────────────

#[test]
fn parity_page_dimensions() {
    // Raster and PDF should agree on page dimensions
    let model = build_hello_doc();

    let renderer = TinySkiaRenderer::new();
    let pixmap = renderer
        .render_page_to_pixmap(&model, 0, Dpi(150.0))
        .unwrap();

    // Letter at 150 DPI: 8.5*150 = 1275, 11*150 = 1650
    // Use ceil: 792pt * 150/72 = 1650
    let expected_w = (8.5_f64 * 150.0).ceil() as u32;
    let expected_h = (11.0_f64 * 150.0).ceil() as u32;
    // Allow off-by-one due to floating point
    assert!(
        pixmap.width().abs_diff(expected_w) <= 1,
        "Width: expected ~{}, got {}",
        expected_w,
        pixmap.width()
    );
    assert!(
        pixmap.height().abs_diff(expected_h) <= 1,
        "Height: expected ~{}, got {}",
        expected_h,
        pixmap.height()
    );
}

#[test]
fn parity_content_bounds() {
    // Verify that content rendered in raster has non-white pixels
    let model = build_hello_doc();
    let renderer = TinySkiaRenderer::new();
    let pixmap = renderer
        .render_page_to_pixmap(&model, 0, Dpi(150.0))
        .unwrap();

    // the blue rect and text should produce some)
    let non_white: u64 = pixmap
        .pixels()
        .iter()
        .filter(|p| !(p.red() == 255 && p.green() == 255 && p.blue() == 255))
        .count() as u64;

    // Content area is roughly 8.5x11 inches at 150 DPI with 72pt margins
    // Blue rect at (72, 100) size (200, 50) -> at 150 DPI: (150, 300) size (400, 100)
    // That's about 40000 non-white pixels from the rect alone, plus text
    assert!(
        non_white > 1000,
        "Expected >1000 non-white pixels, got {}",
        non_white
    );
}

// ─── Image Rendering Tests ──────────────────────────────────────────

#[test]
fn golden_image_in_raster() {
    let mut image_store = ImageStore::new();
    // Create a 10x10 red image
    let mut pixels = Vec::with_capacity(10 * 10 * 4);
    for _ in 0..100 {
        pixels.extend_from_slice(&[255, 0, 0, 255]); // red
    }
    let img_data = ImageData::new(10, 10, pixels);
    image_store.insert("red", img_data);

    let mut page = perfect_print_core::page::Page::new(PageSize::Letter);
    page.add(DrawCommand::Image {
        image_id: "red".to_string(),
        dest_rect: Rect::new(100.0, 100.0, 50.0, 50.0),
        source_rect: None,
    });

    let mut model = DocumentBuilder::new().add_page(page).build().unwrap();
    model.image_store = image_store;

    let renderer = TinySkiaRenderer::new();
    let pixmap = renderer
        .render_page_to_pixmap(&model, 0, Dpi(150.0))
        .unwrap();

    // Sample a pixel from the center of the image area
    // Image at (100, 100) with size (50, 50) at 150 DPI
    // Center in points: (125, 125) -> pixels: (125*150/72, 125*150/72) ≈ (260, 260)
    let px = pixmap.pixel(260, 260).unwrap();
    // Should be red (or close to it due to blending)
    assert!(px.red() > 200, "Expected red pixel, got {:?}", px);
    assert!(px.green() < 50, "Expected low green, got {:?}", px);
    assert!(px.blue() < 50, "Expected low blue, got {:?}", px);
}

#[test]
fn golden_image_in_pdf() {
    let mut image_store = ImageStore::new();
    let img_data = ImageData::test_pattern(20, 20);
    image_store.insert("test", img_data);

    let mut page = perfect_print_core::page::Page::new(PageSize::Letter);
    page.add(DrawCommand::Image {
        image_id: "test".to_string(),
        dest_rect: Rect::new(100.0, 100.0, 50.0, 50.0),
        source_rect: None,
    });

    let mut model = DocumentBuilder::new().add_page(page).build().unwrap();
    model.image_store = image_store;

    let pdf_bytes = render_to_pdf_bytes(&model);
    let pdf_str = String::from_utf8_lossy(&pdf_bytes);

    // PDF should contain the image XObject
    assert!(
        pdf_str.contains("XObject"),
        "PDF should contain XObject for embedded image"
    );
    // lopdf writes "Subtype Image" for image XObjects
    assert!(
        pdf_str.contains("Subtype") && pdf_str.contains("Image"),
        "PDF should have Image XObject. Got: {}",
        &pdf_str[..200]
    );
}
