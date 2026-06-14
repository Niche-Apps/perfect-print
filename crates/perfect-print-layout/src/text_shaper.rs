use perfect_print_core::draw::ShapedGlyph;
use rustybuzz::{Direction, GlyphBuffer, UnicodeBuffer};
use unicode_bidi::{BidiInfo, Level};

use crate::font_loader::LoadedFont;

/// Text shaping engine using rustybuzz.
pub struct TextShaper;

impl TextShaper {
    pub fn new() -> Self {
        Self
    }

    /// Shape a string of text into positioned glyphs.
    /// Automatically detects direction from the text content.
    pub fn shape(&self, text: &str, font_size: f64, loaded_font: &LoadedFont) -> Vec<ShapedGlyph> {
        let face = &loaded_font.face;
        let scale = loaded_font.scale_to_points(font_size);

        let mut buffer = UnicodeBuffer::new();
        buffer.push_str(text);
        buffer.guess_segment_properties();

        let glyph_buffer: GlyphBuffer = rustybuzz::shape(face, &[], buffer);

        glyph_buffer
            .glyph_infos()
            .iter()
            .zip(glyph_buffer.glyph_positions().iter())
            .map(|(info, pos)| ShapedGlyph {
                glyph_id: info.glyph_id,
                x_offset: pos.x_offset as f64 * scale,
                y_offset: pos.y_offset as f64 * scale,
                x_advance: pos.x_advance as f64 * scale,
                y_advance: pos.y_advance as f64 * scale,
                font_index: 0,
                cluster: info.cluster,
            })
            .collect()
    }

    /// Shape text with explicit direction.
    pub fn shape_with_direction(
        &self,
        text: &str,
        font_size: f64,
        loaded_font: &LoadedFont,
        direction: Direction,
    ) -> Vec<ShapedGlyph> {
        let face = &loaded_font.face;
        let scale = loaded_font.scale_to_points(font_size);

        let mut buffer = UnicodeBuffer::new();
        buffer.push_str(text);
        buffer.set_direction(direction);
        buffer.guess_segment_properties();

        let glyph_buffer: GlyphBuffer = rustybuzz::shape(face, &[], buffer);

        glyph_buffer
            .glyph_infos()
            .iter()
            .zip(glyph_buffer.glyph_positions().iter())
            .map(|(info, pos)| ShapedGlyph {
                glyph_id: info.glyph_id,
                x_offset: pos.x_offset as f64 * scale,
                y_offset: pos.y_offset as f64 * scale,
                x_advance: pos.x_advance as f64 * scale,
                y_advance: pos.y_advance as f64 * scale,
                font_index: 0,
                cluster: info.cluster,
            })
            .collect()
    }

    /// Shape text with full bidi support.
    ///
    /// This method:
    /// 1. Runs the Unicode Bidi algorithm to determine paragraph level and embedding levels
    /// 2. Reorders runs for visual order
    /// 3. Shapes each run with the correct direction
    /// 4. Returns glyphs in visual order with correct positioning
    pub fn shape_bidi(
        &self,
        text: &str,
        font_size: f64,
        loaded_font: &LoadedFont,
        base_direction: Option<Direction>,
    ) -> Vec<ShapedGlyph> {
        if text.is_empty() {
            return Vec::new();
        }

        // Run the Unicode Bidi algorithm
        let bidi_info = BidiInfo::new(
            text,
            match base_direction {
                Some(Direction::RightToLeft) => Some(Level::rtl()),
                Some(Direction::LeftToRight) => Some(Level::ltr()),
                _ => None,
            },
        );

        let _paragraph = &bidi_info.paragraphs[0];
        let levels = &bidi_info.levels;

        // Get visual reordering map from unicode-bidi
        let visual_order = BidiInfo::reorder_visual(levels);

        // Build runs: each run is a contiguous range of characters at the same level
        let mut runs: Vec<(std::ops::Range<usize>, Level)> = Vec::new();
        let bytes = text.as_bytes();
        let mut run_start = 0;
        let mut run_level = levels[0];

        for (i, &level) in levels.iter().enumerate() {
            if i == 0 {
                continue;
            }
            if level != run_level {
                runs.push((run_start..i, run_level));
                run_start = i;
                run_level = level;
            }
        }
        runs.push((run_start..bytes.len(), run_level));

        // Reorder runs visually
        let mut sorted_runs = runs.clone();
        sorted_runs.sort_by_key(|(range, _)| {
            visual_order
                .get(range.start)
                .copied()
                .unwrap_or(range.start)
        });

        let mut all_glyphs: Vec<ShapedGlyph> = Vec::new();
        let mut x_offset: f64 = 0.0;

        for (run_range, level) in &sorted_runs {
            let run_text = &text[run_range.start..run_range.end];
            if run_text.is_empty() {
                continue;
            }

            let direction = if level.is_rtl() {
                Direction::RightToLeft
            } else {
                Direction::LeftToRight
            };

            let run_glyphs = self.shape_with_direction(run_text, font_size, loaded_font, direction);

            if direction == Direction::RightToLeft {
                let run_width: f64 = run_glyphs.iter().map(|g| g.x_advance).sum();
                let mut pen_x = x_offset + run_width;
                for glyph in run_glyphs {
                    pen_x -= glyph.x_advance;
                    all_glyphs.push(ShapedGlyph {
                        x_offset: pen_x - x_offset,
                        ..glyph
                    });
                }
                x_offset += run_width;
            } else {
                let run_width: f64 = run_glyphs.iter().map(|g| g.x_advance).sum();
                for glyph in run_glyphs {
                    all_glyphs.push(ShapedGlyph {
                        x_offset: glyph.x_offset + x_offset,
                        ..glyph
                    });
                }
                x_offset += run_width;
            }
        }

        all_glyphs
    }

    /// Measure the width of a text string at a given font size.
    pub fn measure_width(&self, text: &str, font_size: f64, loaded_font: &LoadedFont) -> f64 {
        let glyphs = self.shape(text, font_size, loaded_font);
        glyphs.iter().map(|g| g.x_advance).sum()
    }

    /// Measure width with bidi support.
    pub fn measure_width_bidi(
        &self,
        text: &str,
        font_size: f64,
        loaded_font: &LoadedFont,
        base_direction: Option<Direction>,
    ) -> f64 {
        let glyphs = self.shape_bidi(text, font_size, loaded_font, base_direction);
        glyphs.iter().map(|g| g.x_advance).sum()
    }
}

impl Default for TextShaper {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::font_loader::{FontCache, FontProperties};
    use perfect_print_core::font::{FontStyle, FontWeight};

    fn get_test_font() -> Option<LoadedFont> {
        let mut cache = FontCache::default();
        for family in &[
            "Helvetica",
            "Arial",
            "Times New Roman",
            "Courier",
            "DejaVu Sans",
        ] {
            let props = FontProperties {
                family: family.to_string(),
                style: FontStyle::Normal,
                weight: FontWeight::Normal,
            };
            if let Some(font) = cache.get(&props) {
                return Some(font);
            }
        }
        None
    }

    #[test]
    fn test_shape_latin_text() {
        let font = get_test_font().expect("No system font found");
        let shaper = TextShaper::new();
        let glyphs = shaper.shape("Hello", 12.0, &font);
        assert!(!glyphs.is_empty(), "Should produce glyphs for 'Hello'");
        assert_eq!(glyphs.len(), 5, "Should have 5 glyphs for 'Hello'");
    }

    #[test]
    fn test_shape_empty_text() {
        let font = get_test_font().expect("No system font found");
        let shaper = TextShaper::new();
        let glyphs = shaper.shape("", 12.0, &font);
        assert!(glyphs.is_empty(), "Empty text should produce no glyphs");
    }

    #[test]
    fn test_measure_width() {
        let font = get_test_font().expect("No system font found");
        let shaper = TextShaper::new();
        let w1 = shaper.measure_width("Hello", 12.0, &font);
        let w2 = shaper
            .shape("Hello", 12.0, &font)
            .iter()
            .map(|g| g.x_advance)
            .sum::<f64>();
        assert!(
            (w1 - w2).abs() < 0.001,
            "measure_width should match sum of advances"
        );
        assert!(w1 > 0.0, "Text should have positive width");
    }

    #[test]
    fn test_glyph_advances_are_positive() {
        let font = get_test_font().expect("No system font found");
        let shaper = TextShaper::new();
        let glyphs = shaper.shape("ABCDEFGHIJ", 14.0, &font);
        for glyph in &glyphs {
            assert!(glyph.x_advance > 0.0, "Glyph advance should be positive");
        }
    }

    #[test]
    fn test_shape_rtl_text() {
        let font = get_test_font().expect("No system font found");
        let shaper = TextShaper::new();

        // Arabic text (RTL) - "سلام" (salam/peace)
        let rtl_text = "سلام";
        let glyphs = shaper.shape_with_direction(rtl_text, 12.0, &font, Direction::RightToLeft);

        assert!(!glyphs.is_empty(), "RTL text should produce glyphs");
    }

    #[test]
    fn test_shape_bidi_mixed_text() {
        let font = get_test_font().expect("No system font found");
        let shaper = TextShaper::new();

        // Mixed LTR+RTL text
        let mixed = "Hello سلام World";
        let glyphs = shaper.shape_bidi(mixed, 12.0, &font, None);

        assert!(!glyphs.is_empty(), "Mixed bidi text should produce glyphs");

        let total_width: f64 = glyphs.iter().map(|g| g.x_advance).sum();
        assert!(total_width > 0.0, "Total width should be positive");
    }

    #[test]
    fn test_shape_bidi_explicit_ltr() {
        let font = get_test_font().expect("No system font found");
        let shaper = TextShaper::new();

        let text = "Hello World";
        let glyphs = shaper.shape_bidi(text, 12.0, &font, Some(Direction::LeftToRight));
        assert!(!glyphs.is_empty());

        let total_width: f64 = glyphs.iter().map(|g| g.x_advance).sum();
        assert!(total_width > 0.0);
    }

    #[test]
    fn test_shape_bidi_explicit_rtl() {
        let font = get_test_font().expect("No system font found");
        let shaper = TextShaper::new();

        let text = "Hello World";
        let glyphs = shaper.shape_bidi(text, 12.0, &font, Some(Direction::RightToLeft));
        assert!(!glyphs.is_empty());

        let total_width: f64 = glyphs.iter().map(|g| g.x_advance).sum();
        assert!(total_width > 0.0);
    }

    #[test]
    fn test_shape_bidi_empty() {
        let font = get_test_font().expect("No system font found");
        let shaper = TextShaper::new();

        let glyphs = shaper.shape_bidi("", 12.0, &font, None);
        assert!(glyphs.is_empty());
    }
}
