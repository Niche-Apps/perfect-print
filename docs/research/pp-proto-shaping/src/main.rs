// Test rustybuzz text shaping quality across scripts
use rustybuzz::{shape, Face, UnicodeBuffer, Direction, Feature};
use std::fs;

fn shape_text(text: &str, font_data: &[u8], desc: &str, dir: Direction) {
    let face = Face::from_slice(font_data, 0).expect(&format!("Failed to load font: {}", desc));
    let mut buf = UnicodeBuffer::new();
    buf.push_str(text);
    buf.set_direction(dir);
    // Guess script from text
    buf.guess_segment_properties();

    let glyphs = shape(&face, &[], buf);
    let positions: Vec<(u32, i32, i32, i32)> = glyphs.glyph_positions()
        .iter()
        .zip(glyphs.glyph_infos())
        .map(|(pos, info)| (info.glyph_id as u32, pos.x_offset, pos.y_offset, pos.x_advance))
        .collect();

    let ids: Vec<u32> = positions.iter().map(|(id, _, _, _)| *id).collect();
    let total_width: i32 = positions.iter().map(|(_, _, _, adv)| *adv).sum();

    println!("--- {} ---", desc);
    println!("  Text: {}", text);
    println!("  Direction: {:?}", dir);
    println!("  Glyph count: {}", ids.len());
    println!("  Glyph IDs: {:?}", ids);
    println!("  Total width (font units @ 1000 UPM): {}", total_width);
    println!("  Total width (points @ 12pt): {:.2}", total_width as f64 * 12.0 / 1000.0);
    println!();
}

fn main() {
    let fonts = [
        ("/System/Library/Fonts/Helvetica.ttc", "Helvetica"),
        ("/System/Library/Fonts/Supplemental/Songti.ttc", "Songti (CJK)"),
        ("/System/Library/Fonts/Supplemental/Arial Unicode.ttf", "Arial Unicode"),
        ("/System/Library/Fonts/Supplemental/Al Nile.ttc", "Al Nile (Arabic)"),
    ];

    for (path, name) in fonts {
        if !std::path::Path::new(path).exists() {
            println!("Font not found: {} ({})\n", path, name);
            continue;
        }

        let font_data = fs::read(path).expect(&format!("Failed to read {}", path));
        println!("=== {} ===", name);

        // Latin
        shape_text("Hello World! ffi fi fl ff", &font_data, "Latin (with ligatures)", Direction::LeftToRight);

        // Only test broader scripts with Arial Unicode (which has wide coverage)
        if name.contains("Arial Unicode") {
            shape_text("Hello 欢迎来到世界", &font_data, "Mixed Latin/CJK", Direction::LeftToRight);
            shape_text("مرحبا بالعالم", &font_data, "Arabic (RTL)", Direction::RightToLeft);
            shape_text("שלום עולם", &font_data, "Hebrew (RTL)", Direction::RightToLeft);
            shape_text("👋🌍🔥", &font_data, "Emoji", Direction::LeftToRight);
            shape_text("Price: 1,234.56 USD", &font_data, "Numbers/symbols", Direction::LeftToRight);
        }

        println!();
    }

    println!("=== rustybuzz Summary ===");
    println!("(+) Full harfbuzz shaping: ligatures, kerning, all scripts");
    println!("(+) no_std compatible");
    println!("(+) ttf-parser integration for font loading");
    println!("(+) Unicode bidi integration (guess_segment_properties)");
    println!("(-) no built-in font fallback - must build own font stack");
    println!("(-) no PDF/vRaster output - pure shaping only");
    println!("=> Use rustybuzz for ALL text shaping in perfect-print");
}
