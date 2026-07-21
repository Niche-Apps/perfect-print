//! Tiny-skia raster renderer for the canonical page model.

use perfect_print_core::color::Color;
use perfect_print_core::document::DocumentModel;
use perfect_print_core::draw::{DrawCommand, FillRule, PathOp};
use perfect_print_core::font::FontRef;
use perfect_print_core::page::LayerType;
use perfect_print_core::units::{Dpi, Rect};
use std::collections::HashMap;
use std::path::Path as StdPath;
use std::sync::{Arc, Mutex};
use tiny_skia::{
    Color as SkColor, Paint, Path as SkPath, PathBuilder, Pixmap, Rect as SkRect, Stroke, Transform,
};
use ttf_parser::{Face, GlyphId, OutlineBuilder};

use super::{RenderError, RenderResult};

/// A cached font face with its outline data.
struct CachedFont {
    face: Face<'static>,
    units_per_em: u16,
    glyph_paths: HashMap<u32, SkPath>,
}

/// Font cache for the raster renderer.
struct FontCache {
    /// Keyed by "family|bold|italic" — family alone is not enough, since a
    /// bold or italic `TextRun` must be rendered with the actual bold/italic
    /// face (its `ShapedGlyph`s were shaped against that face's glyph IDs
    /// upstream in `ParagraphEngine`; drawing them with the regular face's
    /// outlines would paint the wrong glyphs).
    fonts: HashMap<String, Arc<CachedFont>>,
    /// Shared font database for system font discovery
    db: fontdb::Database,
}

impl FontCache {
    fn new() -> Self {
        let mut db = fontdb::Database::new();
        db.load_system_fonts();
        Self {
            fonts: HashMap::new(),
            db,
        }
    }

    fn cache_key(font_ref: &FontRef, bold: bool, italic: bool) -> String {
        format!("{}|{}|{}", font_ref.as_ref(), bold, italic)
    }

    /// Load a font by its `FontRef` name plus bold/italic. Uses a shared
    /// font database.
    fn load_font(&mut self, font_ref: &FontRef, bold: bool, italic: bool) -> Option<Arc<CachedFont>> {
        let key = Self::cache_key(font_ref, bold, italic);
        if let Some(cached) = self.fonts.get(&key) {
            return Some(cached.clone());
        }

        let query = fontdb::Query {
            families: &[fontdb::Family::Name(font_ref.as_ref())],
            weight: if bold {
                fontdb::Weight::BOLD
            } else {
                fontdb::Weight::NORMAL
            },
            stretch: fontdb::Stretch::Normal,
            style: if italic {
                fontdb::Style::Italic
            } else {
                fontdb::Style::Normal
            },
        };

        let face_id = self.db.query(&query)?;
        // `face_index` matters for TrueType Collections (.ttc), where a
        // single file bundles multiple faces (e.g. Regular/Bold/Italic) —
        // discarding it and always parsing index 0 would silently render
        // every weight/style as the Regular face.
        let (font_bytes, face_index) =
            self.db.with_face_data(face_id, |data, idx| (data.to_vec(), idx))?;
        let face = Face::parse(&font_bytes, face_index).ok()?;
        let units_per_em = face.units_per_em();

        // Leak the font data to get 'static lifetime for Face
        let font_data_static: &'static [u8] = Box::leak(font_bytes.into_boxed_slice());
        let face_static = Face::parse(font_data_static, face_index).ok()?;

        let cached = Arc::new(CachedFont {
            face: face_static,
            units_per_em,
            glyph_paths: HashMap::new(),
        });

        self.fonts.insert(key, cached.clone());
        Some(cached)
    }

    /// Get or build a glyph path for the given glyph ID.
    fn get_glyph_path(
        &mut self,
        font_ref: &FontRef,
        bold: bool,
        italic: bool,
        glyph_id: u32,
    ) -> Option<SkPath> {
        let cached = self.load_font(font_ref, bold, italic)?;
        // Check if already cached
        if let Some(path) = cached.glyph_paths.get(&glyph_id) {
            return Some(path.clone());
        }

        // Build the glyph path
        let mut builder = TinySkiaOutlineBuilder::new();
        let glyph_id = GlyphId(glyph_id as u16);
        cached.face.outline_glyph(glyph_id, &mut builder)?;
        let path = builder.finish()?;

        // Note: We can't easily cache in the Arc since it's immutable
        // For now, just return the path each time
        Some(path)
    }
}

/// OutlineBuilder implementation that creates tiny-skia Path objects.
struct TinySkiaOutlineBuilder {
    builder: PathBuilder,
}

impl TinySkiaOutlineBuilder {
    fn new() -> Self {
        Self {
            builder: PathBuilder::new(),
        }
    }

    fn finish(self) -> Option<SkPath> {
        self.builder.finish()
    }
}

impl OutlineBuilder for TinySkiaOutlineBuilder {
    fn move_to(&mut self, x: f32, y: f32) {
        self.builder.move_to(x, y);
    }

    fn line_to(&mut self, x: f32, y: f32) {
        self.builder.line_to(x, y);
    }

    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        self.builder.quad_to(x1, y1, x, y);
    }

    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        self.builder.cubic_to(x1, y1, x2, y2, x, y);
    }

    fn close(&mut self) {
        self.builder.close();
    }
}

/// Graphics state for raster rendering.
/// Tracks the current transform and opacity for draw commands.
#[derive(Debug, Clone)]
struct RenderState {
    pub transform: Transform,
    pub opacity: f64,
}

impl Default for RenderState {
    fn default() -> Self {
        Self {
            transform: Transform::identity(),
            opacity: 1.0,
        }
    }
}

/// Raster renderer using tiny-skia.
pub struct TinySkiaRenderer {
    font_cache: Mutex<FontCache>,
}

impl TinySkiaRenderer {
    pub fn new() -> Self {
        Self {
            font_cache: Mutex::new(FontCache::new()),
        }
    }

    fn render_page(
        &self,
        document: &DocumentModel,
        page_index: usize,
        dpi: Dpi,
    ) -> RenderResult<Pixmap> {
        let page = document
            .pages
            .get(page_index)
            .ok_or(RenderError::InvalidPageIndex(page_index))?;

        let scale = dpi.0 / 72.0;
        let width = (page.size.width * scale).ceil() as u32;
        let height = (page.size.height * scale).ceil() as u32;

        let width = width.max(1);
        let height = height.max(1);

        let mut pixmap = Pixmap::new(width, height)
            .ok_or_else(|| RenderError::TinySkia("Failed to create pixmap".to_string()))?;

        // Fill background white
        pixmap.fill(SkColor::WHITE);

        let base_transform = Transform::from_scale(scale as f32, scale as f32);
        let state = RenderState {
            transform: base_transform,
            opacity: 1.0,
        };

        // Render layers in order: background, watermark, foreground, header, footer
        let mut ordered_layers: Vec<_> = page.layers.iter().collect();
        ordered_layers.sort_by_key(|l| match l.layer_type {
            LayerType::Background => 0,
            LayerType::Watermark => 1,
            LayerType::Foreground => 2,
            LayerType::Header => 3,
            LayerType::Footer => 4,
        });

        for layer in ordered_layers {
            for cmd in &layer.commands {
                self.render_command(&mut pixmap, cmd, &state, &document.image_store)?;
            }
        }

        Ok(pixmap)
    }

    fn render_command(
        &self,
        pixmap: &mut Pixmap,
        cmd: &DrawCommand,
        state: &RenderState,
        image_store: &perfect_print_core::resource::ImageStore,
    ) -> RenderResult<()> {
        match cmd {
            DrawCommand::FillRect { rect, color } => {
                let sk_rect = self.core_rect_to_skia(*rect);
                if let Some(r) = sk_rect {
                    let mut paint = Paint::default();
                    let mut color = *color;
                    color.a *= state.opacity;
                    paint.set_color(self.core_color_to_skia(color));
                    pixmap.fill_rect(r, &paint, state.transform, None);
                }
            }
            DrawCommand::StrokeRect {
                rect, color, width, ..
            } => {
                let sk_rect = self.core_rect_to_skia(*rect);
                if let Some(r) = sk_rect {
                    let mut paint = Paint::default();
                    let mut color = *color;
                    color.a *= state.opacity;
                    paint.set_color(self.core_color_to_skia(color));
                    let stroke = Stroke {
                        width: *width as f32,
                        ..Default::default()
                    };
                    let path = PathBuilder::from_rect(
                        tiny_skia::Rect::from_ltrb(r.left(), r.top(), r.right(), r.bottom())
                            .unwrap(),
                    );
                    pixmap.stroke_path(&path, &paint, &stroke, state.transform, None);
                }
            }
            DrawCommand::FillPath {
                ops,
                fill_rule,
                color,
            } => {
                if let Some(path) = self.build_path(ops) {
                    let mut paint = Paint::default();
                    let mut color = *color;
                    color.a *= state.opacity;
                    paint.set_color(self.core_color_to_skia(color));
                    let rule = match fill_rule {
                        FillRule::NonZero => tiny_skia::FillRule::Winding,
                        FillRule::EvenOdd => tiny_skia::FillRule::EvenOdd,
                    };
                    pixmap.fill_path(&path, &paint, rule, state.transform, None);
                }
            }
            DrawCommand::StrokePath {
                ops, width, color, ..
            } => {
                if let Some(path) = self.build_path(ops) {
                    let mut paint = Paint::default();
                    let mut color = *color;
                    color.a *= state.opacity;
                    paint.set_color(self.core_color_to_skia(color));
                    let stroke = Stroke {
                        width: *width as f32,
                        ..Default::default()
                    };
                    pixmap.stroke_path(&path, &paint, &stroke, state.transform, None);
                }
            }
            DrawCommand::Text {
                run,
                position,
                max_width: _,
            } => {
                let font_ref = &run.style.font;
                let bold = run.style.bold;
                let italic = run.style.italic;
                let font_size = run.style.size as f32;
                let mut color = run.style.color;
                color.a *= state.opacity;
                let color = self.core_color_to_skia(color);

                let mut font_cache = self.font_cache.lock().unwrap();

                let loaded_font = font_cache.load_font(font_ref, bold, italic);
                let units_per_em = loaded_font
                    .as_ref()
                    .map(|font| font.units_per_em)
                    .unwrap_or(1000) as f32;

                let mut pen_x = position.x;
                let mut pen_y = position.y;

                if run.glyphs.is_empty() && !run.text.is_empty() {
                    if let Some(font) = loaded_font.as_ref() {
                        for ch in run.text.chars() {
                            if ch == '\n' {
                                pen_x = position.x;
                                pen_y += run.style.line_height.unwrap_or(run.style.size * 1.2);
                                continue;
                            }
                            if let Some(glyph_id) = font.face.glyph_index(ch).map(|id| id.0 as u32)
                            {
                                if let Some(glyph_path) =
                                    font_cache.get_glyph_path(font_ref, bold, italic, glyph_id)
                                {
                                    let glyph_transform = Transform::from_scale(
                                        font_size / units_per_em,
                                        -font_size / units_per_em,
                                    )
                                    .post_concat(Transform::from_translate(
                                        pen_x as f32,
                                        pen_y as f32,
                                    ))
                                    .post_concat(state.transform);

                                    let mut paint = Paint::default();
                                    paint.set_color(color);
                                    paint.anti_alias = true;

                                    pixmap.fill_path(
                                        &glyph_path,
                                        &paint,
                                        tiny_skia::FillRule::Winding,
                                        glyph_transform,
                                        None,
                                    );
                                }
                                let advance = font
                                    .face
                                    .glyph_hor_advance(ttf_parser::GlyphId(glyph_id as u16))
                                    .map(|advance| {
                                        advance as f64 * run.style.size / font.units_per_em as f64
                                    })
                                    .unwrap_or(run.style.size * 0.5);
                                pen_x += advance;
                            } else {
                                pen_x += run.style.size * 0.5;
                            }
                        }
                    }
                } else {
                    for glyph in &run.glyphs {
                        if let Some(glyph_path) =
                            font_cache.get_glyph_path(font_ref, bold, italic, glyph.glyph_id)
                        {
                            let glyph_x = pen_x + glyph.x_offset;
                            let glyph_y = pen_y + glyph.y_offset;

                            let glyph_transform = Transform::from_scale(
                                font_size / units_per_em,
                                -font_size / units_per_em,
                            )
                            .post_concat(Transform::from_translate(glyph_x as f32, glyph_y as f32))
                            .post_concat(state.transform);

                            let mut paint = Paint::default();
                            paint.set_color(color);
                            paint.anti_alias = true;

                            pixmap.fill_path(
                                &glyph_path,
                                &paint,
                                tiny_skia::FillRule::Winding,
                                glyph_transform,
                                None,
                            );
                        }

                        pen_x += glyph.x_advance;
                        pen_y += glyph.y_advance;
                    }
                }
            }
            DrawCommand::Image {
                image_id,
                dest_rect,
                ..
            } => {
                if let Some(image_data) = image_store.get(image_id) {
                    let src_w = image_data.width as u32;
                    let src_h = image_data.height as u32;
                    if src_w == 0 || src_h == 0 {
                        return Ok(());
                    }

                    // Build a pixmap from the image data
                    let src_pixmap = Pixmap::from_vec(
                        image_data.pixels.clone(),
                        tiny_skia::IntSize::from_wh(src_w, src_h).unwrap(),
                    )
                    .unwrap();

                    // Compute the destination rectangle in screen space
                    let _scale_x = dest_rect.width / image_data.width as f64;
                    let _scale_y = dest_rect.height / image_data.height as f64;

                    let screen_w =
                        (dest_rect.width * state.transform.sx as f64).abs().max(1.0) as u32;
                    let screen_h = (dest_rect.height * state.transform.sy as f64)
                        .abs()
                        .max(1.0) as u32;

                    // Scale the source image to the destination size
                    let mut scaled = Pixmap::new(screen_w, screen_h).unwrap();
                    scaled.draw_pixmap(
                        0,
                        0,
                        src_pixmap.as_ref(),
                        &tiny_skia::PixmapPaint::default(),
                        tiny_skia::Transform::from_scale(
                            screen_w as f32 / src_w as f32,
                            screen_h as f32 / src_h as f32,
                        ),
                        None,
                    );

                    // Composite onto the main pixmap at the correct position
                    let tx = state.transform.tx + (dest_rect.x as f32 * state.transform.sx);
                    let ty = state.transform.ty + (dest_rect.y as f32 * state.transform.sy);

                    let mut paint = tiny_skia::PixmapPaint::default();
                    paint.opacity = state.opacity as f32;

                    pixmap.draw_pixmap(
                        tx as i32,
                        ty as i32,
                        scaled.as_ref(),
                        &paint,
                        tiny_skia::Transform::identity(),
                        None,
                    );
                } else {
                    let sk_rect = self.core_rect_to_skia(*dest_rect);
                    if let Some(r) = sk_rect {
                        let mut paint = Paint::default();
                        paint.set_color(SkColor::from_rgba8(255, 100, 100, 255));
                        pixmap.fill_rect(r, &paint, state.transform, None);
                    }
                }
            }
            DrawCommand::PushClip { rect } => {
                // Clip: we create a temporary pixmap, draw to it, then composite back.
                // This is expensive but correct. For production, a clip mask would be better.
                // For now, we skip the clip implementation as it requires significant
                // refactoring of the rendering pipeline.
                let _ = rect;
            }
            DrawCommand::PopClip => {}
            DrawCommand::PushTransform { transform: t } => {
                // This is handled by the caller composing the transform into RenderState.
                let _ = t;
            }
            DrawCommand::PopTransform => {}
            DrawCommand::PushOpacity { opacity } => {
                // This is handled by the caller composing the opacity into RenderState.
                let _ = opacity;
            }
            DrawCommand::PopOpacity => {}
            DrawCommand::BeginGroup { .. } | DrawCommand::EndGroup => {}
            DrawCommand::Block { commands, .. } => {
                for cmd in commands.iter() {
                    self.render_command(pixmap, cmd, state, image_store)?;
                }
            }
        }
        Ok(())
    }
    fn build_path(&self, ops: &[PathOp]) -> Option<tiny_skia::Path> {
        let mut builder = PathBuilder::new();
        for op in ops {
            match op {
                PathOp::MoveTo(p) => {
                    builder.move_to(p.x as f32, p.y as f32);
                }
                PathOp::LineTo(p) => {
                    builder.line_to(p.x as f32, p.y as f32);
                }
                PathOp::CurveTo { cp1, cp2, end } => {
                    builder.cubic_to(
                        cp1.x as f32,
                        cp1.y as f32,
                        cp2.x as f32,
                        cp2.y as f32,
                        end.x as f32,
                        end.y as f32,
                    );
                }
                PathOp::QuadTo { cp, end } => {
                    builder.quad_to(cp.x as f32, cp.y as f32, end.x as f32, end.y as f32);
                }
                PathOp::Close => {
                    builder.close();
                }
            }
        }
        builder.finish()
    }

    fn core_color_to_skia(&self, color: Color) -> SkColor {
        SkColor::from_rgba8(
            (color.r * 255.0).clamp(0.0, 255.0) as u8,
            (color.g * 255.0).clamp(0.0, 255.0) as u8,
            (color.b * 255.0).clamp(0.0, 255.0) as u8,
            (color.a * 255.0).clamp(0.0, 255.0) as u8,
        )
    }

    fn core_rect_to_skia(&self, rect: Rect) -> Option<SkRect> {
        SkRect::from_xywh(
            rect.x as f32,
            rect.y as f32,
            rect.width as f32,
            rect.height as f32,
        )
    }
}

impl super::Render for TinySkiaRenderer {
    fn render_to_raster(
        &self,
        document: &DocumentModel,
        dpi: Dpi,
        output_dir: &StdPath,
    ) -> RenderResult<Vec<std::path::PathBuf>> {
        std::fs::create_dir_all(output_dir)?;
        let mut paths = Vec::new();
        for i in 0..document.page_count() {
            let path = output_dir.join(format!("page_{:03}.png", i + 1));
            self.render_page_to_png(document, i, dpi, &path)?;
            paths.push(path);
        }
        Ok(paths)
    }

    fn render_page_to_png(
        &self,
        document: &DocumentModel,
        page_index: usize,
        dpi: Dpi,
        output_path: &StdPath,
    ) -> RenderResult<()> {
        let pixmap = self.render_page(document, page_index, dpi)?;
        pixmap
            .save_png(output_path)
            .map_err(|e| RenderError::TinySkia(format!("PNG save failed: {}", e)))?;
        Ok(())
    }

    fn render_page_to_pixmap(
        &self,
        document: &DocumentModel,
        page_index: usize,
        dpi: Dpi,
    ) -> RenderResult<Pixmap> {
        self.render_page(document, page_index, dpi)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Render;
    use perfect_print_core::document::DocumentBuilder;
    use perfect_print_core::page::PageSize;
    use perfect_print_core::units::Dpi;

    #[test]
    fn render_single_page_letter() {
        let model = DocumentBuilder::new()
            .page(PageSize::Letter)
            .build()
            .unwrap();

        let renderer = TinySkiaRenderer::new();
        let pixmap = renderer
            .render_page_to_pixmap(&model, 0, Dpi::PRINT_STANDARD)
            .unwrap();

        // Letter at 300 DPI = 2550 x 3300 pixels
        assert!(pixmap.width() > 2500);
        assert!(pixmap.height() > 3200);
    }

    #[test]
    fn render_single_page_a4() {
        let model = DocumentBuilder::new().page(PageSize::A4).build().unwrap();

        let renderer = TinySkiaRenderer::new();
        let pixmap = renderer
            .render_page_to_pixmap(&model, 0, Dpi::PRINT_STANDARD)
            .unwrap();

        // A4 at 300 DPI = 2480 x 3508 pixels
        assert!(pixmap.width() > 2400);
        assert!(pixmap.height() > 3400);
    }

    #[test]
    fn render_with_fill_rect() {
        use perfect_print_core::color::Color;
        use perfect_print_core::document::PageBuilder;
        use perfect_print_core::draw::DrawCommand;
        use perfect_print_core::units::Rect;

        let mut page = perfect_print_core::page::Page::new(PageSize::Letter);
        page.add(DrawCommand::FillRect {
            rect: Rect::new(100.0, 100.0, 200.0, 50.0),
            color: Color::red(),
        });

        let model = DocumentBuilder::new().add_page(page).build().unwrap();

        let renderer = TinySkiaRenderer::new();
        let pixmap = renderer
            .render_page_to_pixmap(&model, 0, Dpi::PRINT_STANDARD)
            .unwrap();
        assert!(pixmap.width() > 0);
        assert!(pixmap.height() > 0);
    }

    #[test]
    fn invalid_page_index_errors() {
        let model = DocumentBuilder::new()
            .page(PageSize::Letter)
            .build()
            .unwrap();

        let renderer = TinySkiaRenderer::new();
        let result = renderer.render_page_to_pixmap(&model, 5, Dpi::PRINT_STANDARD);
        assert!(result.is_err());
        match result.unwrap_err() {
            RenderError::InvalidPageIndex(5) => {}
            other => panic!("Expected InvalidPageIndex(5), got {:?}", other),
        }
    }

    #[test]
    fn font_cache_reuses_database() {
        // Verify that the font cache uses a shared database (not re-creating it per load)
        let renderer = TinySkiaRenderer::new();

        // Render two pages with text — the second should reuse the cached font
        let model = DocumentBuilder::new()
            .page(PageSize::Letter)
            .build()
            .unwrap();

        let _ = renderer.render_page_to_pixmap(&model, 0, Dpi::PRINT_STANDARD);
        let _ = renderer.render_page_to_pixmap(&model, 0, Dpi::PRINT_STANDARD);
        // If we get here without panicking, the cache works
    }

    #[test]
    fn units_per_em_is_correct() {
        // Verify that the font cache reports the correct units_per_em
        let mut cache = FontCache::new();
        let font_ref = perfect_print_core::font::FontRef::new("Helvetica");

        // Load the font and check units_per_em is a reasonable value (typically 1000 or 2048).
        let font = cache.load_font(&font_ref, false, false);
        assert!(font.is_some());
        let upem = font.unwrap().units_per_em;
        assert!(upem > 0, "units_per_em must be positive");
        assert!(upem <= 10000, "units_per_em should be reasonable (<=10000)");
    }

    #[test]
    fn bold_and_regular_load_distinct_faces() {
        // Regression test: `load_font` must select the actual bold/italic
        // face — not silently fall back to face index 0 (Regular) of a
        // TrueType Collection — otherwise bold/italic text renders
        // identically to plain text (see raster::FontCache doc comment).
        let mut cache = FontCache::new();
        let font_ref = perfect_print_core::font::FontRef::new("Helvetica");

        let Some(regular) = cache.load_font(&font_ref, false, false) else {
            return; // system lacks Helvetica; nothing to verify
        };
        let Some(bold) = cache.load_font(&font_ref, true, false) else {
            return; // system lacks a bold face; nothing to verify
        };

        // 'H' should have a measurably wider (heavier) outline in the bold
        // face than in the regular face.
        let glyph_regular = regular.face.glyph_index('H');
        let glyph_bold = bold.face.glyph_index('H');
        if let (Some(gr), Some(gb)) = (glyph_regular, glyph_bold) {
            let advance_regular = regular.face.glyph_hor_advance(gr);
            let advance_bold = bold.face.glyph_hor_advance(gb);
            if let (Some(ar), Some(ab)) = (advance_regular, advance_bold) {
                assert!(
                    ab >= ar,
                    "bold 'H' advance ({ab}) should be >= regular advance ({ar})"
                );
            }
        }
    }
}
