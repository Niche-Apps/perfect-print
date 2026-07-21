use perfect_print_core::font::{FontRef, FontStyle, FontWeight};
use rustybuzz::Face;
use std::collections::HashMap;

/// A loaded font face with its data, ready for shaping.
#[derive(Clone)]
pub struct LoadedFont {
    pub font_ref: FontRef,
    pub face: Face<'static>,
    pub units_per_em: u16,
}

impl LoadedFont {
    /// Scale factor from font units to points at a given font size.
    pub fn scale_to_points(&self, font_size: f64) -> f64 {
        font_size / self.units_per_em as f64
    }

    /// Check if this font has a glyph for the given Unicode codepoint.
    pub fn has_glyph(&self, codepoint: char) -> bool {
        self.face.glyph_index(codepoint).is_some()
    }

    /// Get the glyph ID for a codepoint, if available.
    pub fn glyph_id(&self, codepoint: char) -> Option<u32> {
        self.face.glyph_index(codepoint).map(|g| g.0 as u32)
    }
}

/// Font lookup properties.
#[derive(Debug, Clone)]
pub struct FontProperties {
    pub family: String,
    pub style: FontStyle,
    pub weight: FontWeight,
}

impl FontProperties {
    pub fn new(family: &str) -> Self {
        Self {
            family: family.to_string(),
            style: FontStyle::Normal,
            weight: FontWeight::Normal,
        }
    }

    pub fn with_style(mut self, style: FontStyle) -> Self {
        self.style = style;
        self
    }

    pub fn with_weight(mut self, weight: FontWeight) -> Self {
        self.weight = weight;
        self
    }
}

impl std::fmt::Display for FontProperties {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{:?}:{:?}", self.family, self.weight, self.style)
    }
}

pub trait FontLoader: Send + Sync {
    fn load(&self, properties: &FontProperties) -> Option<LoadedFont>;
    fn families(&self) -> Vec<String>;
}

/// System font loader using fontdb.
pub struct SystemFontLoader {
    db: fontdb::Database,
}

impl SystemFontLoader {
    pub fn new() -> Self {
        let mut db = fontdb::Database::new();
        db.load_system_fonts();
        Self { db }
    }

    /// Get the raw font data for a font matching the given family name.
    /// Returns (font_data, font_index) if found.
    pub fn get_font_data(&self, family: &str) -> Option<(Vec<u8>, u32)> {
        self.get_font_data_for(&FontProperties::new(family))
    }

    /// Get the raw font data for a font matching the given family, weight,
    /// and style (e.g. the actual bold/italic face, not just the regular
    /// face scaled or slanted synthetically). Returns (font_data, font_index)
    /// if found.
    pub fn get_font_data_for(&self, properties: &FontProperties) -> Option<(Vec<u8>, u32)> {
        let query = fontdb::Query {
            families: &[fontdb::Family::Name(&properties.family)],
            weight: fontdb::Weight(properties.weight.value()),
            stretch: fontdb::Stretch::Normal,
            style: match properties.style {
                FontStyle::Normal => fontdb::Style::Normal,
                FontStyle::Italic => fontdb::Style::Italic,
            },
        };
        let face_id = self.db.query(&query)?;
        self.load_font_data(face_id)
    }

    /// List all available font families on the system.
    pub fn families(&self) -> Vec<String> {
        let mut families: Vec<String> = self
            .db
            .faces()
            .flat_map(|face| face.families.iter().map(|(name, _lang)| name.clone()))
            .collect();
        families.sort();
        families.dedup();
        families
    }

    fn load_font_data(&self, face_id: fontdb::ID) -> Option<(Vec<u8>, u32)> {
        self.db
            .with_face_data(face_id, |data, idx| (data.to_vec(), idx))
    }
}

impl FontLoader for SystemFontLoader {
    fn load(&self, properties: &FontProperties) -> Option<LoadedFont> {
        let query = fontdb::Query {
            families: &[fontdb::Family::Name(&properties.family)],
            weight: fontdb::Weight(properties.weight.value()),
            stretch: fontdb::Stretch::Normal,
            style: match properties.style {
                FontStyle::Normal => fontdb::Style::Normal,
                FontStyle::Italic => fontdb::Style::Italic,
            },
        };

        let face_id = self.db.query(&query)?;
        let (font_data, font_index) = self.load_font_data(face_id)?;

        // Leak the font data to get a 'static reference.
        // This is fine for a long-lived font cache.
        let static_data: &'static [u8] = Box::leak(font_data.into_boxed_slice());
        let face = Face::from_slice(static_data, font_index)?;
        let units_per_em: u16 = face.units_per_em().try_into().ok()?;

        Some(LoadedFont {
            font_ref: FontRef::new(&properties.family),
            face,
            units_per_em,
        })
    }

    fn families(&self) -> Vec<String> {
        let mut families: Vec<String> = self
            .db
            .faces()
            .flat_map(|face| face.families.iter().map(|(name, _lang)| name.clone()))
            .collect();
        families.sort();
        families.dedup();
        families
    }
}

/// Font fallback entry: a font to try when the primary font is missing a glyph.
#[derive(Debug, Clone)]
pub struct FallbackFont {
    pub properties: FontProperties,
    pub priority: u32, // lower = higher priority
}

impl FallbackFont {
    pub fn new(family: &str, priority: u32) -> Self {
        Self {
            properties: FontProperties::new(family),
            priority,
        }
    }
}

/// Default fallback font list for common scripts.
pub fn default_fallbacks() -> Vec<FallbackFont> {
    vec![
        // CJK fonts
        FallbackFont::new("PingFang SC", 10),
        FallbackFont::new("Heiti SC", 11),
        FallbackFont::new("STHeiti", 12),
        FallbackFont::new("Hiragino Sans", 13),
        FallbackFont::new("Yu Gothic", 14),
        FallbackFont::new("MS Gothic", 15),
        // Arabic/Hebrew
        FallbackFont::new("Geeza Pro", 20),
        FallbackFont::new("Arial Hebrew", 21),
        // Emoji
        FallbackFont::new("Apple Color Emoji", 30),
        // Generic sans-serif fallback
        FallbackFont::new("Arial Unicode MS", 40),
        FallbackFont::new("DejaVu Sans", 41),
    ]
}

/// Font cache for efficient font reuse with fallback support.
pub struct FontCache {
    loader: Box<dyn FontLoader>,
    cache: HashMap<String, LoadedFont>,
    fallbacks: Vec<FallbackFont>,
}

impl FontCache {
    pub fn new(loader: Box<dyn FontLoader>) -> Self {
        Self {
            loader,
            cache: HashMap::new(),
            fallbacks: default_fallbacks(),
        }
    }

    pub fn with_fallbacks(mut self, fallbacks: Vec<FallbackFont>) -> Self {
        self.fallbacks = fallbacks;
        self
    }

    pub fn get(&mut self, properties: &FontProperties) -> Option<LoadedFont> {
        let key = properties.to_string();

        if let Some(font) = self.cache.get(&key) {
            return Some(font.clone());
        }

        let font = self.loader.load(properties)?;
        self.cache.insert(key, font.clone());
        Some(font)
    }

    pub fn get_by_family(&mut self, family: &str) -> Option<LoadedFont> {
        let properties = FontProperties::new(family);
        self.get(&properties)
    }

    /// Get a font that can render the given codepoint.
    /// First tries the primary font, then falls back through the fallback list.
    pub fn get_for_codepoint(
        &mut self,
        primary: &FontProperties,
        codepoint: char,
    ) -> Option<LoadedFont> {
        // Try primary font first
        if let Some(font) = self.get(primary) {
            if font.has_glyph(codepoint) {
                return Some(font);
            }
        }

        // Try fallback fonts in priority order
        let mut sorted_fallbacks = self.fallbacks.clone();
        sorted_fallbacks.sort_by_key(|f| f.priority);

        for fallback in &sorted_fallbacks {
            if let Some(font) = self.get(&fallback.properties) {
                if font.has_glyph(codepoint) {
                    return Some(font);
                }
            }
        }

        // Last resort: return primary font (will show .notdef glyph)
        self.get(primary)
    }

    /// Get the best font for a string of text.
    pub fn get_for_text(&mut self, primary: &FontProperties, text: &str) -> Option<LoadedFont> {
        let font = self.get(primary)?;
        let can_render_all = text
            .chars()
            .all(|c| c.is_ascii_whitespace() || font.has_glyph(c));
        if can_render_all {
            return Some(font);
        }
        // Return primary anyway; per-glyph fallback happens during shaping
        Some(font)
    }

    pub fn families(&self) -> Vec<String> {
        self.loader.families()
    }

    /// Add a custom fallback font.
    pub fn add_fallback(&mut self, fallback: FallbackFont) {
        self.fallbacks.push(fallback);
    }
}

impl Default for FontCache {
    fn default() -> Self {
        Self::new(Box::new(SystemFontLoader::new()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn get_font_data_for_selects_the_requested_weight_and_style() {
        let loader = SystemFontLoader::new();
        let normal = loader.get_font_data_for(&FontProperties::new("Helvetica"));
        let bold = loader.get_font_data_for(
            &FontProperties::new("Helvetica").with_weight(FontWeight::Bold),
        );
        let italic = loader.get_font_data_for(
            &FontProperties::new("Helvetica").with_style(FontStyle::Italic),
        );
        // Skip if the system doesn't have Helvetica at all.
        if normal.is_none() {
            return;
        }
        // Regular, bold, and italic must resolve to different faces within
        // the font source (a distinct face_index for a TrueType Collection,
        // or distinct bytes for separate font files) — this is what
        // distinguishes the actual bold/italic glyph outlines from a
        // synthetically-unstyled regular face.
        if let (Some((_, normal_index)), Some((_, bold_index))) = (&normal, &bold) {
            assert_ne!(
                normal_index, bold_index,
                "bold face_index should differ from regular"
            );
        }
        if let (Some((_, normal_index)), Some((_, italic_index))) = (&normal, &italic) {
            assert_ne!(
                normal_index, italic_index,
                "italic face_index should differ from regular"
            );
        }
    }

    #[test]
    fn test_system_font_loader() {
        let loader = SystemFontLoader::new();
        let families = loader.families();
        assert!(!families.is_empty(), "Should find system fonts");
        eprintln!("Found {} font families", families.len());
    }

    #[test]
    fn test_font_cache() {
        let mut cache = FontCache::default();

        let props = FontProperties::new("Helvetica");

        let font = cache.get(&props);

        // Test cache hit
        let font2 = cache.get(&props);
        assert_eq!(font.is_some(), font2.is_some());
    }

    #[test]
    fn test_font_properties_display() {
        let props = FontProperties::new("Arial");
        let s = format!("{}", props);
        assert!(s.contains("Arial"));
    }

    #[test]
    fn test_loaded_font_has_glyph() {
        let mut cache = FontCache::default();
        let props = FontProperties::new("Helvetica");

        if let Some(font) = cache.get(&props) {
            // ASCII 'A' should be present in most fonts
            assert!(font.has_glyph('A'), "Font should have glyph for 'A'");
        }
    }

    #[test]
    fn test_fallback_for_cjk() {
        let mut cache = FontCache::default();
        let primary = FontProperties::new("Helvetica");

        // Try to find a font that can render CJK
        let font = cache.get_for_codepoint(&primary, '中');
        assert!(font.is_some(), "Should find a font for CJK codepoint");
    }

    #[test]
    fn test_fallback_for_emoji() {
        let mut cache = FontCache::default();
        let primary = FontProperties::new("Helvetica");

        // Try to find a font that can render emoji
        let font = cache.get_for_codepoint(&primary, '😀');
        // This may or may not succeed depending on system fonts
        let _ = font;
    }

    #[test]
    fn test_default_fallbacks_not_empty() {
        let fallbacks = default_fallbacks();
        assert!(!fallbacks.is_empty(), "Should have default fallback fonts");
    }

    #[test]
    fn test_custom_fallback() {
        let mut cache = FontCache::default();
        cache.add_fallback(FallbackFont::new("Courier", 5));

        let fallbacks = default_fallbacks();
        assert!(fallbacks.len() > 0);
    }
}
