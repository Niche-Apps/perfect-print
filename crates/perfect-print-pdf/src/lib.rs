//! PDF output from the canonical page model

use perfect_print_core::color::Color;
use perfect_print_core::document::DocumentModel;
use perfect_print_core::draw::{DrawCommand, FillRule, PathOp};
use perfect_print_core::page::LayerType;
use std::path::Path;
use thiserror::Error;

mod sfnt;

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

        // Collect all unique (family, bold, italic) combinations used in the
        // document. Bold/italic runs must embed the actual bold/italic face
        // — not just the regular face under a different label — because the
        // glyph IDs baked into each run's `ShapedGlyph`s were produced by
        // shaping against that specific face (see
        // `paragraph::font_properties_for_style`); embedding the wrong face
        // would map those glyph IDs to the wrong outlines.
        let mut font_keys: Vec<PdfFontKey> = Vec::new();
        for cmd in document.all_commands() {
            if let DrawCommand::Text { run, .. } = cmd {
                let key = PdfFontKey::from_style(&run.style);
                if !font_keys.contains(&key) {
                    font_keys.push(key);
                }
            }
        }
        // Default to Helvetica if no text found
        if font_keys.is_empty() {
            font_keys.push(PdfFontKey::from_style(
                &perfect_print_core::draw::TextStyle::new(
                    perfect_print_core::font::FontRef::new("Helvetica"),
                    12.0,
                ),
            ));
        }

        // Embed each font.
        let mut embedded_fonts: Vec<EmbeddedFont> = Vec::new();
        for key in &font_keys {
            let mut font_embedded = false;
            if let Some((font_data, face_index)) = font_loader.get_font_data_for(&key.to_properties())
            {
                let (descriptor_id, _, first_char, last_char, widths) = embed_truetype_font(
                    &mut pdf,
                    &font_data,
                    face_index,
                    &key.base_font_name(),
                );
                let font_obj = pdf_embedded_font(
                    &key.base_font_name(),
                    descriptor_id,
                    first_char,
                    last_char,
                    &widths,
                );
                let font_id = pdf.add_object(font_obj);
                embedded_fonts.push(EmbeddedFont {
                    key: key.clone(),
                    font_id,
                    // Same range as PDF_FONT_FIRST_CHAR/PDF_FONT_LAST_CHAR;
                    // kept alongside the widths so callers building TJ
                    // arrays can look up a code's declared /Widths entry
                    // without recomputing font metrics.
                    first_char,
                    widths: Some(widths),
                });
                font_embedded = true;
            }
            if !font_embedded {
                // Fallback: use standard Type1 font (not embedded). The
                // reader supplies metrics for the 14 standard fonts itself,
                // so there's no /Widths array for us to reason about here.
                let font_id = pdf.add_object(pdf_font_descriptor(&key.base_font_name()));
                embedded_fonts.push(EmbeddedFont {
                    key: key.clone(),
                    font_id,
                    first_char: PDF_FONT_FIRST_CHAR,
                    widths: None,
                });
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
        embedded_fonts: &[EmbeddedFont],
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
        for (i, embedded) in embedded_fonts.iter().enumerate() {
            let font_key = format!("F{}", i + 1);
            fonts.set(font_key.as_str(), Object::Reference(embedded.font_id));
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
        embedded_fonts: &[EmbeddedFont],
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

                // Find the font reference for this run's font, matching
                // family AND bold/italic so the glyph IDs (shaped against a
                // specific face) line up with the embedded face.
                let key = PdfFontKey::from_style(&run.style);
                let (font_key, font_widths) = find_font(&key, embedded_fonts);
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
                    content.push_str(&build_tj_array(run, font_widths));
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

/// Identifies a distinct embedded PDF font resource: family + weight/style.
/// Two text runs with the same family but different bold/italic must embed
/// (and reference) different font resources, since the glyph IDs baked into
/// their `ShapedGlyph`s come from shaping against that specific face.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PdfFontKey {
    family: String,
    bold: bool,
    italic: bool,
}

impl PdfFontKey {
    fn from_style(style: &perfect_print_core::draw::TextStyle) -> Self {
        Self {
            family: style.font.as_ref().to_string(),
            bold: style.bold,
            italic: style.italic,
        }
    }

    fn to_properties(&self) -> perfect_print_layout::font_loader::FontProperties {
        use perfect_print_core::font::{FontStyle, FontWeight};
        perfect_print_layout::font_loader::FontProperties::new(&self.family)
            .with_weight(if self.bold {
                FontWeight::Bold
            } else {
                FontWeight::Normal
            })
            .with_style(if self.italic {
                FontStyle::Italic
            } else {
                FontStyle::Normal
            })
    }

    /// Human-readable BaseFont name for the PDF font dictionary, e.g.
    /// "Helvetica-BoldItalic".
    fn base_font_name(&self) -> String {
        match (self.bold, self.italic) {
            (true, true) => format!("{}-BoldItalic", self.family),
            (true, false) => format!("{}-Bold", self.family),
            (false, true) => format!("{}-Italic", self.family),
            (false, false) => self.family.clone(),
        }
    }
}

/// An embedded (or standard, non-embedded) PDF font resource.
pub struct EmbeddedFont {
    key: PdfFontKey,
    font_id: lopdf::ObjectId,
    /// First code covered by `widths` (matches `PDF_FONT_FIRST_CHAR` for
    /// embedded TrueType fonts). Unused when `widths` is `None`.
    first_char: i64,
    /// The exact `/Widths` values written into this font's dictionary, in
    /// 1000-units-per-em text space, indexed by `code - first_char`. `None`
    /// for the standard-Type1 fallback path, where the reader supplies its
    /// own metrics for the 14 standard fonts and we have no declared
    /// `/Widths` to stay consistent with.
    widths: Option<Vec<i64>>,
}

/// Find the font resource name (e.g., "F1", "F2") and declared `/Widths`
/// (if any) for a given font in the embedded fonts list.
fn find_font<'a>(key: &PdfFontKey, embedded_fonts: &'a [EmbeddedFont]) -> (String, Option<&'a EmbeddedFont>) {
    for (i, candidate) in embedded_fonts.iter().enumerate() {
        if &candidate.key == key {
            return (format!("F{}", i + 1), Some(candidate));
        }
    }
    ("F1".to_string(), None) // fallback
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

/// First and last WinAnsi code emitted in `/FirstChar`/`/LastChar`/`/Widths`.
/// We use the full 32..=255 range for every embedded font rather than
/// computing the exact codes used by each run: it's simpler, always valid
/// per ISO 32000-1 §9.6.2, and the extra entries cost little in a PDF that
/// already embeds a whole font program.
const PDF_FONT_FIRST_CHAR: i64 = 32;
const PDF_FONT_LAST_CHAR: i64 = 255;

/// Map a single-byte WinAnsiEncoding code to the Unicode scalar it
/// represents, or `None` for codes that WinAnsi leaves undefined.
///
/// WinAnsiEncoding matches Windows-1252/CP1252:
/// - 0x20..=0x7E: ASCII, unicode == code.
/// - 0xA0..=0xFF: matches Latin-1, unicode == code.
/// - 0x80..=0x9F: the CP1252 "control block" overrides — NOT Latin-1 (which
///   leaves these as C1 control codes). Codes with no CP1252 mapping
///   (0x81, 0x8D, 0x8F, 0x90, 0x9D) return `None`.
fn winansi_code_to_unicode(code: u8) -> Option<char> {
    match code {
        0x20..=0x7E => Some(code as char),
        0x80 => Some('\u{20AC}'),
        0x81 => None,
        0x82 => Some('\u{201A}'),
        0x83 => Some('\u{0192}'),
        0x84 => Some('\u{201E}'),
        0x85 => Some('\u{2026}'),
        0x86 => Some('\u{2020}'),
        0x87 => Some('\u{2021}'),
        0x88 => Some('\u{02C6}'),
        0x89 => Some('\u{2030}'),
        0x8A => Some('\u{0160}'),
        0x8B => Some('\u{2039}'),
        0x8C => Some('\u{0152}'),
        0x8D => None,
        0x8E => Some('\u{017D}'),
        0x8F => None,
        0x90 => None,
        0x91 => Some('\u{2018}'),
        0x92 => Some('\u{2019}'),
        0x93 => Some('\u{201C}'),
        0x94 => Some('\u{201D}'),
        0x95 => Some('\u{2022}'),
        0x96 => Some('\u{2013}'),
        0x97 => Some('\u{2014}'),
        0x98 => Some('\u{02DC}'),
        0x99 => Some('\u{2122}'),
        0x9A => Some('\u{0161}'),
        0x9B => Some('\u{203A}'),
        0x9C => Some('\u{0153}'),
        0x9D => None,
        0x9E => Some('\u{017E}'),
        0x9F => Some('\u{0178}'),
        0xA0..=0xFF => Some(code as char),
        _ => None,
    }
}

/// Compute the `/Widths` array (in 1000-units-per-em text space) for codes
/// `PDF_FONT_FIRST_CHAR..=PDF_FONT_LAST_CHAR` against the same font face
/// that gets embedded in `/FontFile2`. Codes with no WinAnsi mapping, or
/// whose mapped character has no glyph in the face, get width 0 (rather
/// than the `.notdef` advance — 0 is unambiguous and matches what most
/// consumers already assume when a code is unused).
fn compute_winansi_widths(face: &ttf_parser::Face) -> Vec<i64> {
    let units_per_em = face.units_per_em() as f64;
    let mut widths = Vec::with_capacity((PDF_FONT_LAST_CHAR - PDF_FONT_FIRST_CHAR + 1) as usize);
    for code in PDF_FONT_FIRST_CHAR..=PDF_FONT_LAST_CHAR {
        let width = winansi_code_to_unicode(code as u8)
            .and_then(|ch| face.glyph_index(ch))
            .and_then(|gid| face.glyph_hor_advance(gid))
            .map(|adv| (adv as f64 * 1000.0 / units_per_em).round() as i64)
            .unwrap_or(0);
        widths.push(width);
    }
    widths
}

/// Embed a TrueType font in the PDF document.
/// Returns the font descriptor object ID, the font file stream ID, the
/// FirstChar/LastChar range, and the /Widths array for that range.
fn embed_truetype_font(
    pdf: &mut lopdf::Document,
    font_data: &[u8],
    face_index: u32,
    font_name: &str,
) -> (lopdf::ObjectId, lopdf::ObjectId, i64, i64, Vec<i64>) {
    use lopdf::{Dictionary, Object, Stream};

    // Parse basic font metrics from the raw TTF/TTC data using ttf-parser.
    // `face_index` selects the right face within a TrueType Collection
    // (.ttc) — e.g. a system "Helvetica" bundle where index 0 is Regular,
    // 1 is Bold, etc. Using a fixed index 0 here would report Regular
    // metrics (ascent/descent/italic angle) for every weight/style.
    let face = ttf_parser::Face::parse(font_data, face_index).ok();
    let ascender = face.as_ref().map(|f| f.ascender() as i64).unwrap_or(800);
    let descender = face.as_ref().map(|f| f.descender() as i64).unwrap_or(-200);
    let cap_height = face
        .as_ref()
        .and_then(|f| f.capital_height())
        .map(|h| h as i64);
    let italic_angle = face.as_ref().map(|f| f.italic_angle()).unwrap_or(0.0);
    // /Widths must come from the exact same face bytes/index used for
    // `/FontFile2` below, or glyph metrics won't match the embedded outlines.
    let widths = face
        .as_ref()
        .map(compute_winansi_widths)
        .unwrap_or_else(|| vec![0; (PDF_FONT_LAST_CHAR - PDF_FONT_FIRST_CHAR + 1) as usize]);

    // PDF `/FontFile2` must contain a single sfnt font program, not a
    // `ttcf` TrueType Collection. On macOS most system fonts (e.g.
    // Helvetica) resolve to a `.ttc` with `face_index` selecting the
    // desired weight/style; embedding the raw collection bytes would embed
    // every face in the family and produce a stream strict PDF viewers
    // can't render text from. Extract just the referenced face; fall back
    // to the original bytes (previous, buggy-but-safe behavior) if
    // extraction fails for any reason.
    let embedded_bytes: std::borrow::Cow<[u8]> = if sfnt::is_ttc(font_data) {
        match sfnt::extract_ttc_face(font_data, face_index) {
            Some(extracted) => std::borrow::Cow::Owned(extracted),
            None => {
                log::warn!(
                    "Failed to extract face {} from TrueType Collection for font '{}'; \
                     embedding the full collection instead (bloated PDF)",
                    face_index,
                    font_name
                );
                std::borrow::Cow::Borrowed(font_data)
            }
        }
    } else {
        std::borrow::Cow::Borrowed(font_data)
    };

    // Create FontFile2 stream (single-face TrueType data)
    let mut font_file_dict = Dictionary::new();
    font_file_dict.set("Length1", embedded_bytes.len() as i64);
    let font_file_stream = Stream::new(font_file_dict, embedded_bytes.into_owned());
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
    // Advance for codes outside FirstChar..=LastChar (there are none, since
    // we cover the full 32..=255 range, but PDF readers may still consult
    // this for .notdef fallback behavior).
    descriptor.set("MissingWidth", 0_i64);
    descriptor.set("FontFile2", Object::Reference(font_file_id));
    let descriptor_id = pdf.add_object(Object::Dictionary(descriptor));

    (
        descriptor_id,
        font_file_id,
        PDF_FONT_FIRST_CHAR,
        PDF_FONT_LAST_CHAR,
        widths,
    )
}

/// Create a PDF Font dictionary for an embedded TrueType font.
///
/// Per ISO 32000-1 §9.6.2, simple TrueType font dictionaries MUST include
/// `/FirstChar`, `/LastChar`, and `/Widths`. Omitting them is spec-invalid:
/// CoreGraphics logs "missing or invalid 'FirstChar' entry" and falls back
/// to guessed glyph widths (wrong letter spacing), and strict CUPS/driver
/// PDF->PostScript filters may drop the text run entirely.
fn pdf_embedded_font(
    name: &str,
    descriptor_id: lopdf::ObjectId,
    first_char: i64,
    last_char: i64,
    widths: &[i64],
) -> lopdf::Object {
    use lopdf::{Dictionary, Object};
    let mut dict = Dictionary::new();
    dict.set("Type", "Font");
    dict.set("Subtype", "TrueType");
    dict.set("BaseFont", name);
    dict.set("Encoding", "WinAnsiEncoding");
    dict.set("FirstChar", first_char);
    dict.set("LastChar", last_char);
    let widths_array: Vec<Object> = widths.iter().map(|w| Object::Integer(*w)).collect();
    dict.set("Widths", Object::Array(widths_array));
    dict.set("FontDescriptor", Object::Reference(descriptor_id));
    Object::Dictionary(dict)
}

/// Reverse of `winansi_code_to_unicode`: map a Unicode scalar back to its
/// single-byte WinAnsi code, if representable. `None` for anything outside
/// WinAnsi's repertoire (e.g. non-Latin scripts) — those runs have no
/// meaningful `/Widths` entry to reconcile against anyway, since they can't
/// be represented under `/Encoding/WinAnsiEncoding` in the first place.
fn char_to_winansi_code(ch: char) -> Option<u8> {
    match ch {
        ' '..='~' => Some(ch as u8),
        '\u{20AC}' => Some(0x80),
        '\u{201A}' => Some(0x82),
        '\u{0192}' => Some(0x83),
        '\u{201E}' => Some(0x84),
        '\u{2026}' => Some(0x85),
        '\u{2020}' => Some(0x86),
        '\u{2021}' => Some(0x87),
        '\u{02C6}' => Some(0x88),
        '\u{2030}' => Some(0x89),
        '\u{0160}' => Some(0x8A),
        '\u{2039}' => Some(0x8B),
        '\u{0152}' => Some(0x8C),
        '\u{017D}' => Some(0x8E),
        '\u{2018}' => Some(0x91),
        '\u{2019}' => Some(0x92),
        '\u{201C}' => Some(0x93),
        '\u{201D}' => Some(0x94),
        '\u{2022}' => Some(0x95),
        '\u{2013}' => Some(0x96),
        '\u{2014}' => Some(0x97),
        '\u{02DC}' => Some(0x98),
        '\u{2122}' => Some(0x99),
        '\u{0161}' => Some(0x9A),
        '\u{203A}' => Some(0x9B),
        '\u{0153}' => Some(0x9C),
        '\u{017E}' => Some(0x9E),
        '\u{0178}' => Some(0x9F),
        '\u{00A0}'..='\u{00FF}' => Some(ch as u8),
        _ => None,
    }
}

/// Look up a character's declared `/Widths` advance (1000-units-per-em) in
/// the given font, or 0 if the font has no declared widths (standard Type1
/// fallback) or the character has no WinAnsi code.
fn declared_width_1000(font: Option<&EmbeddedFont>, ch: char) -> f64 {
    let Some(font) = font else { return 0.0 };
    let Some(widths) = font.widths.as_ref() else {
        return 0.0;
    };
    let Some(code) = char_to_winansi_code(ch) else {
        return 0.0;
    };
    let index = code as i64 - font.first_char;
    if index < 0 {
        return 0.0;
    }
    widths.get(index as usize).copied().unwrap_or(0) as f64
}

/// Build a PDF `TJ` array that places each glyph exactly at the shaper's
/// computed advance (`ShapedGlyph::x_advance`, already in points at the
/// run's font size).
///
/// A `TJ` string element is *not* an absolute advance: the reader first
/// moves the pen by the glyph's own `/Widths` advance (declared in the font
/// dictionary) and only then applies the adjustment number between
/// elements, which is *subtracted* (in thousandths of text space) from that
/// movement. So to make the reader land exactly at `shaped_advance` we must
/// account for whatever the font's `/Widths` entry already contributes:
///
///   adjustment = declared_width_1000 - shaped_advance * (1000 / font_size)
///
/// Before the font dictionaries carried a `/Widths` array (see
/// `pdf_embedded_font`), this term didn't exist in the PDF's data model at
/// all — but conforming readers still need *some* value for the "declared"
/// advance when none is given, and both CoreGraphics and Poppler fall back
/// to the embedded font program's own `hmtx` advances (the same source
/// `compute_winansi_widths` reads from) rather than 0. So `declared_width`
/// was already implicitly non-zero pre-fix; omitting it here from the
/// adjustment math double-counted every glyph's advance and produced the
/// "I N V OI C E"-style spacing this function now corrects.
fn build_tj_array(run: &perfect_print_core::draw::TextRun, font: Option<&EmbeddedFont>) -> String {
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

    // The adjustment number sitting between string i-1 and string i in a TJ
    // array applies to the pen movement *after* string i-1 is drawn: the
    // reader advances by string i-1's own glyph's declared /Widths advance,
    // then subtracts this adjustment, and only then draws string i. So the
    // adjustment before string i must be derived from character i-1's
    // shaped advance (what we *want* that movement to be) and character
    // i-1's declared width (what the reader will apply by default) — not
    // character i's. Track the previous character's shaped advance as we
    // walk the run.
    let mut prev: Option<(char, f64)> = None;
    for (byte_idx, ch) in &char_indices {
        let cluster = *byte_idx as u32;
        // Some characters in `run.text` may have no corresponding shaped
        // glyph (e.g. a wrapped continuation the shaper never ran on). For
        // those, fall back to the font's own declared advance rather than
        // 0 — that makes the adjustment before/after this character
        // resolve to ~0, leaving the reader's default (already-correct)
        // font-metrics-based spacing untouched instead of corrupting it.
        let shaped_advance = cluster_advances
            .get(&cluster)
            .copied()
            .unwrap_or_else(|| declared_width_1000(font, *ch) / scale);

        if let Some((prev_ch, prev_shaped_advance)) = prev {
            let declared = declared_width_1000(font, prev_ch);
            let adjustment = declared - (prev_shaped_advance * scale);
            result.push_str(&format!("{} ", adjustment));
        }
        prev = Some((*ch, shaped_advance));

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

    #[test]
    fn pdf_font_dict_has_valid_widths_array() {
        // Regression test for spec-invalid TrueType font dictionaries:
        // ISO 32000-1 §9.6.2 requires /FirstChar, /LastChar, and /Widths on
        // simple TrueType fonts. Without them, CoreGraphics logs
        // "missing or invalid 'FirstChar' entry" and guesses glyph widths
        // (visibly wrong letter spacing), and strict CUPS/driver PDF->PS
        // filters drop the text run entirely, producing blank printed pages.
        let mut page = perfect_print_core::page::Page::new(PageSize::Letter);
        page.add(DrawCommand::Text {
            run: perfect_print_core::draw::TextRun {
                text: "Hi".to_string(),
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
        let bytes = renderer.render_to_bytes(&model).unwrap();

        let pdf = lopdf::Document::load_mem(&bytes).unwrap();

        // Find the embedded TrueType font dictionary.
        let mut found_font = false;
        for (_, obj) in pdf.objects.iter() {
            if let lopdf::Object::Dictionary(dict) = obj {
                let is_truetype_font = dict
                    .get(b"Type")
                    .and_then(|o| o.as_name())
                    .map(|n| n == b"Font")
                    .unwrap_or(false)
                    && dict
                        .get(b"Subtype")
                        .and_then(|o| o.as_name())
                        .map(|n| n == b"TrueType")
                        .unwrap_or(false);
                if !is_truetype_font {
                    continue;
                }
                found_font = true;

                let first_char = dict
                    .get(b"FirstChar")
                    .expect("FirstChar must be present")
                    .as_i64()
                    .expect("FirstChar must be an integer");
                let last_char = dict
                    .get(b"LastChar")
                    .expect("LastChar must be present")
                    .as_i64()
                    .expect("LastChar must be an integer");
                assert!(
                    first_char >= 0 && first_char < last_char,
                    "FirstChar/LastChar must be a sane ascending range, got {}..={}",
                    first_char,
                    last_char
                );

                let widths = dict
                    .get(b"Widths")
                    .expect("Widths must be present")
                    .as_array()
                    .expect("Widths must be an array");
                assert_eq!(
                    widths.len() as i64,
                    last_char - first_char + 1,
                    "Widths array must have one entry per code in FirstChar..=LastChar"
                );

                // Verify the width for 'H' (0x48) matches the ttf-parser
                // advance for that glyph, scaled to 1000 units/em, within
                // rounding tolerance.
                let font_loader = perfect_print_layout::font_loader::SystemFontLoader::new();
                let props = perfect_print_layout::font_loader::FontProperties::new("Helvetica");
                let (font_data, face_index) = font_loader
                    .get_font_data_for(&props)
                    .expect("Helvetica font data should be available on macOS test hosts");
                let face = ttf_parser::Face::parse(&font_data, face_index).unwrap();
                let glyph_id = face.glyph_index('H').expect("face should have glyph 'H'");
                let expected_width = (face.glyph_hor_advance(glyph_id).unwrap() as f64
                    * 1000.0
                    / face.units_per_em() as f64)
                    .round() as i64;

                let h_index = ('H' as i64) - first_char;
                assert!(h_index >= 0 && (h_index as usize) < widths.len());
                let actual_width = widths[h_index as usize]
                    .as_i64()
                    .or_else(|_| widths[h_index as usize].as_float().map(|f| f as i64))
                    .expect("width entry must be numeric");

                assert!(
                    (actual_width - expected_width).abs() <= 1,
                    "width for 'H' should match ttf-parser advance (±1): got {}, expected {}",
                    actual_width,
                    expected_width
                );
            }
        }

        assert!(found_font, "PDF should contain a TrueType font dictionary");
    }
}
