//! PDF output from the canonical page model

use perfect_print_core::color::Color;
use perfect_print_core::document::DocumentModel;
use perfect_print_core::draw::{DrawCommand, FillRule, PathOp};
use perfect_print_core::page::LayerType;
use std::path::Path;
use thiserror::Error;

/// Error type for PDF operations.
#[derive(Debug, Error)]
pub enum PdfError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("PDF generation error: {0}")]
    Generation(String),

    #[error("Invalid page index: {0}")]
    InvalidPageIndex(usize),

    #[error("Font error: {0}")]
    Font(String),

    #[error("Image error: {0}")]
    Image(String),
}

pub type PdfResult<T> = Result<T, PdfError>;

/// PDF renderer that converts the canonical page model to PDF bytes.
pub struct PdfRenderer;

impl Default for PdfRenderer {
    fn default() -> Self {
        Self::new()
    }
}

impl PdfRenderer {
    pub fn new() -> Self {
        Self
    }

    /// Render the document to a PDF file.
    pub fn render_to_pdf(&self, document: &DocumentModel, output_path: &Path) -> PdfResult<()> {
        let mut pdf = self.build_pdf(document)?;
        pdf.save(output_path)
            .map_err(|e| PdfError::Generation(format!("PDF save failed: {}", e)))?;
        Ok(())
    }

    /// Render the document directly to PDF bytes.
    ///
    /// This avoids shared temporary files and is the preferred API for native
    /// print dialogs, webview bridges, uploads, and concurrent render jobs.
    pub fn render_to_bytes(&self, document: &DocumentModel) -> PdfResult<Vec<u8>> {
        let mut pdf = self.build_pdf(document)?;
        let mut bytes = Vec::new();
        pdf.save_to(&mut bytes)
            .map_err(|e| PdfError::Generation(format!("PDF serialization failed: {}", e)))?;
        Ok(bytes)
    }

    fn build_pdf(&self, document: &DocumentModel) -> PdfResult<lopdf::Document> {
        use lopdf::{Document as PdfDocument, Object};

        let mut pdf = PdfDocument::with_version("1.4");

        // Set up font loader for embedding
        let font_loader = perfect_print_layout::font_loader::SystemFontLoader::new();

        // Collect all unique font names used in the document
        let mut font_names: Vec<String> = Vec::new();
        for cmd in document.all_commands() {
            if let DrawCommand::Text { run, .. } = cmd {
                let name = run.style.font.as_ref().to_string();
                if !font_names.contains(&name) {
                    font_names.push(name);
                }
            }
        }
        // Default to Helvetica if no text found
        if font_names.is_empty() {
            font_names.push("Helvetica".to_string());
        }

        // Embed each font: (font_name, font_object_id)
        let mut embedded_fonts: Vec<(String, lopdf::ObjectId)> = Vec::new();
        for font_name in &font_names {
            let mut font_embedded = false;
            if let Some((font_data, _)) = font_loader.get_font_data(font_name) {
                let (descriptor_id, _) = embed_truetype_font(&mut pdf, &font_data, font_name);
                let font_obj = pdf_embedded_font(font_name, descriptor_id);
                let font_id = pdf.add_object(font_obj);
                embedded_fonts.push((font_name.clone(), font_id));
                font_embedded = true;
            }
            if !font_embedded {
                // Fallback: use standard Type1 font (not embedded)
                let font_id = pdf.add_object(pdf_font_descriptor(font_name));
                embedded_fonts.push((font_name.clone(), font_id));
            }
        }

        // Reserve the page tree object before pages so every Page can point back
        // at its Parent. Some PDF consumers are forgiving without this; PDFKit
        // and printer preview paths are not.
        let pages_id = pdf.new_object_id();

        // Build pages
        let mut page_ids = Vec::new();

        for page in &document.pages {
            let page_id = self.build_page(
                &mut pdf,
                page,
                pages_id,
                &embedded_fonts,
                &document.image_store,
            )?;
            page_ids.push(page_id);
        }

        // Build page tree
        let mut pages_dict = lopdf::Dictionary::new();
        pages_dict.set("Type", "Pages");
        pages_dict.set("Count", page_ids.len() as i64);
        let kids: Vec<Object> = page_ids.iter().map(|id| Object::Reference(*id)).collect();
        pages_dict.set("Kids", kids);
        pdf.set_object(pages_id, Object::Dictionary(pages_dict));

        // Build catalog
        let catalog_id = pdf.new_object_id();
        let mut catalog_dict = lopdf::Dictionary::new();
        catalog_dict.set("Type", "Catalog");
        catalog_dict.set("Pages", Object::Reference(pages_id));
        pdf.set_object(catalog_id, Object::Dictionary(catalog_dict));

        pdf.trailer.set("Root", Object::Reference(catalog_id));

        // Set document info
        let mut info_id = None;
        if let Some(ref title) = document.metadata.title {
            let id = pdf.new_object_id();
            let mut info_dict = lopdf::Dictionary::new();
            info_dict.set("Title", title.as_str());
            info_dict.set("Creator", "perfect-print 0.1.0");
            pdf.set_object(id, Object::Dictionary(info_dict));
            info_id = Some(id);
        }
        if let Some(id) = info_id {
            pdf.trailer.set("Info", Object::Reference(id));
        }

        Ok(pdf)
    }

    fn build_page(
        &self,
        pdf: &mut lopdf::Document,
        page: &perfect_print_core::page::Page,
        parent_id: lopdf::ObjectId,
        embedded_fonts: &[(String, lopdf::ObjectId)],
        image_store: &perfect_print_core::resource::ImageStore,
    ) -> PdfResult<lopdf::ObjectId> {
        use lopdf::{Dictionary, Object, Stream};

        let width = page.size.width;
        let height = page.size.height;

        // Build content stream
        let mut content = String::new();

        // Track image XObjects added to this page: (image_id, xobj_id)
        let mut xobject_names: Vec<(String, lopdf::ObjectId)> = Vec::new();

        // Render layers in order
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
                // For Image commands, embed the XObject first
                if let DrawCommand::Image { image_id, .. } = cmd {
                    if image_store.has(image_id)
                        && !xobject_names.iter().any(|(name, _)| name == image_id)
                    {
                        if let Some(img_data) = image_store.get(image_id) {
                            match self.embed_image_xobject(pdf, &img_data) {
                                Ok(xobj_id) => {
                                    xobject_names.push((image_id.clone(), xobj_id));
                                }
                                Err(e) => {
                                    log::warn!("Failed to embed image '{}': {}", image_id, e);
                                }
                            }
                        }
                    }
                }
                self.render_command(&mut content, cmd, height, &xobject_names, embedded_fonts)?;
            }
        }

        // Create content stream
        let content_bytes = content.into_bytes();
        let content_stream = Stream::new(Dictionary::new(), content_bytes);
        let content_id = pdf.add_object(Object::Stream(content_stream));

        // Create page dictionary
        let page_id = pdf.new_object_id();
        let mut page_dict = Dictionary::new();
        page_dict.set("Type", "Page");
        page_dict.set("Parent", Object::Reference(parent_id));
        page_dict.set(
            "MediaBox",
            vec![
                Object::Real(0.0_f32),
                Object::Real(0.0_f32),
                Object::Real(width as f32),
                Object::Real(height as f32),
            ],
        );
        page_dict.set("Contents", Object::Reference(content_id));

        // Build resources with fonts and xobjects
        let mut res = Dictionary::new();
        let mut fonts = Dictionary::new();
        for (i, (_font_name, font_id)) in embedded_fonts.iter().enumerate() {
            let font_key = format!("F{}", i + 1);
            fonts.set(font_key.as_str(), Object::Reference(*font_id));
        }
        // Ensure at least F1 exists
        if embedded_fonts.is_empty() {
            fonts.set("F1", Object::Reference(pdf.new_object_id()));
        }
        res.set("Font", Object::Dictionary(fonts));

        if !xobject_names.is_empty() {
            let mut xobjects = Dictionary::new();
            for (name, xobj_id) in &xobject_names {
                xobjects.set(name.as_str(), Object::Reference(*xobj_id));
            }
            res.set("XObject", Object::Dictionary(xobjects));
        }

        page_dict.set("Resources", Object::Dictionary(res));

        pdf.set_object(page_id, Object::Dictionary(page_dict));
        Ok(page_id)
    }

    /// Embed an image as a PDF XObject (Image XObject with FlateDecode).
    fn embed_image_xobject(
        &self,
        pdf: &mut lopdf::Document,
        image_data: &perfect_print_core::image::ImageData,
    ) -> PdfResult<lopdf::ObjectId> {
        use lopdf::{Dictionary, Object, Stream};

        let w = image_data.width as i64;
        let h = image_data.height as i64;

        // Convert RGBA to RGB
        let mut rgb_data = Vec::with_capacity((w * h * 3) as usize);
        for chunk in image_data.pixels.chunks_exact(4) {
            rgb_data.extend_from_slice(&chunk[0..3]);
        }

        // Compress with flate2 (zlib)
        let mut encoder =
            flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        use std::io::Write;
        encoder
            .write_all(&rgb_data)
            .map_err(|e| PdfError::Generation(format!("Zlib encode failed: {}", e)))?;
        let compressed = encoder
            .finish()
            .map_err(|e| PdfError::Generation(format!("Zlib finish failed: {}", e)))?;

        // Create the image XObject stream
        let mut stream_dict = Dictionary::new();
        stream_dict.set("Type", "XObject");
        stream_dict.set("Subtype", "Image");
        stream_dict.set("Width", w);
        stream_dict.set("Height", h);
        stream_dict.set("ColorSpace", "DeviceRGB");
        stream_dict.set("BitsPerComponent", 8_i64);
        stream_dict.set("Filter", "FlateDecode");

        let stream = Stream::new(stream_dict, compressed);
        let stream_id = pdf.add_object(Object::Stream(stream));

        Ok(stream_id)
    }

    fn render_command(
        &self,
        content: &mut String,
        cmd: &DrawCommand,
        page_height: f64,
        xobject_names: &[(String, lopdf::ObjectId)],
        embedded_fonts: &[(String, lopdf::ObjectId)],
    ) -> PdfResult<()> {
        match cmd {
            DrawCommand::FillRect { rect, color } => {
                let y = page_height - rect.y - rect.height;
                content.push_str(&format_fill_color(color));
                content.push_str(&format!(
                    "{} {} {} {} re f\n",
                    rect.x, y, rect.width, rect.height,
                ));
            }
            DrawCommand::StrokeRect {
                rect, color, width, ..
            } => {
                let y = page_height - rect.y - rect.height;
                content.push_str(&format!("{} w\n", width));
                content.push_str(&format_stroke_color(color));
                content.push_str(&format!(
                    "{} {} {} {} re S\n",
                    rect.x, y, rect.width, rect.height,
                ));
            }
            DrawCommand::FillPath {
                ops,
                fill_rule,
                color,
            } => {
                self.build_pdf_path(content, ops, page_height)?;
                content.push_str(&format_fill_color(color));
                match fill_rule {
                    FillRule::NonZero => content.push_str("f\n"),
                    FillRule::EvenOdd => content.push_str("f*\n"),
                }
            }
            DrawCommand::StrokePath {
                ops, width, color, ..
            } => {
                self.build_pdf_path(content, ops, page_height)?;
                content.push_str(&format!("{} w\n", width));
                content.push_str(&format_stroke_color(color));
                content.push_str("S\n");
            }
            DrawCommand::Text { run, position, .. } => {
                let y = page_height - position.y;
                content.push_str("BT\n");

                // Find the font reference for this run's font
                let font_key = find_font_key(run.style.font.as_ref(), embedded_fonts);
                content.push_str(&format!("/{} {} Tf\n", font_key, run.style.size));
                content.push_str(&format!("{} {} Td\n", position.x, y));

                if run.glyphs.is_empty() {
                    let escaped = run
                        .text
                        .replace('\\', "\\\\")
                        .replace('(', "\\(")
                        .replace(')', "\\)");
                    content.push_str(&format!("({}) Tj\n", escaped));
                } else {
                    content.push_str(&build_tj_array(run));
                }

                content.push_str("ET\n");
            }
            DrawCommand::Image {
                image_id,
                dest_rect,
                ..
            } => {
                // Use the pre-embedded XObject
                if let Some((_, _)) = xobject_names.iter().find(|(name, _)| name == image_id) {
                    // PDF uses bottom-left origin, so flip y
                    let y = page_height - dest_rect.y - dest_rect.height;
                    // Use Do operator to draw the XObject
                    content.push_str(&format!(
                        "q\n{} 0 0 {} {} {} cm\n/{} Do\nQ\n",
                        dest_rect.width, dest_rect.height, dest_rect.x, y, image_id
                    ));
                } else {
                    // Fallback: draw a gray rectangle
                    let y = page_height - dest_rect.y - dest_rect.height;
                    content.push_str("0.8 0.8 0.8 rg\n");
                    content.push_str(&format!(
                        "{} {} {} {} re f\n",
                        dest_rect.x, y, dest_rect.width, dest_rect.height,
                    ));
                }
            }
            DrawCommand::PushClip { .. }
            | DrawCommand::PopClip
            | DrawCommand::PushTransform { .. }
            | DrawCommand::PopTransform
            | DrawCommand::PushOpacity { .. }
            | DrawCommand::PopOpacity
            | DrawCommand::BeginGroup { .. }
            | DrawCommand::EndGroup => {}
            DrawCommand::Block { commands, .. } => {
                for cmd in commands.iter() {
                    self.render_command(content, cmd, page_height, xobject_names, embedded_fonts)?;
                }
            }
        }
        Ok(())
    }

    fn build_pdf_path(
        &self,
        content: &mut String,
        ops: &[PathOp],
        page_height: f64,
    ) -> PdfResult<()> {
        for op in ops {
            match op {
                PathOp::MoveTo(p) => {
                    content.push_str(&format!("{} {} m\n", p.x, page_height - p.y));
                }
                PathOp::LineTo(p) => {
                    content.push_str(&format!("{} {} l\n", p.x, page_height - p.y));
                }
                PathOp::CurveTo { cp1, cp2, end } => {
                    content.push_str(&format!(
                        "{} {} {} {} {} {} c\n",
                        cp1.x,
                        page_height - cp1.y,
                        cp2.x,
                        page_height - cp2.y,
                        end.x,
                        page_height - end.y,
                    ));
                }
                PathOp::QuadTo { cp, end } => {
                    // Convert quadratic to cubic for PDF
                    content.push_str(&format!(
                        "{} {} {} {} {} {} c\n",
                        cp.x,
                        page_height - cp.y,
                        cp.x,
                        page_height - cp.y,
                        end.x,
                        page_height - end.y,
                    ));
                }
                PathOp::Close => {
                    content.push_str("h\n");
                }
            }
        }
        Ok(())
    }
}

/// Find the font key (e.g., "F1", "F2") for a given font name in the embedded fonts list.
fn find_font_key(font_name: &str, embedded_fonts: &[(String, lopdf::ObjectId)]) -> String {
    for (i, (name, _)) in embedded_fonts.iter().enumerate() {
        if name == font_name {
            return format!("F{}", i + 1);
        }
    }
    "F1".to_string() // fallback
}

fn format_fill_color(color: &Color) -> String {
    format!("{} {} {} rg\n", color.r, color.g, color.b)
}

fn format_stroke_color(color: &Color) -> String {
    format!("{} {} {} RG\n", color.r, color.g, color.b)
}

fn pdf_font_descriptor(name: &str) -> lopdf::Object {
    use lopdf::{Dictionary, Object};
    let mut dict = Dictionary::new();
    dict.set("Type", "Font");
    dict.set("Subtype", "Type1");
    dict.set("BaseFont", name);
    dict.set("Encoding", "WinAnsiEncoding");
    Object::Dictionary(dict)
}

/// Embed a TrueType font in the PDF document.
/// Returns the font descriptor object ID and the font file stream ID.
fn embed_truetype_font(
    pdf: &mut lopdf::Document,
    font_data: &[u8],
    font_name: &str,
) -> (lopdf::ObjectId, lopdf::ObjectId) {
    use lopdf::{Dictionary, Object, Stream};

    // Parse basic font metrics from the raw TTF data using ttf-parser
    let face = ttf_parser::Face::parse(font_data, 0).ok();
    let ascender = face.as_ref().map(|f| f.ascender() as i64).unwrap_or(800);
    let descender = face.as_ref().map(|f| f.descender() as i64).unwrap_or(-200);
    let cap_height = face
        .as_ref()
        .and_then(|f| f.capital_height())
        .map(|h| h as i64);
    let italic_angle = face.as_ref().map(|f| f.italic_angle()).unwrap_or(0.0);

    // Create FontFile2 stream (raw TrueType data)
    let mut font_file_dict = Dictionary::new();
    font_file_dict.set("Length1", font_data.len() as i64);
    let font_file_stream = Stream::new(font_file_dict, font_data.to_vec());
    let font_file_id = pdf.add_object(Object::Stream(font_file_stream));

    // Create FontDescriptor
    let mut descriptor = Dictionary::new();
    descriptor.set("Type", "FontDescriptor");
    descriptor.set("FontName", font_name);
    descriptor.set("Flags", 32_i64); // Nonsymbolic
    descriptor.set("ItalicAngle", italic_angle);
    descriptor.set("Ascent", ascender);
    descriptor.set("Descent", descender);
    if let Some(ch) = cap_height {
        descriptor.set("CapHeight", ch);
    }
    descriptor.set("StemV", 80_i64);
    descriptor.set("FontFile2", Object::Reference(font_file_id));
    let descriptor_id = pdf.add_object(Object::Dictionary(descriptor));

    (descriptor_id, font_file_id)
}

/// Create a PDF Font dictionary for an embedded TrueType font.
fn pdf_embedded_font(name: &str, descriptor_id: lopdf::ObjectId) -> lopdf::Object {
    use lopdf::{Dictionary, Object};
    let mut dict = Dictionary::new();
    dict.set("Type", "Font");
    dict.set("Subtype", "TrueType");
    dict.set("BaseFont", name);
    dict.set("Encoding", "WinAnsiEncoding");
    dict.set("FontDescriptor", Object::Reference(descriptor_id));
    Object::Dictionary(dict)
}

fn build_tj_array(run: &perfect_print_core::draw::TextRun) -> String {
    use perfect_print_core::draw::ShapedGlyph;

    if run.glyphs.is_empty() {
        return String::new();
    }

    let font_size = run.style.size;
    let scale = 1000.0 / font_size;

    let mut result = String::new();
    result.push('[');

    // Group glyphs by cluster
    let mut cluster_map: std::collections::HashMap<u32, Vec<&ShapedGlyph>> =
        std::collections::HashMap::new();
    for glyph in &run.glyphs {
        cluster_map.entry(glyph.cluster).or_default().push(glyph);
    }

    // Map from cluster (byte index) to the advance of the first glyph in that cluster
    let mut cluster_advances: std::collections::HashMap<u32, f64> =
        std::collections::HashMap::new();
    for (cluster, glyphs) in &cluster_map {
        if let Some(first_glyph) = glyphs.first() {
            cluster_advances.insert(*cluster, first_glyph.x_advance);
        }
    }

    let text = &run.text;
    let char_indices: Vec<(usize, char)> = text.char_indices().collect();

    let mut first = true;
    for (byte_idx, ch) in &char_indices {
        let cluster = *byte_idx as u32;
        let shaped_advance = cluster_advances.get(&cluster).copied().unwrap_or(0.0);

        if !first {
            let adjustment = -(shaped_advance * scale);
            result.push_str(&format!("{} ", adjustment));
        }
        if first {
            first = false;
        }

        let escaped = ch
            .to_string()
            .replace('\\', "\\\\")
            .replace('(', "\\(")
            .replace(')', "\\)");
        result.push_str(&format!("({}) ", escaped));
    }

    result.push_str("] TJ\n");
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use perfect_print_core::color::Color;
    use perfect_print_core::document::DocumentBuilder;
    use perfect_print_core::document::PageBuilder;
    use perfect_print_core::draw::DrawCommand;
    use perfect_print_core::page::PageSize;
    use perfect_print_core::units::Rect;

    #[test]
    fn render_to_bytes_returns_loadable_pdf_without_temp_files() {
        let model = DocumentBuilder::new()
            .page(PageSize::Letter)
            .build()
            .unwrap();

        let bytes = PdfRenderer::new().render_to_bytes(&model).unwrap();

        assert_eq!(&bytes[..5], b"%PDF-");
        let parsed = lopdf::Document::load_mem(&bytes).unwrap();
        assert_eq!(parsed.get_pages().len(), 1);
    }

    #[test]
    fn render_letter_to_pdf() {
        let model = DocumentBuilder::new()
            .page(PageSize::Letter)
            .build()
            .unwrap();

        let renderer = PdfRenderer::new();
        let path = std::env::temp_dir().join("test_letter.pdf");
        renderer.render_to_pdf(&model, &path).unwrap();

        assert!(path.exists());
        let metadata = std::fs::metadata(&path).unwrap();
        assert!(metadata.len() > 0);

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn render_a4_to_pdf() {
        let model = DocumentBuilder::new().page(PageSize::A4).build().unwrap();

        let renderer = PdfRenderer::new();
        let path = std::env::temp_dir().join("test_a4.pdf");
        renderer.render_to_pdf(&model, &path).unwrap();

        assert!(path.exists());
        let metadata = std::fs::metadata(&path).unwrap();
        assert!(metadata.len() > 0);

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn render_content_to_pdf() {
        let mut page = perfect_print_core::page::Page::new(PageSize::Letter);
        page.add(DrawCommand::FillRect {
            rect: Rect::new(100.0, 100.0, 200.0, 50.0),
            color: Color::blue(),
        });

        let model = DocumentBuilder::new().add_page(page).build().unwrap();

        let renderer = PdfRenderer::new();
        let path = std::env::temp_dir().join("test_content.pdf");
        renderer.render_to_pdf(&model, &path).unwrap();

        assert!(path.exists());
        let metadata = std::fs::metadata(&path).unwrap();
        assert!(metadata.len() > 200);

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn render_text_to_pdf() {
        let mut page = perfect_print_core::page::Page::new(PageSize::Letter);
        page.add(DrawCommand::Text {
            run: perfect_print_core::draw::TextRun {
                text: "Hello PDF".to_string(),
                glyphs: vec![],
                style: perfect_print_core::draw::TextStyle::new(
                    perfect_print_core::font::FontRef::new("Helvetica"),
                    24.0,
                ),
            },
            position: perfect_print_core::units::Point::new(72.0, 72.0),
            max_width: None,
        });

        let model = DocumentBuilder::new().add_page(page).build().unwrap();

        // Render to PDF
        let pdf_renderer = PdfRenderer::new();
        let path = std::env::temp_dir().join("test_text.pdf");
        pdf_renderer.render_to_pdf(&model, &path).unwrap();

        assert!(path.exists());
        let metadata = std::fs::metadata(&path).unwrap();
        assert!(metadata.len() > 200);

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn render_multi_page_to_pdf() {
        let model = DocumentBuilder::new()
            .page(PageSize::Letter)
            .page(PageSize::Letter)
            .page(PageSize::A4)
            .build()
            .unwrap();

        let renderer = PdfRenderer::new();
        let path = std::env::temp_dir().join("test_multi.pdf");
        renderer.render_to_pdf(&model, &path).unwrap();

        assert!(path.exists());
        let metadata = std::fs::metadata(&path).unwrap();
        assert!(metadata.len() > 500);

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn render_image_to_pdf() {
        let mut image_store = perfect_print_core::resource::ImageStore::new();
        let img_data = perfect_print_core::image::ImageData::test_pattern(20, 20);
        image_store.insert("test", img_data);

        let mut page = perfect_print_core::page::Page::new(PageSize::Letter);
        page.add(DrawCommand::Image {
            image_id: "test".to_string(),
            dest_rect: Rect::new(100.0, 100.0, 50.0, 50.0),
            source_rect: None,
        });

        let mut model = DocumentBuilder::new().add_page(page).build().unwrap();
        model.image_store = image_store;

        let renderer = PdfRenderer::new();
        let path = std::env::temp_dir().join("test_image.pdf");
        renderer.render_to_pdf(&model, &path).unwrap();

        assert!(path.exists());
        let metadata = std::fs::metadata(&path).unwrap();
        assert!(metadata.len() > 500);

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn rendered_pdf_is_loadable_by_pdf_consumers() {
        let mut image_store = perfect_print_core::resource::ImageStore::new();
        let img_data = perfect_print_core::image::ImageData::test_pattern(20, 20);
        image_store.insert("test", img_data);

        let mut page = perfect_print_core::page::Page::new(PageSize::Letter);
        page.add(DrawCommand::Image {
            image_id: "test".to_string(),
            dest_rect: Rect::new(
                0.0,
                0.0,
                PageSize::Letter.width(),
                PageSize::Letter.height(),
            ),
            source_rect: None,
        });

        let mut model = DocumentBuilder::new()
            .title("Loadable PDF")
            .add_page(page)
            .build()
            .unwrap();
        model.image_store = image_store;

        let renderer = PdfRenderer::new();
        let path = std::env::temp_dir().join("test_loadable_pdf.pdf");
        renderer.render_to_pdf(&model, &path).unwrap();

        let loaded = lopdf::Document::load(&path).expect("rendered PDF should load");
        assert!(
            loaded.trailer.get(b"Root").is_ok(),
            "PDF trailer should contain Root for PDFKit/Preview"
        );
        assert_eq!(loaded.get_pages().len(), 1);

        let pdf_bytes = std::fs::read(&path).unwrap();
        assert!(pdf_bytes.starts_with(b"%PDF-1.4"));
        assert!(String::from_utf8_lossy(&pdf_bytes).contains("/Parent"));

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn pdf_has_embedded_font() {
        // Create a document with text
        let mut page = perfect_print_core::page::Page::new(PageSize::Letter);
        page.add(DrawCommand::Text {
            run: perfect_print_core::draw::TextRun {
                text: "Font Test".to_string(),
                glyphs: vec![],
                style: perfect_print_core::draw::TextStyle::new(
                    perfect_print_core::font::FontRef::new("Helvetica"),
                    12.0,
                ),
            },
            position: perfect_print_core::units::Point::new(72.0, 72.0),
            max_width: None,
        });

        let model = DocumentBuilder::new().add_page(page).build().unwrap();

        let renderer = PdfRenderer::new();
        let path = std::env::temp_dir().join("test_embedded_font.pdf");
        renderer.render_to_pdf(&model, &path).unwrap();

        let pdf_bytes = std::fs::read(&path).unwrap();
        let pdf_str = String::from_utf8_lossy(&pdf_bytes);

        // Check for FontFile2 marker (embedded TrueType font)
        assert!(
            pdf_str.contains("FontFile2"),
            "PDF should contain FontFile2 for embedded font. First 500 chars: {}",
            &pdf_str[..500.min(pdf_str.len())]
        );

        // Check for FontDescriptor
        assert!(
            pdf_str.contains("FontDescriptor"),
            "PDF should contain FontDescriptor"
        );

        // Check for TrueType subtype
        assert!(
            pdf_str.contains("TrueType"),
            "PDF should contain TrueType subtype for embedded font"
        );

        let _ = std::fs::remove_file(&path);
    }
}
