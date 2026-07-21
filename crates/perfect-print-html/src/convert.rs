//! DOM → `ContentBlock` converter.
//!
//! Walks a `scraper::Html` tree, resolves the CSS cascade (UA stylesheet +
//! document `<style>` blocks + inline `style=""`) at each element, and
//! lowers the result into `perfect_print_layout::flow::ContentBlock`s that
//! the existing `FlowLayoutEngine` can lay out. Never hard-errors on
//! unsupported markup/CSS — everything unsupported is recorded as a
//! warning (mirrors the project's `Strictness::Warn` philosophy).

use ego_tree::NodeRef;
use scraper::{ElementRef, Html, Node, Selector};

use perfect_print_core::color::Color;
use perfect_print_core::draw::{DrawCommand, TextAlign, TextStyle};
use perfect_print_core::font::FontRef;
use perfect_print_core::image::ImageData;
use perfect_print_core::page::{Margins, PageSize};
use perfect_print_core::units::Rect;
use perfect_print_layout::flow::{ContentBlock, ListItem, ListKind, StyledSpan};
use perfect_print_layout::table::{Cell, CellStyle, Column, ColumnWidth, Row};

use crate::css::{parse_color, parse_declarations, parse_length, Declaration};
use crate::stylesheet::Stylesheet;
use crate::{HtmlDocument, HtmlRenderError, HtmlRenderStage, ResourcePolicy};

/// Marker inserted into a span stream to record a `<br>` forced line break;
/// split on this and drop it when materializing `RichParagraph`s.
const BR_MARKER: &str = "\u{2028}";

/// Inline tags recognized by the CSS subset (everything else encountered in
/// inline context is treated as an unstyled `span`, with a warning).
const KNOWN_INLINE_TAGS: &[&str] = &[
    "b", "strong", "i", "em", "u", "s", "strike", "del", "span", "a", "code",
];

/// One resolved image, ready to be inserted into the document's image store.
#[derive(Debug, Clone)]
pub struct LoadedImage {
    pub id: String,
    pub data: ImageData,
}

/// Page size + margins resolved from `@page` (if present) or defaults.
#[derive(Debug, Clone, Copy)]
pub struct PageSetup {
    pub size: PageSize,
    pub margins: Margins,
}

impl Default for PageSetup {
    fn default() -> Self {
        Self {
            size: PageSize::Letter,
            margins: Margins::all(72.0),
        }
    }
}

/// Everything produced by converting an `HtmlDocument`'s DOM into layout
/// blocks.
#[derive(Debug, Clone)]
pub struct ConvertedDocument {
    pub blocks: Vec<ContentBlock>,
    pub title: Option<String>,
    pub page: PageSetup,
    pub images: Vec<LoadedImage>,
    pub warnings: Vec<String>,
}

/// Parse → cascade → convert. Never hard-errors on unsupported HTML/CSS;
/// everything outside the supported subset is recorded in `warnings`.
pub fn convert(doc: &HtmlDocument) -> Result<ConvertedDocument, HtmlRenderError> {
    if doc.html().trim().is_empty() {
        return Err(HtmlRenderError::at_stage(
            "HTML_INPUT_INVALID",
            HtmlRenderStage::Parse,
            "HTML input is empty",
        ));
    }

    let html = Html::parse_document(doc.html());

    let mut converter = Converter::new(doc.policy().clone());
    converter.build_stylesheet(&html);
    converter.page = resolve_page_setup(&converter.sheet, doc);
    converter.title = extract_title(&html);

    if let Ok(selector) = Selector::parse("body") {
        if let Some(body) = html.select(&selector).next() {
            let base_style = converter.base_style();
            converter.walk_container(body, &base_style, 0.0);
        }
    }

    Ok(converter.finish())
}

fn extract_title(html: &Html) -> Option<String> {
    let selector = Selector::parse("title").ok()?;
    let el = html.select(&selector).next()?;
    let text = collapse_whitespace(&el.text().collect::<Vec<_>>().join(""));
    let text = text.trim();
    if text.is_empty() {
        None
    } else {
        Some(text.to_string())
    }
}

/// Precedence: caller's explicit `page_settings()` > document `@page` >
/// letter default.
fn resolve_page_setup(sheet: &Stylesheet, doc: &HtmlDocument) -> PageSetup {
    let mut setup = PageSetup::default();

    if let Some(rule) = &sheet.page_rule {
        if let Some(size_spec) = rule.size {
            setup.size = size_spec.to_page_size();
        }
        if let Some(margin) = rule.margin {
            setup.margins = Margins::all(margin);
        }
    }

    if let Some(explicit) = doc.explicit_page_settings() {
        setup.size = PageSize::Custom {
            width: explicit.width_points,
            height: explicit.height_points,
        };
    }

    setup
}

/// Hard-coded default (user-agent) stylesheet: headings, body/p defaults,
/// and the semantic effect of the basic inline tags — expressed as regular
/// CSS rules so the same cascade machinery applies to them.
fn ua_stylesheet() -> Stylesheet {
    Stylesheet::parse(
        "h1 { font-family: Helvetica; font-size: 24pt; font-weight: bold; color: #000000 } \
         h2 { font-family: Helvetica; font-size: 18pt; font-weight: bold; color: #000000 } \
         h3 { font-family: Helvetica; font-size: 14pt; font-weight: bold; color: #000000 } \
         h4 { font-family: Helvetica; font-size: 12pt; font-weight: bold; color: #000000 } \
         h5 { font-family: Helvetica; font-size: 12pt; font-weight: bold; color: #000000 } \
         h6 { font-family: Helvetica; font-size: 12pt; font-weight: bold; color: #000000 } \
         body { font-family: Helvetica; font-size: 12pt; color: #000000 } \
         p { font-family: Helvetica; font-size: 12pt; color: #000000 } \
         li { font-family: Helvetica; font-size: 12pt; color: #000000 } \
         b, strong { font-weight: bold } \
         i, em { font-style: italic } \
         u { text-decoration: underline } \
         s, strike, del { text-decoration: line-through } \
         code { font-family: Courier }",
    )
}

/// CSS `position` values we act on. `Absolute` takes the element out of the
/// normal flow and onto a `ContentBlock::Positioned`; `Relative` is accepted
/// as a flow-preserving no-op (it just establishes a coordinate origin,
/// which is already the page/content origin in this renderer). Anything
/// else (`fixed`, `sticky`, ...) is unsupported and warns.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PositionMode {
    Absolute,
    Relative,
}

/// Non-inherited per-element CSS effects (margins, breaks, table styling).
#[derive(Debug, Clone, Default)]
struct ElementProps {
    margin_top: Option<f64>,
    margin_bottom: Option<f64>,
    page_break_before: bool,
    page_break_after: bool,
    background_color: Option<Color>,
    padding: Option<f64>,
    position: Option<PositionMode>,
    left: Option<f64>,
    top: Option<f64>,
    explicit_width: Option<f64>,
}

fn parse_font_weight(value: &str) -> Option<bool> {
    let v = value.trim().to_ascii_lowercase();
    match v.as_str() {
        "normal" => Some(false),
        "bold" => Some(true),
        _ => v.parse::<u32>().ok().map(|n| n >= 700),
    }
}

fn parse_line_height(value: &str, font_size: f64) -> Option<f64> {
    let v = value.trim();
    if let Ok(multiplier) = v.parse::<f64>() {
        return Some(multiplier * font_size);
    }
    parse_length(v, font_size)
}

/// Apply a cascade-ordered declaration list onto a style inherited from the
/// parent. Inherited fields (font, color, align, line-height,
/// letter-spacing, decorations) are carried on `style`; non-inherited
/// per-element effects land in the returned `ElementProps`. Anything
/// unrecognized is pushed to `warnings`, never a hard error.
fn apply_declarations(
    parent: &TextStyle,
    decls: &[Declaration],
    warnings: &mut Vec<String>,
) -> (TextStyle, ElementProps) {
    let mut style = parent.clone();
    let mut props = ElementProps::default();

    for d in decls {
        match d.property.as_str() {
            "font-family" => {
                let name = d
                    .value
                    .split(',')
                    .next()
                    .unwrap_or("")
                    .trim()
                    .trim_matches('"')
                    .trim_matches('\'')
                    .to_string();
                if !name.is_empty() {
                    style.font = FontRef::new(name);
                }
            }
            "font-size" => match parse_length(&d.value, parent.size) {
                Some(size) if size > 0.0 => style.size = size,
                _ => warnings.push(format!("unsupported font-size: {}", d.value)),
            },
            "font-weight" => match parse_font_weight(&d.value) {
                Some(bold) => style.bold = bold,
                None => warnings.push(format!("unsupported font-weight: {}", d.value)),
            },
            "font-style" => {
                let v = d.value.trim().to_ascii_lowercase();
                match v.as_str() {
                    "italic" | "oblique" => style.italic = true,
                    "normal" => style.italic = false,
                    _ => warnings.push(format!("unsupported font-style: {}", d.value)),
                }
            }
            "color" => match parse_color(&d.value) {
                Some(c) => style.color = c,
                None => warnings.push(format!("unsupported color: {}", d.value)),
            },
            "text-align" => {
                let v = d.value.trim().to_ascii_lowercase();
                match v.as_str() {
                    "left" => style.align = TextAlign::Left,
                    "right" => style.align = TextAlign::Right,
                    "center" => style.align = TextAlign::Center,
                    "justify" => style.align = TextAlign::Justified,
                    _ => warnings.push(format!("unsupported text-align: {}", d.value)),
                }
            }
            "line-height" => match parse_line_height(&d.value, style.size) {
                Some(lh) => style.line_height = Some(lh),
                None => warnings.push(format!("unsupported line-height: {}", d.value)),
            },
            "letter-spacing" => match parse_length(&d.value, style.size) {
                Some(ls) => style.letter_spacing = Some(ls),
                None => warnings.push(format!("unsupported letter-spacing: {}", d.value)),
            },
            "text-decoration" => {
                for tok in d.value.split_whitespace() {
                    match tok.to_ascii_lowercase().as_str() {
                        "underline" => style.underline = true,
                        "line-through" => style.strikethrough = true,
                        "none" => {}
                        other => warnings.push(format!("unsupported text-decoration: {other}")),
                    }
                }
            }
            "margin-top" => match parse_length(&d.value, style.size) {
                Some(m) => props.margin_top = Some(m),
                None => warnings.push(format!("unsupported margin-top: {}", d.value)),
            },
            "margin-bottom" => match parse_length(&d.value, style.size) {
                Some(m) => props.margin_bottom = Some(m),
                None => warnings.push(format!("unsupported margin-bottom: {}", d.value)),
            },
            "background-color" => match parse_color(&d.value) {
                Some(c) => props.background_color = Some(c),
                None => warnings.push(format!("unsupported background-color: {}", d.value)),
            },
            "padding" => match parse_length(&d.value, style.size) {
                Some(p) => props.padding = Some(p),
                None => warnings.push(format!("unsupported padding: {}", d.value)),
            },
            "page-break-before" => {
                props.page_break_before = d.value.trim().eq_ignore_ascii_case("always")
            }
            "page-break-after" => {
                props.page_break_after = d.value.trim().eq_ignore_ascii_case("always")
            }
            "break-before" => {
                if d.value.trim().eq_ignore_ascii_case("page") {
                    props.page_break_before = true;
                }
            }
            "break-after" => {
                if d.value.trim().eq_ignore_ascii_case("page") {
                    props.page_break_after = true;
                }
            }
            "position" => {
                let v = d.value.trim().to_ascii_lowercase();
                match v.as_str() {
                    "absolute" => props.position = Some(PositionMode::Absolute),
                    "relative" => props.position = Some(PositionMode::Relative),
                    "static" => props.position = None,
                    _ => warnings.push(format!("unsupported position: {}", d.value)),
                }
            }
            "left" => match parse_length(&d.value, style.size) {
                Some(l) => props.left = Some(l),
                None => warnings.push(format!("unsupported left: {}", d.value)),
            },
            "top" => match parse_length(&d.value, style.size) {
                Some(t) => props.top = Some(t),
                None => warnings.push(format!("unsupported top: {}", d.value)),
            },
            "width" => match parse_length(&d.value, style.size) {
                Some(w) => props.explicit_width = Some(w),
                None => warnings.push(format!("unsupported width: {}", d.value)),
            },
            other => warnings.push(format!("unsupported CSS property: {other}")),
        }
    }

    (style, props)
}

/// Collapse HTML whitespace: any run of `[\t\n\r ]` collapses to a single
/// space. Does not trim the ends — callers decide edge-trimming policy.
fn collapse_whitespace(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut last_was_space = false;
    for c in s.chars() {
        if c == ' ' || c == '\t' || c == '\n' || c == '\r' {
            if !last_was_space {
                out.push(' ');
            }
            last_was_space = true;
        } else {
            out.push(c);
            last_was_space = false;
        }
    }
    out
}

/// Collapse whitespace within each span, then trim leading whitespace off
/// the first span and trailing whitespace off the last (HTML block-edge
/// trimming), dropping spans that become empty.
fn collapse_span_whitespace(spans: Vec<StyledSpan>) -> Vec<StyledSpan> {
    let mut collapsed: Vec<StyledSpan> = spans
        .into_iter()
        .map(|s| StyledSpan {
            text: collapse_whitespace(&s.text),
            style: s.style,
        })
        .filter(|s| !s.text.is_empty())
        .collect();

    if let Some(first) = collapsed.first_mut() {
        first.text = first.text.trim_start().to_string();
    }
    if let Some(last) = collapsed.last_mut() {
        last.text = last.text.trim_end().to_string();
    }
    collapsed.retain(|s| !s.text.is_empty());
    collapsed
}

/// Split a span stream on `<br>` markers into separate groups (paragraphs),
/// dropping the marker spans themselves.
fn split_on_br(spans: Vec<StyledSpan>) -> Vec<Vec<StyledSpan>> {
    let mut groups = Vec::new();
    let mut current = Vec::new();
    for span in spans {
        if span.text == BR_MARKER {
            groups.push(std::mem::take(&mut current));
        } else {
            current.push(span);
        }
    }
    groups.push(current);
    groups
}

/// Minimal RFC 4648 base64 decoder (standard alphabet), used for `data:`
/// image URIs. Avoids pulling in an extra dependency for a single call
/// site.
fn decode_base64(input: &str) -> Result<Vec<u8>, String> {
    fn value(b: u8) -> Option<u8> {
        match b {
            b'A'..=b'Z' => Some(b - b'A'),
            b'a'..=b'z' => Some(b - b'a' + 26),
            b'0'..=b'9' => Some(b - b'0' + 52),
            b'+' => Some(62),
            b'/' => Some(63),
            _ => None,
        }
    }

    let clean: Vec<u8> = input.bytes().filter(|b| !b.is_ascii_whitespace()).collect();
    let mut out = Vec::with_capacity(clean.len() / 4 * 3);
    let mut chunk = [0u8; 4];
    let mut chunk_len = 0usize;
    let mut pad = 0usize;

    for &b in &clean {
        if b == b'=' {
            chunk[chunk_len] = 0;
            chunk_len += 1;
            pad += 1;
        } else {
            let v = value(b).ok_or_else(|| "invalid base64 character".to_string())?;
            chunk[chunk_len] = v;
            chunk_len += 1;
        }
        if chunk_len == 4 {
            let n = (chunk[0] as u32) << 18
                | (chunk[1] as u32) << 12
                | (chunk[2] as u32) << 6
                | (chunk[3] as u32);
            out.push((n >> 16) as u8);
            if pad < 2 {
                out.push((n >> 8) as u8);
            }
            if pad < 1 {
                out.push(n as u8);
            }
            chunk_len = 0;
            pad = 0;
        }
    }
    Ok(out)
}

struct Converter {
    sheet: Stylesheet,
    policy: ResourcePolicy,
    page: PageSetup,
    title: Option<String>,
    blocks: Vec<ContentBlock>,
    images: Vec<LoadedImage>,
    warnings: Vec<String>,
    counter: usize,
    pending_gap: f64,
    has_content: bool,
}

impl Converter {
    fn new(policy: ResourcePolicy) -> Self {
        Self {
            sheet: Stylesheet::empty(),
            policy,
            page: PageSetup::default(),
            title: None,
            blocks: Vec::new(),
            images: Vec::new(),
            warnings: Vec::new(),
            counter: 0,
            pending_gap: 0.0,
            has_content: false,
        }
    }

    fn build_stylesheet(&mut self, html: &Html) {
        let mut sheet = ua_stylesheet();
        if let Ok(selector) = Selector::parse("style") {
            for el in html.select(&selector) {
                let css_text: String = el.text().collect::<Vec<_>>().join("");
                sheet = sheet.merge(Stylesheet::parse(&css_text));
            }
        }
        self.sheet = sheet;
    }

    fn base_style(&self) -> TextStyle {
        let mut style = TextStyle::new(FontRef::new("Helvetica"), 12.0);
        style.color = Color::black();
        let decls = self.sheet.matching_declarations("body", &[], None);
        let mut warnings = Vec::new();
        let (resolved, _) = apply_declarations(&style, &decls, &mut warnings);
        style = resolved;
        style
    }

    fn content_width(&self) -> f64 {
        (self.page.size.width() - self.page.margins.left - self.page.margins.right).max(1.0)
    }

    fn resolve(&mut self, el: ElementRef, parent: &TextStyle) -> (TextStyle, ElementProps) {
        let value = el.value();
        let tag = value.name().to_ascii_lowercase();
        let classes: Vec<String> = value.classes().map(|c| c.to_string()).collect();
        let id = value.id();
        let mut decls = self.sheet.matching_declarations(&tag, &classes, id);
        if let Some(style_attr) = value.attr("style") {
            decls.extend(parse_declarations(style_attr));
        }
        apply_declarations(parent, &decls, &mut self.warnings)
    }

    fn push_gap(&mut self, margin_top: f64) {
        if self.has_content {
            let gap = self.pending_gap.max(margin_top);
            if gap > 0.0 {
                self.blocks.push(ContentBlock::Gap(gap));
            }
        }
        self.pending_gap = 0.0;
    }

    fn set_pending(&mut self, margin_bottom: f64) {
        self.pending_gap = self.pending_gap.max(margin_bottom);
    }

    /// Walk `node`'s children, dispatching block-level elements immediately
    /// and accumulating inline content (text + inline elements) into an
    /// implicit paragraph flushed whenever a block boundary is hit.
    fn walk_container<'a>(&mut self, node: ElementRef<'a>, style: &TextStyle, indent: f64) {
        let mut pending_inline: Vec<NodeRef<'a, Node>> = Vec::new();

        for child in node.children() {
            if child.value().as_text().is_some() {
                pending_inline.push(child);
                continue;
            }

            let Some(el) = ElementRef::wrap(child) else {
                continue;
            };
            let tag = el.value().name().to_ascii_lowercase();

            if KNOWN_INLINE_TAGS.contains(&tag.as_str()) || tag == "br" {
                pending_inline.push(child);
                continue;
            }

            self.flush_inline_run(&mut pending_inline, style, indent);

            match tag.as_str() {
                "script" => self.warnings.push("ignored <script> element".to_string()),
                "style" => {} // already consumed as CSS
                "head" => {}  // only <title> is used, extracted separately
                "title" => {}
                "hr" => self.emit_hr(style, indent),
                "img" => self.emit_img(el),
                "table" => self.emit_table(el, style),
                "ul" => self.emit_list(el, ListKind::Bulleted, style, indent),
                "ol" => self.emit_list(el, ListKind::Numbered, style, indent),
                "blockquote" => {
                    let (el_style, props) = self.resolve(el, style);
                    self.open_block(&props);
                    self.walk_container(el, &el_style, indent + 36.0);
                    self.close_block(&props, el_style.size * 0.5);
                }
                "h1" | "h2" | "h3" | "h4" | "h5" | "h6" | "p" | "li" => {
                    self.emit_paragraph(el, &tag, style, indent)
                }
                "div" | "body" | "html" => {
                    let (el_style, props) = self.resolve(el, style);
                    if props.position == Some(PositionMode::Absolute) {
                        self.emit_positioned(el, &el_style, &props);
                    } else {
                        self.open_block(&props);
                        self.walk_container(el, &el_style, indent);
                        self.close_block(&props, el_style.size * 0.5);
                    }
                }
                other => {
                    self.warnings
                        .push(format!("unsupported tag treated as block: <{other}>"));
                    let (el_style, props) = self.resolve(el, style);
                    self.open_block(&props);
                    self.walk_container(el, &el_style, indent);
                    self.close_block(&props, el_style.size * 0.5);
                }
            }
        }

        self.flush_inline_run(&mut pending_inline, style, indent);
    }

    fn open_block(&mut self, props: &ElementProps) {
        if props.page_break_before {
            self.pending_gap = 0.0;
            self.blocks.push(ContentBlock::PageBreak);
        } else {
            self.push_gap(props.margin_top.unwrap_or(0.0));
        }
    }

    fn close_block(&mut self, props: &ElementProps, default_margin: f64) {
        self.set_pending(props.margin_bottom.unwrap_or(default_margin));
        if props.page_break_after {
            self.blocks.push(ContentBlock::PageBreak);
        }
    }

    /// `position: absolute` element: converted to a `ContentBlock::Positioned`
    /// instead of participating in the normal block flow. Children convert
    /// recursively as normal blocks, but into their own isolated block list
    /// (starting a fresh coordinate frame at the positioned box's origin) —
    /// the flow's gap/margin bookkeeping (`pending_gap`, `has_content`) is
    /// saved and restored around the recursion so the positioned element is
    /// completely invisible to the surrounding flow, matching CSS taking it
    /// out of flow.
    fn emit_positioned(&mut self, el: ElementRef, style: &TextStyle, props: &ElementProps) {
        let x = props.left.unwrap_or(0.0);
        let y = props.top.unwrap_or(0.0);
        let content_width = self.content_width();
        let width = props
            .explicit_width
            .unwrap_or_else(|| (content_width - x).max(1.0));

        let saved_blocks = std::mem::take(&mut self.blocks);
        let saved_pending_gap = self.pending_gap;
        let saved_has_content = self.has_content;
        self.pending_gap = 0.0;
        self.has_content = false;

        self.walk_container(el, style, 0.0);

        let inner_blocks = std::mem::replace(&mut self.blocks, saved_blocks);
        self.pending_gap = saved_pending_gap;
        self.has_content = saved_has_content;

        self.blocks.push(ContentBlock::Positioned {
            x,
            y,
            width,
            blocks: inner_blocks,
        });
    }

    fn flush_inline_run<'a>(
        &mut self,
        pending: &mut Vec<NodeRef<'a, Node>>,
        style: &TextStyle,
        indent: f64,
    ) {
        if pending.is_empty() {
            return;
        }
        let mut spans = Vec::new();
        for node in pending.drain(..) {
            self.collect_inline_node(node, style, &mut spans);
        }
        self.emit_rich_groups(spans, style, indent);
    }

    fn collect_inline_node<'a>(
        &mut self,
        node: NodeRef<'a, Node>,
        style: &TextStyle,
        spans: &mut Vec<StyledSpan>,
    ) {
        if let Some(text) = node.value().as_text() {
            spans.push(StyledSpan {
                text: text.to_string(),
                style: style.clone(),
            });
            return;
        }

        let Some(el) = ElementRef::wrap(node) else {
            return;
        };
        let tag = el.value().name().to_ascii_lowercase();

        if tag == "br" {
            spans.push(StyledSpan {
                text: BR_MARKER.to_string(),
                style: style.clone(),
            });
            return;
        }

        if !KNOWN_INLINE_TAGS.contains(&tag.as_str()) {
            self.warnings.push(format!("unsupported tag: <{tag}>"));
        }

        let (child_style, _props) = self.resolve(el, style);
        for gc in el.children() {
            self.collect_inline_node(gc, &child_style, spans);
        }
    }

    fn emit_rich_groups(&mut self, spans: Vec<StyledSpan>, base_style: &TextStyle, indent: f64) {
        for group in split_on_br(spans) {
            let group = collapse_span_whitespace(group);
            if group.is_empty() {
                continue;
            }
            self.blocks.push(ContentBlock::RichParagraph {
                spans: group,
                base_style: base_style.clone(),
                indent_left: indent,
            });
            self.has_content = true;
        }
    }

    fn emit_paragraph(&mut self, el: ElementRef, tag: &str, style: &TextStyle, indent: f64) {
        let (el_style, props) = self.resolve(el, style);
        let is_heading =
            tag.len() == 2 && tag.starts_with('h') && tag.as_bytes()[1].is_ascii_digit();
        let default_margin = if is_heading {
            el_style.size * 0.75
        } else {
            el_style.size * 0.5
        };

        self.open_block(&ElementProps {
            margin_top: Some(props.margin_top.unwrap_or(default_margin)),
            ..props.clone()
        });

        let mut spans = Vec::new();
        for child in el.children() {
            self.collect_inline_node(child, &el_style, &mut spans);
        }
        self.emit_rich_groups(spans, &el_style, indent);

        self.close_block(&props, default_margin);
    }

    fn emit_hr(&mut self, style: &TextStyle, indent: f64) {
        let margin = style.size * 0.5;
        self.push_gap(margin);
        let content_width = self.content_width();
        let rect = Rect::new(indent, 0.0, (content_width - indent).max(1.0), 0.5);
        self.blocks.push(ContentBlock::Commands(vec![DrawCommand::FillRect {
            rect,
            color: Color::gray(0.6),
        }]));
        self.has_content = true;
        self.set_pending(margin);
    }

    fn emit_img(&mut self, el: ElementRef) {
        let Some(src) = el.value().attr("src").map(str::to_string) else {
            self.warnings.push("img element missing src".to_string());
            return;
        };
        if src.is_empty() {
            self.warnings.push("img element missing src".to_string());
            return;
        }

        match self.load_image_bytes(&src) {
            Ok(Some(bytes)) => match ImageData::load_from_bytes(&bytes) {
                Ok(data) => {
                    let id = format!("html-img-{}", self.counter);
                    self.counter += 1;
                    let width = el
                        .value()
                        .attr("width")
                        .and_then(|v| v.parse::<f64>().ok())
                        .unwrap_or(data.width as f64);
                    let height = el
                        .value()
                        .attr("height")
                        .and_then(|v| v.parse::<f64>().ok())
                        .unwrap_or(data.height as f64);
                    self.images.push(LoadedImage {
                        id: id.clone(),
                        data,
                    });
                    self.blocks.push(ContentBlock::Image {
                        image_id: id,
                        dest_rect: Rect::new(0.0, 0.0, width, height),
                    });
                    self.has_content = true;
                }
                Err(e) => self
                    .warnings
                    .push(format!("failed to decode image '{src}': {e}")),
            },
            Ok(None) => self
                .warnings
                .push(format!("image blocked by resource policy: {src}")),
            Err(e) => self
                .warnings
                .push(format!("failed to load image '{src}': {e}")),
        }
    }

    fn load_image_bytes(&self, src: &str) -> Result<Option<Vec<u8>>, String> {
        if let Some(rest) = src.strip_prefix("data:") {
            if let Some(comma) = rest.find(',') {
                let meta = &rest[..comma];
                let data_part = &rest[comma + 1..];
                if meta.contains("base64") {
                    return decode_base64(data_part).map(Some);
                }
                return Ok(Some(data_part.as_bytes().to_vec()));
            }
        }

        if let Ok(url) = url::Url::parse(src) {
            return match self.policy.allows_url(src) {
                Ok(true) => {
                    if url.scheme() == "file" {
                        match url.to_file_path() {
                            Ok(path) => std::fs::read(&path).map(Some).map_err(|e| e.to_string()),
                            Err(_) => Ok(None),
                        }
                    } else {
                        // data/about were handled above; http(s) has no fetch
                        // client in this pure-Rust pipeline.
                        Ok(None)
                    }
                }
                Ok(false) => Ok(None),
                Err(e) => Err(e.to_string()),
            };
        }

        let path = std::path::Path::new(src);
        match self.policy.allows_local_path(path) {
            Ok(true) => std::fs::read(path).map(Some).map_err(|e| e.to_string()),
            Ok(false) => Ok(None),
            Err(e) => Err(e.to_string()),
        }
    }

    fn emit_table(&mut self, el: ElementRef, style: &TextStyle) {
        let (el_style, props) = self.resolve(el, style);
        let mut rows: Vec<Row> = Vec::new();
        let mut max_cols = 0usize;
        self.collect_table_rows(el, &el_style, &mut rows, &mut max_cols);
        if rows.is_empty() {
            return;
        }
        let columns: Vec<Column> = (0..max_cols).map(|_| Column::new(ColumnWidth::Auto)).collect();

        let default_margin = el_style.size * 0.5;
        self.open_block(&ElementProps {
            margin_top: Some(props.margin_top.unwrap_or(default_margin)),
            ..props.clone()
        });
        self.blocks.push(ContentBlock::Table { columns, rows });
        self.has_content = true;
        self.close_block(&props, default_margin);
    }

    fn collect_table_rows(
        &mut self,
        node: ElementRef,
        style: &TextStyle,
        rows: &mut Vec<Row>,
        max_cols: &mut usize,
    ) {
        for child in node.children() {
            let Some(el) = ElementRef::wrap(child) else {
                continue;
            };
            let tag = el.value().name().to_ascii_lowercase();
            match tag.as_str() {
                "thead" | "tbody" | "tfoot" => self.collect_table_rows(el, style, rows, max_cols),
                "tr" => {
                    let mut cells = Vec::new();
                    let mut is_header_row = false;
                    for cc in el.children() {
                        let Some(cell_el) = ElementRef::wrap(cc) else {
                            continue;
                        };
                        let ctag = cell_el.value().name().to_ascii_lowercase();
                        if ctag != "td" && ctag != "th" {
                            continue;
                        }
                        let (mut cell_style, cell_props) = self.resolve(cell_el, style);
                        if ctag == "th" {
                            cell_style.bold = true;
                            is_header_row = true;
                        }
                        let raw: String = cell_el.text().collect::<Vec<_>>().join("");
                        let text = collapse_whitespace(raw.trim());
                        let mut cell_style_wrapper = CellStyle {
                            text_style: cell_style,
                            ..CellStyle::default()
                        };
                        if let Some(bg) = cell_props.background_color {
                            cell_style_wrapper.background = Some(bg);
                        }
                        if let Some(pad) = cell_props.padding {
                            cell_style_wrapper.padding = pad;
                        }
                        cells.push(Cell::new(text).with_style(cell_style_wrapper));
                    }
                    if !cells.is_empty() {
                        *max_cols = (*max_cols).max(cells.len());
                        rows.push(if is_header_row {
                            Row::header(cells)
                        } else {
                            Row::new(cells)
                        });
                    }
                }
                _ => {}
            }
        }
    }

    fn emit_list(&mut self, el: ElementRef, kind: ListKind, style: &TextStyle, indent: f64) {
        let _ = indent; // list indent is handled per-item via ListItem::level
        let (el_style, props) = self.resolve(el, style);
        let mut items = Vec::new();
        self.collect_list_items(el, &el_style, 0, &mut items);
        if items.is_empty() {
            return;
        }
        let default_margin = el_style.size * 0.5;
        self.open_block(&ElementProps {
            margin_top: Some(props.margin_top.unwrap_or(default_margin)),
            ..props.clone()
        });
        self.blocks.push(ContentBlock::List {
            items,
            kind,
            style: el_style,
        });
        self.has_content = true;
        self.close_block(&props, default_margin);
    }

    fn collect_list_items(
        &mut self,
        el: ElementRef,
        style: &TextStyle,
        level: usize,
        out: &mut Vec<ListItem>,
    ) {
        for child in el.children() {
            let Some(li) = ElementRef::wrap(child) else {
                continue;
            };
            if li.value().name().to_ascii_lowercase() != "li" {
                continue;
            }
            let (li_style, _props) = self.resolve(li, style);

            let mut spans = Vec::new();
            let mut nested_lists: Vec<(ElementRef, ListKind)> = Vec::new();
            for gc in li.children() {
                if let Some(ge) = ElementRef::wrap(gc) {
                    let gtag = ge.value().name().to_ascii_lowercase();
                    if gtag == "ul" {
                        nested_lists.push((ge, ListKind::Bulleted));
                        continue;
                    }
                    if gtag == "ol" {
                        nested_lists.push((ge, ListKind::Numbered));
                        continue;
                    }
                }
                self.collect_inline_node(gc, &li_style, &mut spans);
            }
            let spans = collapse_span_whitespace(spans);
            out.push(ListItem { spans, level });

            for (nested_el, _nested_kind) in nested_lists {
                self.collect_list_items(nested_el, &li_style, level + 1, out);
            }
        }
    }

    fn finish(self) -> ConvertedDocument {
        ConvertedDocument {
            blocks: self.blocks,
            title: self.title,
            page: self.page,
            images: self.images,
            warnings: self.warnings,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::HtmlDocument;

    fn blocks_of(html: &str) -> ConvertedDocument {
        convert(&HtmlDocument::new(html)).unwrap()
    }

    #[test]
    fn collapse_whitespace_runs_collapse_to_single_space() {
        assert_eq!(collapse_whitespace("a   b"), "a b");
        assert_eq!(collapse_whitespace("a\n\t b"), "a b");
        assert_eq!(collapse_whitespace("  a  "), " a ");
        assert_eq!(collapse_whitespace("abc"), "abc");
        assert_eq!(collapse_whitespace(""), "");
    }

    #[test]
    fn paragraph_with_inline_bold() {
        let c = blocks_of("<p>Hello <b>world</b></p>");
        assert_eq!(c.blocks.len(), 1);
        let ContentBlock::RichParagraph { spans, .. } = &c.blocks[0] else {
            panic!("expected RichParagraph, got {:?}", c.blocks[0]);
        };
        assert_eq!(spans.len(), 2);
        assert!(!spans[0].style.bold && spans[1].style.bold);
        assert_eq!(spans[1].text, "world");
    }

    #[test]
    fn headings_use_ua_defaults() {
        let c = blocks_of("<h1>Title</h1>");
        let ContentBlock::RichParagraph { spans, .. } = &c.blocks[0] else {
            panic!("expected RichParagraph");
        };
        assert_eq!(spans[0].style.size, 24.0);
        assert!(spans[0].style.bold);
    }

    #[test]
    fn style_block_and_inline_style_cascade() {
        let c = blocks_of(
            r#"<style>p { color: #0000ff } .big { font-size: 20pt }</style>
                        <p class="big" style="color: #ff0000">x</p>"#,
        );
        let ContentBlock::RichParagraph { spans, .. } = &c.blocks[0] else {
            panic!("expected RichParagraph");
        };
        assert_eq!(spans[0].style.color, Color::rgb(1.0, 0.0, 0.0));
        assert_eq!(spans[0].style.size, 20.0);
    }

    #[test]
    fn nested_inline_styles_compose() {
        let c = blocks_of("<i><b>x</b></i>");
        let ContentBlock::RichParagraph { spans, .. } = &c.blocks[0] else {
            panic!("expected RichParagraph");
        };
        assert!(spans[0].style.bold && spans[0].style.italic);
    }

    #[test]
    fn lists_convert() {
        let c = blocks_of("<ul><li>a</li><li>b</li></ul>");
        let ContentBlock::List { items, kind, .. } = &c.blocks[0] else {
            panic!("expected List, got {:?}", c.blocks[0]);
        };
        assert!(matches!(kind, ListKind::Bulleted));
        assert_eq!(items.len(), 2);
    }

    #[test]
    fn ordered_list_is_numbered() {
        let c = blocks_of("<ol><li>a</li><li>b</li></ol>");
        let ContentBlock::List { kind, .. } = &c.blocks[0] else {
            panic!("expected List");
        };
        assert!(matches!(kind, ListKind::Numbered));
    }

    #[test]
    fn tables_convert() {
        let c = blocks_of("<table><tr><th>H</th></tr><tr><td>c</td></tr></table>");
        let ContentBlock::Table { columns, rows } = &c.blocks[0] else {
            panic!("expected Table, got {:?}", c.blocks[0]);
        };
        assert_eq!(columns.len(), 1);
        assert_eq!(rows.len(), 2);
        assert!(rows[0].is_header);
        assert!(rows[0].cells[0].style.text_style.bold);
    }

    #[test]
    fn hr_becomes_rule_and_br_breaks() {
        let c = blocks_of("<p>a<br>b</p><hr>");
        let rich_paragraphs: Vec<_> = c
            .blocks
            .iter()
            .filter(|b| matches!(b, ContentBlock::RichParagraph { .. }))
            .collect();
        assert_eq!(
            rich_paragraphs.len(),
            2,
            "br should split into two paragraphs"
        );
        let has_commands = c
            .blocks
            .iter()
            .any(|b| matches!(b, ContentBlock::Commands(_)));
        assert!(has_commands, "hr should emit a Commands block");
        assert!(c.warnings.is_empty(), "supported markup should not warn");
    }

    #[test]
    fn page_break_css() {
        let c = blocks_of(r#"<p>a</p><p style="page-break-before: always">b</p>"#);
        assert!(matches!(c.blocks[1], ContentBlock::PageBreak));
    }

    #[test]
    fn at_page_sets_size() {
        let c = blocks_of("<style>@page { size: a4 }</style><p>x</p>");
        assert_eq!(c.page.size.to_size().width, 595.0);
        assert_eq!(c.page.size.to_size().height, 842.0);
    }

    #[test]
    fn unknown_tag_warns_not_errors() {
        let c = blocks_of("<article><p>x</p></article><video src=\"x\"></video>");
        assert!(!c.blocks.is_empty());
        assert!(c.warnings.iter().any(|w| w.contains("video")));
    }

    #[test]
    fn img_data_uri_loads() {
        // A minimal 1x1 transparent PNG, base64-encoded.
        let png_b64 = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+A8AAQUBAScY42YAAAAASUVORK5CYII=";
        let html = format!(r#"<img src="data:image/png;base64,{png_b64}">"#);
        let c = blocks_of(&html);
        assert_eq!(c.images.len(), 1);
        assert!(matches!(c.blocks[0], ContentBlock::Image { .. }));
    }

    #[test]
    fn img_file_outside_base_dir_rejected() {
        // Default offline policy has no local base directory configured, so
        // any local file reference is rejected — warning, no image block.
        let c = blocks_of(r#"<img src="/etc/hosts">"#);
        assert!(c.images.is_empty());
        assert!(!c
            .blocks
            .iter()
            .any(|b| matches!(b, ContentBlock::Image { .. })));
        assert!(c.warnings.iter().any(|w| w.contains("policy")));
    }

    #[test]
    fn title_extracted() {
        let c = blocks_of("<html><head><title>T</title></head><body><p>x</p></body></html>");
        assert_eq!(c.title.as_deref(), Some("T"));
    }

    #[test]
    fn margins_between_blocks() {
        let c = blocks_of("<p>a</p><p>b</p>");
        assert!(
            c.blocks
                .iter()
                .any(|b| matches!(b, ContentBlock::Gap(g) if *g > 0.0)),
            "expected a Gap block between the two paragraphs, got {:?}",
            c.blocks
        );
    }

    #[test]
    fn no_leading_gap_before_first_block() {
        let c = blocks_of("<p>Hello <b>world</b></p>");
        assert_eq!(
            c.blocks.len(),
            1,
            "no leading margin before the first block"
        );
    }

    #[test]
    fn base64_decoder_roundtrips_known_vector() {
        // "Man" -> "TWFu" is the canonical RFC 4648 test vector.
        assert_eq!(decode_base64("TWFu").unwrap(), b"Man");
        assert_eq!(decode_base64("TWE=").unwrap(), b"Ma");
        assert_eq!(decode_base64("TQ==").unwrap(), b"M");
    }

    #[test]
    fn absolute_div_becomes_positioned_block() {
        let c = blocks_of(r#"<div style="position:absolute;left:1in;top:2in;width:3in">Hi</div>"#);
        assert_eq!(c.blocks.len(), 1);
        let ContentBlock::Positioned {
            x,
            y,
            width,
            blocks,
        } = &c.blocks[0]
        else {
            panic!("expected Positioned, got {:?}", c.blocks[0]);
        };
        assert_eq!(*x, 72.0);
        assert_eq!(*y, 144.0);
        assert_eq!(*width, 216.0);
        assert_eq!(blocks.len(), 1);
        assert!(matches!(blocks[0], ContentBlock::RichParagraph { .. }));
        assert!(
            c.warnings.iter().all(|w| !w.contains("position")
                && !w.contains("unsupported left")
                && !w.contains("unsupported top")
                && !w.contains("unsupported width")),
            "position/left/top/width should not warn when handled, got {:?}",
            c.warnings
        );
    }

    #[test]
    fn absolute_div_missing_left_top_defaults_to_zero() {
        let c = blocks_of(r#"<div style="position:absolute;width:1in">Hi</div>"#);
        let ContentBlock::Positioned { x, y, .. } = &c.blocks[0] else {
            panic!("expected Positioned, got {:?}", c.blocks[0]);
        };
        assert_eq!(*x, 0.0);
        assert_eq!(*y, 0.0);
    }

    #[test]
    fn absolute_div_missing_width_uses_remaining_content_width() {
        let c = blocks_of(r#"<div style="position:absolute;left:1in">Hi</div>"#);
        let ContentBlock::Positioned { x, width, .. } = &c.blocks[0] else {
            panic!("expected Positioned, got {:?}", c.blocks[0]);
        };
        // Letter width (612pt) minus default 72pt margins each side = 468pt
        // content width; minus the 72pt left offset = 396pt remaining.
        assert_eq!(*x, 72.0);
        assert_eq!(*width, 396.0);
    }

    #[test]
    fn template_like_document_produces_one_positioned_block_each_no_flow_order() {
        let html = r#"
            <div style="position:absolute;left:0.5in;top:0.5in">INVOICE</div>
            <div style="position:absolute;left:0.5in;top:1.2in">Address</div>
            <div style="position:absolute;left:5.5in;top:7in;width:2.5in">Total</div>
        "#;
        let c = blocks_of(html);
        let positioned: Vec<_> = c
            .blocks
            .iter()
            .filter(|b| matches!(b, ContentBlock::Positioned { .. }))
            .collect();
        assert_eq!(positioned.len(), 3, "got blocks: {:?}", c.blocks);
        // No flow-order dependence: there should be no Gap blocks pushed as a
        // side effect of the absolutely-positioned divs (they never touch
        // pending_gap/has_content), and nothing but the three Positioned
        // blocks themselves.
        assert!(
            c.blocks
                .iter()
                .all(|b| matches!(b, ContentBlock::Positioned { .. })),
            "expected only Positioned blocks, got {:?}",
            c.blocks
        );
    }

    #[test]
    fn relative_position_is_silent_no_op_for_flow() {
        let c = blocks_of(r#"<div style="position:relative"><p>x</p></div>"#);
        assert!(
            c.warnings.iter().all(|w| !w.contains("position")),
            "position:relative should not warn, got {:?}",
            c.warnings
        );
        assert!(!c
            .blocks
            .iter()
            .any(|b| matches!(b, ContentBlock::Positioned { .. })));
    }
}
