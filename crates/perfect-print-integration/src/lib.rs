//! Integration tests: build → render → parse → verify.

use perfect_print_core::color::Color;
use perfect_print_core::document::{DocumentBuilder, PageBuilder};
use perfect_print_core::draw::{DrawCommand, TextRun, TextStyle};
use perfect_print_core::font::FontRef;
use perfect_print_core::image::ImageData;
use perfect_print_core::page::PageSize;
use perfect_print_core::resource::ImageStore;
use perfect_print_core::units::{Point, Rect};
use perfect_print_pdf::PdfRenderer;
use perfect_print_render::Render;
use std::sync::atomic::{AtomicU64, Ordering};

static TEST_ID: AtomicU64 = AtomicU64::new(0);

/// Render a document to PDF bytes (thread-safe unique temp files).
fn render_to_pdf_bytes(model: &perfect_print_core::document::DocumentModel) -> Vec<u8> {
    let id = TEST_ID.fetch_add(1, Ordering::SeqCst);
    let renderer = PdfRenderer::new();
    let path = std::env::temp_dir().join(format!("integ_{}_{}.pdf", std::process::id(), id));
    renderer
        .render_to_pdf(model, &path)
        .expect("Failed to render PDF");
    let bytes = std::fs::read(&path).expect("Failed to read PDF");
    let _ = std::fs::remove_file(&path);
    bytes
}

// ─── End-to-End: Simple Document ─────────────────────────────────────

#[test]
fn e2e_simple_hello_pdf() {
    let model = DocumentBuilder::new()
        .title("Hello")
        .page(PageSize::Letter)
        .build()
        .unwrap();

    let pdf_bytes = render_to_pdf_bytes(&model);
    assert!(&pdf_bytes[0..5] == b"%PDF-");
    assert!(pdf_bytes.len() > 200);
}

#[test]
fn e2e_multi_page_pdf() {
    let model = DocumentBuilder::new()
        .page(PageSize::Letter)
        .page(PageSize::A4)
        .page(PageSize::Legal)
        .build()
        .unwrap();

    let pdf_bytes = render_to_pdf_bytes(&model);
    assert!(pdf_bytes.len() > 400, "Multi-page PDF should be larger");
    assert_eq!(&pdf_bytes[0..5], b"%PDF-");
}

#[test]
fn e2e_document_with_text_pdf() {
    let mut page = perfect_print_core::page::Page::new(PageSize::Letter);
    page.add(DrawCommand::Text {
        run: TextRun {
            text: "Integration Test".to_string(),
            glyphs: vec![],
            style: TextStyle::new(FontRef::new("Helvetica"), 14.0),
        },
        position: Point::new(72.0, 72.0),
        max_width: None,
    });

    let model = DocumentBuilder::new().add_page(page).build().unwrap();
    let pdf_bytes = render_to_pdf_bytes(&model);

    assert!(pdf_bytes.len() > 200);
    let pdf_str = String::from_utf8_lossy(&pdf_bytes);
    assert!(pdf_str.contains("BT"), "PDF should contain text markers");
}

#[test]
fn e2e_document_with_shapes_pdf() {
    let mut page = perfect_print_core::page::Page::new(PageSize::Letter);
    page.add(DrawCommand::FillRect {
        rect: Rect::new(100.0, 100.0, 200.0, 50.0),
        color: Color::blue(),
    });
    page.add(DrawCommand::FillRect {
        rect: Rect::new(100.0, 200.0, 100.0, 100.0),
        color: Color::red(),
    });

    let model = DocumentBuilder::new().add_page(page).build().unwrap();
    let pdf_bytes = render_to_pdf_bytes(&model);

    assert!(pdf_bytes.len() > 300);
    assert_eq!(&pdf_bytes[0..5], b"%PDF-");
}

// ─── End-to-End: Image Document ──────────────────────────────────────

#[test]
fn e2e_document_with_image_pdf() {
    let mut image_store = ImageStore::new();
    let img_data = ImageData::test_pattern(30, 30);
    image_store.insert("test-img", img_data);

    let mut page = perfect_print_core::page::Page::new(PageSize::Letter);
    page.add(DrawCommand::Image {
        image_id: "test-img".to_string(),
        dest_rect: Rect::new(100.0, 100.0, 50.0, 50.0),
        source_rect: None,
    });

    let mut model = DocumentBuilder::new().add_page(page).build().unwrap();
    model.image_store = image_store;

    let pdf_bytes = render_to_pdf_bytes(&model);
    let size_with_image = pdf_bytes.len();

    // Compare with a document without images
    let page2 = perfect_print_core::page::Page::new(PageSize::Letter);
    let model2 = DocumentBuilder::new().add_page(page2).build().unwrap();
    let pdf_bytes2 = render_to_pdf_bytes(&model2);

    assert!(
        size_with_image > pdf_bytes2.len(),
        "PDF with image ({}) should be larger than without ({})",
        size_with_image,
        pdf_bytes2.len()
    );

    let pdf_str = String::from_utf8_lossy(&pdf_bytes);
    assert!(pdf_str.contains("XObject"), "PDF should have XObject");
    assert!(
        pdf_str.contains("FlateDecode"),
        "PDF should use FlateDecode"
    );
}

#[test]
fn e2e_image_determinism() {
    let render = || {
        let mut image_store = ImageStore::new();
        image_store.insert("img", ImageData::test_pattern(10, 10));

        let mut page = perfect_print_core::page::Page::new(PageSize::Letter);
        page.add(DrawCommand::Image {
            image_id: "img".to_string(),
            dest_rect: Rect::new(100.0, 100.0, 50.0, 50.0),
            source_rect: None,
        });

        let mut model = DocumentBuilder::new().add_page(page).build().unwrap();
        model.image_store = image_store;
        render_to_pdf_bytes(&model)
    };

    let pdf1 = render();
    let pdf2 = render();
    assert_eq!(
        pdf1, pdf2,
        "Same image document must produce identical PDF bytes"
    );
}

// ─── End-to-End: Public API ──────────────────────────────────────────

#[test]
fn e2e_public_api_hello() {
    let doc = perfect_print::Document::new()
        .title("Hello")
        .add(perfect_print::Paragraph::new("Hello from the public API!"))
        .build();

    let pdf_bytes = render_to_pdf_bytes(&doc);
    assert!(pdf_bytes.len() > 200);
    assert_eq!(&pdf_bytes[0..5], b"%PDF-");
}

#[test]
fn e2e_public_api_multipage() {
    let doc = perfect_print::Document::new()
        .title("Multi")
        .add(perfect_print::Paragraph::new("Page 1").font_size(24.0))
        .add(perfect_print::PageBreak)
        .add(perfect_print::Paragraph::new("Page 2").font_size(24.0))
        .build();

    let pdf_bytes = render_to_pdf_bytes(&doc);
    assert!(pdf_bytes.len() > 300);
}

#[test]
fn e2e_public_api_save_pdf() {
    let path = std::env::temp_dir().join("e2e_save_pdf.pdf");
    let _ = std::fs::remove_file(&path);

    perfect_print::Document::new()
        .title("Save Test")
        .add(perfect_print::Paragraph::new("Testing save_pdf"))
        .save_pdf(&path)
        .expect("save_pdf should succeed");

    assert!(path.exists());
    assert!(std::fs::metadata(&path).unwrap().len() > 200);
    let _ = std::fs::remove_file(&path);
}

#[test]
fn e2e_public_api_render_png() {
    let dir = std::env::temp_dir().join("e2e_render_png");
    let _ = std::fs::remove_dir_all(&dir);

    let paths = perfect_print::Document::new()
        .add(perfect_print::Paragraph::new("PNG test"))
        .render_png(&dir, 150)
        .expect("render_png should succeed");

    assert!(!paths.is_empty());
    for path in &paths {
        assert!(path.exists());
    }
    let _ = std::fs::remove_dir_all(&dir);
}

// ─── End-to-End: PDF <-> Raster Parity ────────────────────────────────

#[test]
fn e2e_pdf_raster_page_count_parity() {
    let model = DocumentBuilder::new()
        .page(PageSize::Letter)
        .page(PageSize::A4)
        .build()
        .unwrap();

    let pdf_bytes = render_to_pdf_bytes(&model);
    let single = DocumentBuilder::new()
        .page(PageSize::Letter)
        .build()
        .unwrap();
    let single_pdf = render_to_pdf_bytes(&single);
    assert!(pdf_bytes.len() > single_pdf.len());

    let renderer = perfect_print_render::TinySkiaRenderer::new();
    let dir = std::env::temp_dir().join("e2e_parity");
    let _ = std::fs::create_dir_all(&dir);
    let raster_paths = renderer
        .render_to_raster(&model, perfect_print_core::units::Dpi::PRINT_STANDARD, &dir)
        .unwrap();
    assert_eq!(raster_paths.len(), 2);
    let _ = std::fs::remove_dir_all(&dir);
}

// ─── End-to-End: Error Handling ──────────────────────────────────────

#[test]
fn e2e_empty_document_fails() {
    assert!(DocumentBuilder::new().build().is_err());
}

#[test]
fn e2e_invalid_page_size_fails() {
    assert!(DocumentBuilder::new()
        .page(PageSize::Custom {
            width: 0.0,
            height: 100.0
        })
        .build()
        .is_err());
}

#[test]
fn e2e_large_document() {
    let mut builder = DocumentBuilder::new();
    for _ in 0..20 {
        builder = builder.page(PageSize::Letter);
    }
    let pdf_bytes = render_to_pdf_bytes(&builder.build().unwrap());
    assert!(pdf_bytes.len() > 2000, "20-page PDF should be >2000 bytes");
}

// ─── End-to-End: Determinism ─────────────────────────────────────────

#[test]
fn e2e_pdf_determinism() {
    let model = DocumentBuilder::new()
        .page(PageSize::Letter)
        .build()
        .unwrap();
    let pdf1 = render_to_pdf_bytes(&model);
    let pdf2 = render_to_pdf_bytes(&model);
    assert_eq!(pdf1, pdf2, "Same document must produce identical PDF bytes");
}

#[test]
fn e2e_raster_determinism() {
    let model = DocumentBuilder::new()
        .page(PageSize::Letter)
        .build()
        .unwrap();
    let renderer = perfect_print_render::TinySkiaRenderer::new();

    let dir1 = std::env::temp_dir().join("e2e_det1");
    let dir2 = std::env::temp_dir().join("e2e_det2");
    let _ = std::fs::create_dir_all(&dir1);
    let _ = std::fs::create_dir_all(&dir2);

    let p1 = renderer
        .render_to_raster(
            &model,
            perfect_print_core::units::Dpi::PRINT_STANDARD,
            &dir1,
        )
        .unwrap();
    let p2 = renderer
        .render_to_raster(
            &model,
            perfect_print_core::units::Dpi::PRINT_STANDARD,
            &dir2,
        )
        .unwrap();

    assert_eq!(p1.len(), p2.len());
    for (a, b) in p1.iter().zip(p2.iter()) {
        assert_eq!(std::fs::read(a).unwrap(), std::fs::read(b).unwrap());
    }

    let _ = std::fs::remove_dir_all(&dir1);
    let _ = std::fs::remove_dir_all(&dir2);
}
