use printpdf::*;
use std::fs::File;
use std::io::BufWriter;

fn main() {
    let (doc, page1, layer1) = PdfDocument::new("Proto PDF", Mm(210.0), Mm(297.0), "Layer 1");

    // Load a built-in font
    let font_bold = doc.add_builtin_font(BuiltinFont::HelveticaBold).unwrap();
    let font = doc.add_builtin_font(BuiltinFont::Helvetica).unwrap();

    let mut current_layer = doc.get_page(page1).get_layer(layer1);

    // Title
    current_layer.use_text("perfect-print Research Prototype", 24.0, Mm(15.0), Mm(270.0), &font_bold);
    current_layer.use_text("PDF Backend: printpdf 0.7", 12.0, Mm(15.0), Mm(260.0), &font);

    // Test Latin text
    let latin = "The quick brown fox jumps over the lazy dog.";
    current_layer.use_text(&format!("Latin: {}", latin), 12.0, Mm(15.0), Mm(245.0), &font);

    // Test larger Hebrew + RTL note
    current_layer.use_text("Note: printpdf uses rusttype (no harfbuzz), so complex shaping is limited.", 10.0, Mm(15.0), Mm(230.0), &font);

    // Test rectangle
    current_layer.set_fill_color(Color::Rgb(Rgb::new(0.9, 0.95, 1.0, None)));
    current_layer.set_outline_thickness(0.5);
    let rect = Rect::new(Mm(15.0), Mm(195.0), Mm(100.0), Mm(215.0));
    current_layer.add_rect(rect);

    // Text inside rectangle
    current_layer.begin_text_section();
    current_layer.set_font(&font, 10.0);
    current_layer.set_text_cursor(Mm(18.0), Mm(210.0));
    current_layer.write_text("Box with light blue fill", &font);
    current_layer.end_text_section();

    // Multi-page test
    let (page2, layer2) = doc.add_page(Mm(210.0), Mm(297.0), "Layer 1");
    let layer2 = doc.get_page(page2).get_layer(layer2);
    layer2.use_text("Page 2 - Multi-page works", 14.0, Mm(15.0), Mm(270.0), &font_bold);

    // Save
    let out_path = "/Users/josephsee/clawd/perfect-print/docs/research/proto-output.pdf";
    doc.save(&mut BufWriter::new(File::create(out_path).unwrap())).unwrap();

    println!("PDF written to {}", out_path);
    println!("Pages: 2");
    println!("Caption: printpdf works but uses rusttype (not harfbuzz) - complex text shaping WILL be limited");
}
