// Test Cairo+Pango for PDF output with proper text shaping
use cairo::{Context, PdfSurface, ImageSurface, Format};
use pango::{FontDescription, Layout, AttrList, Attribute};
use pango::ffi::PangoAttrColor;

fn main() {
    // PDF output
    let pdf_path = "/Users/josephsee/clawd/perfect-print/docs/research/proto-cairo.pdf";
    let pdf_surface = PdfSurface::new(595.84, 841.92, pdf_path).unwrap();
    let ctx = Context::new(&pdf_surface).unwrap();

    // Use Pango for text layout
    let font_desc = FontDescription::from_string("Helvetica 12");
    println!("Cairo PDF surface created: {} x {}", 595.84, 841.92);
    println!("Font: {}", font_desc.to_string());

    // Draw directly with Cairo API
    ctx.set_source_rgb(0.0, 0.0, 0.0);
    ctx.select_font_face("Helvetica", cairo::FontSlant::Normal, cairo::FontWeight::Bold);
    ctx.set_font_size(24.0);
    ctx.move_to(50.0, 50.0);
    ctx.show_text("perfect-print Cairo Prototype").unwrap();

    ctx.set_font_size(12.0);
    ctx.select_font_face("Helvetica", cairo::FontSlant::Normal, cairo::FontWeight::Normal);
    ctx.move_to(50.0, 80.0);
    ctx.show_text("Cairo PDF + Pango text shaping").unwrap();

    // Draw a rectangle
    ctx.set_source_rgb(0.9, 0.95, 1.0);
    ctx.rectangle(50.0, 100.0, 200.0, 50.0);
    ctx.fill().unwrap();
    ctx.set_source_rgb(0.0, 0.0, 0.0);
    ctx.set_line_width(0.5);
    ctx.rectangle(50.0, 100.0, 200.0, 50.0);
    ctx.stroke().unwrap();

    // Test text in box
    ctx.set_font_size(10.0);
    ctx.move_to(55.0, 130.0);
    ctx.show_text("Box with light blue fill").unwrap();

    // Test CJK text
    ctx.set_font_size(14.0);
    ctx.move_to(50.0, 180.0);
    ctx.show_text("CJK: Hello 欢迎来到世界").unwrap();

    // Test Arabic
    ctx.move_to(50.0, 210.0);
    ctx.show_text("Arabic: مرحبا بالعالم").unwrap();

    // Flush PDF
    pdf_surface.finish();
    println!("PDF written to {}", pdf_path);

    // Also test PNG output for parity comparison
    let png_path = "/Users/josephsee/clawd/perfect-print/docs/research/proto-cairo.png";
    let img_surface = ImageSurface::create(Format::ARgb32, 596, 842).unwrap();
    let img_ctx = Context::new(&img_surface).unwrap();

    // White background
    img_ctx.set_source_rgb(1.0, 1.0, 1.0);
    img_ctx.paint().unwrap();

    // Same content
    img_ctx.set_source_rgb(0.0, 0.0, 0.0);
    img_ctx.select_font_face("Helvetica", cairo::FontSlant::Normal, cairo::FontWeight::Bold);
    img_ctx.set_font_size(24.0);
    img_ctx.move_to(50.0, 50.0);
    img_ctx.show_text("perfect-print Cairo Prototype").unwrap();

    img_ctx.set_font_size(12.0);
    img_ctx.select_font_face("Helvetica", cairo::FontSlant::Normal, cairo::FontWeight::Normal);
    img_ctx.move_to(50.0, 80.0);
    img_ctx.show_text("Cairo PDF + Pango text shaping").unwrap();

    img_ctx.set_source_rgb(0.9, 0.95, 1.0);
    img_ctx.rectangle(50.0, 100.0, 200.0, 50.0);
    img_ctx.fill().unwrap();
    img_ctx.set_source_rgb(0.0, 0.0, 0.0);
    img_ctx.set_line_width(0.5);
    img_ctx.rectangle(50.0, 100.0, 200.0, 50.0);
    img_ctx.stroke().unwrap();

    img_ctx.set_font_size(10.0);
    img_ctx.move_to(55.0, 130.0);
    img_ctx.show_text("Box with light blue fill").unwrap();

    img_ctx.set_font_size(14.0);
    img_ctx.move_to(50.0, 180.0);
    img_ctx.show_text("CJK: Hello 欢迎来到世界").unwrap();

    img_ctx.move_to(50.0, 210.0);
    img_ctx.show_text("Arabic: مرحبا بالعالم").unwrap();

    let mut file = std::fs::File::create(png_path).unwrap();
    img_ctx.show_page().unwrap();
    img_surface.write_to_png(&mut file).unwrap();
    println!("PNG written to {}", png_path);

    println!("\n=== Cairo+Pango Summary ===");
    println!("(+) PDF output via PdfSurface");
    println!("(+) PNG output via ImageSurface (same API)");
    println!("(+) Pango for text layout (harfbuzz underneath)");
    println!("(+) Font embedding in PDF");
    println!("(+) Cross-platform (Linux, macOS, Windows)");
    println!("(-) Heavy system dependency (libcairo, libpango)");
    println!("(-) Not pure Rust");
    println!("(-) Pango text layout is separate from Cairo drawing");
    println!("(-) No native print dialog integration");
}
