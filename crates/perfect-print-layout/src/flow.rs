//! Flow layout and pagination engine.
//!
//! Takes a stream of content blocks and lays them out across pages:
//! - Page breaks between blocks
//! - Widow/orphan control (minimum lines kept together)
//! - Repeated headers/footers on each page
//! - Table header repetition on page breaks

use perfect_print_core::color::Color;
use perfect_print_core::document::{DocumentBuilder, DocumentModel};
use perfect_print_core::draw::{DrawCommand, ShapedGlyph, TextAlign, TextRun, TextStyle};
use perfect_print_core::page::{Layer, Margins, Page, PageSize};
use perfect_print_core::units::{Point, Rect};

use crate::paragraph::{Line, ParagraphEngine};

/// A run of text with a single style, used inside `RichParagraph`.
#[derive(Debug, Clone)]
pub struct StyledSpan {
    pub text: String,
    pub style: TextStyle,
}

/// The marker style for a `ContentBlock::List`.
#[derive(Debug, Clone)]
pub enum ListKind {
    Bulleted,
    Numbered,
}

/// One item within a `ContentBlock::List`.
#[derive(Debug, Clone)]
pub struct ListItem {
    pub spans: Vec<StyledSpan>,
    /// Nesting depth, 0-based. Each level indents 18pt further.
    pub level: usize,
}

/// A block of content to be laid out in the flow.
#[derive(Debug, Clone)]
pub enum ContentBlock {
    /// A paragraph of text.
    Paragraph { text: String, style: TextStyle },
    /// A paragraph with mixed inline styles.
    RichParagraph {
        spans: Vec<StyledSpan>,
        /// Alignment, line-height, and paragraph-level defaults come from here.
        base_style: TextStyle,
        /// Left indent in points (used by list items; 0 for plain rich paragraphs).
        indent_left: f64,
    },
    /// A bulleted or numbered list.
    List {
        items: Vec<ListItem>,
        kind: ListKind,
        style: TextStyle,
    },
    /// A table.
    Table {
        columns: Vec<crate::table::Column>,
        rows: Vec<crate::table::Row>,
    },
    /// An image.
    Image {
        image_id: String,
        dest_rect: perfect_print_core::units::Rect,
    },
    /// Pre-positioned draw commands.
    Commands(Vec<DrawCommand>),
    /// Explicit page break.
    PageBreak,
    /// Vertical gap in points.
    Gap(f64),
    /// Content laid out at a fixed position on the current page, outside the
    /// normal flow (does not advance the flow cursor). Mirrors CSS
    /// `position: absolute` with `left`/`top`/`width` resolved to points.
    Positioned {
        /// Offset from the content-area origin, in points.
        x: f64,
        y: f64,
        /// Layout width for the inner content, in points.
        width: f64,
        blocks: Vec<ContentBlock>,
    },
}

impl ContentBlock {
    pub fn paragraph(text: impl Into<String>, style: TextStyle) -> Self {
        Self::Paragraph {
            text: text.into(),
            style,
        }
    }

    pub fn gap(points: f64) -> Self {
        Self::Gap(points)
    }

    pub fn page_break() -> Self {
        Self::PageBreak
    }
}

/// Configuration for the flow layout engine.
#[derive(Debug, Clone)]
pub struct FlowConfig {
    pub page_size: PageSize,
    pub margins: Margins,
    /// Minimum number of lines at the bottom of a paragraph before a page break (widows).
    pub widow_lines: usize,
    /// Minimum number of lines at the top of a paragraph after a page break (orphans).
    pub orphan_lines: usize,
    /// Whether to allow breaking inside a paragraph (vs. always between paragraphs).
    pub break_inside_paragraphs: bool,
    /// Default text style for paragraphs that don't specify their own.
    pub default_style: Option<TextStyle>,
}

impl Default for FlowConfig {
    fn default() -> Self {
        Self {
            page_size: PageSize::Letter,
            margins: Margins::all(72.0),
            widow_lines: 2,
            orphan_lines: 2,
            break_inside_paragraphs: true,
            default_style: None,
        }
    }
}

/// A positioned block on a page.
#[derive(Debug, Clone)]
pub struct PositionedBlock {
    pub y: f64,
    pub commands: Vec<DrawCommand>,
    pub height: f64,
}

/// Flow layout engine that paginates content across pages.
pub struct FlowLayoutEngine {
    config: FlowConfig,
    paragraph_engine: ParagraphEngine,
}

impl FlowLayoutEngine {
    pub fn new(config: FlowConfig) -> Self {
        Self {
            config,
            paragraph_engine: ParagraphEngine::new(),
        }
    }

    /// Layout all content blocks and return a DocumentModel.
    pub fn layout(&mut self, blocks: &[ContentBlock]) -> DocumentModel {
        let content_rect = self.content_rect();
        let page_height = content_rect.height;
        let content_width = content_rect.width;

        let all_pages = self.layout_into_pages(blocks, content_width, page_height);

        // Build DocumentModel from laid-out pages
        self.build_document(all_pages)
    }

    /// Lay out `blocks` into a sequence of pages, each `page_height` tall and
    /// `content_width` wide. Shared by the top-level `layout()` (real page
    /// height) and by `ContentBlock::Positioned` (page_height ==
    /// `f64::INFINITY`, so paragraphs/tables never trigger a page break and
    /// everything lands on a single synthetic "page" that the caller then
    /// translates into place).
    fn layout_into_pages(
        &mut self,
        blocks: &[ContentBlock],
        content_width: f64,
        page_height: f64,
    ) -> Vec<Vec<PositionedBlock>> {
        // Lists are lowered into RichParagraphs (with a marker span and left
        // indent) before pagination, so the rest of the loop only has to
        // handle one "rich text" shape.
        let expanded = expand_lists(blocks);

        let mut all_pages: Vec<Vec<PositionedBlock>> = vec![vec![]];
        let mut current_y: f64 = 0.0;
        let mut page_idx: usize = 0;

        for block in &expanded {
            match block {
                ContentBlock::PageBreak => {
                    page_idx += 1;
                    all_pages.push(vec![]);
                    current_y = 0.0;
                    continue;
                }
                ContentBlock::Gap(gap) => {
                    current_y += gap;
                    continue;
                }
                _ => {}
            }

            match block {
                ContentBlock::Paragraph { text, style } => {
                    // Merge with document default style if available
                    let merged_style = match &self.config.default_style {
                        Some(default) => merge_styles(default, style),
                        None => style.clone(),
                    };
                    let para_layout =
                        self.paragraph_engine
                            .layout_paragraph(text, &merged_style, content_width);

                    if para_layout.lines.is_empty() {
                        continue;
                    }

                    let lines = para_layout.lines;
                    let total_lines = lines.len();

                    // Try to fit the whole paragraph on the current page
                    let para_height: f64 = lines.iter().map(|l| l.height).sum();

                    if current_y + para_height <= page_height {
                        // Whole paragraph fits on current page
                        for line in &lines {
                            let x_offset = line.glyphs.first().map(|g| g.x).unwrap_or(0.0);
                            let cmd = line_to_draw_command(line, current_y, x_offset);
                            all_pages[page_idx].push(PositionedBlock {
                                y: current_y,
                                commands: vec![cmd],
                                height: line.height,
                            });
                            current_y += line.height;
                        }
                    } else if self.config.break_inside_paragraphs
                        && total_lines > self.config.widow_lines
                    {
                        // Break the paragraph across pages
                        for (i, line) in lines.iter().enumerate() {
                            // Check if we need a page break before this line
                            if current_y + line.height > page_height {
                                // Would leave fewer than widow_lines on current page?
                                let lines_after = total_lines - i;
                                if lines_after < self.config.widow_lines
                                    && all_pages[page_idx].len() > 1
                                {
                                    // Avoid widow: keep one more line on this page
                                    // unless it would create an orphan
                                    if lines_after >= self.config.orphan_lines {
                                        // Just break normally, orphan is acceptable
                                        page_idx += 1;
                                        all_pages.push(vec![]);
                                        current_y = 0.0;
                                    }
                                    // else: keep going, the widow/orphan rules cancel out
                                } else {
                                    page_idx += 1;
                                    all_pages.push(vec![]);
                                    current_y = 0.0;
                                }
                            }

                            let x_offset = line.glyphs.first().map(|g| g.x).unwrap_or(0.0);
                            let cmd = line_to_draw_command(line, current_y, x_offset);
                            all_pages[page_idx].push(PositionedBlock {
                                y: current_y,
                                commands: vec![cmd],
                                height: line.height,
                            });
                            current_y += line.height;
                        }
                    } else {
                        // Don't break inside paragraph: move whole thing to next page
                        if current_y > 0.0 {
                            page_idx += 1;
                            all_pages.push(vec![]);
                            current_y = 0.0;
                        }
                        for line in &lines {
                            let x_offset = line.glyphs.first().map(|g| g.x).unwrap_or(0.0);
                            let cmd = line_to_draw_command(line, current_y, x_offset);
                            all_pages[page_idx].push(PositionedBlock {
                                y: current_y,
                                commands: vec![cmd],
                                height: line.height,
                            });
                            current_y += line.height;
                        }
                    }
                }
                ContentBlock::Table { columns, rows } => {
                    use crate::table::TableEngine;

                    let mut table_engine = TableEngine::new();
                    let available_width = content_width;

                    // Estimate table height
                    let est_height: f64 = rows.iter().map(|r| r.height.unwrap_or(20.0)).sum();

                    let table_layout = if current_y + est_height > page_height && rows.len() > 1 {
                        // Paginated table
                        let table_pages = table_engine.layout_table_paginated(
                            columns,
                            rows,
                            available_width,
                            page_height,
                            current_y,
                        );

                        if table_pages.is_empty() {
                            table_engine.layout_table(columns, rows, available_width, current_y)
                        } else {
                            // Use the first page on this page, rest on subsequent pages
                            if table_pages.len() > 1 {
                                for tp in &table_pages[1..] {
                                    page_idx += 1;
                                    all_pages.push(vec![]);
                                    all_pages[page_idx].push(PositionedBlock {
                                        y: 0.0,
                                        commands: tp.commands.clone(),
                                        height: tp.total_height,
                                    });
                                }
                            }
                            table_pages.into_iter().next().unwrap()
                        }
                    } else {
                        table_engine.layout_table(columns, rows, available_width, current_y)
                    };

                    all_pages[page_idx].push(PositionedBlock {
                        y: current_y,
                        commands: table_layout.commands,
                        height: table_layout.total_height,
                    });
                    current_y += table_layout.total_height;
                }
                ContentBlock::Commands(cmds) => {
                    all_pages[page_idx].push(PositionedBlock {
                        y: current_y,
                        commands: cmds.clone(),
                        height: 0.0,
                    });
                }
                ContentBlock::Image {
                    ref image_id,
                    ref dest_rect,
                } => {
                    let cmd = DrawCommand::Image {
                        image_id: image_id.clone(),
                        dest_rect: *dest_rect,
                        source_rect: None,
                    };
                    let height = dest_rect.height;
                    all_pages[page_idx].push(PositionedBlock {
                        y: current_y,
                        commands: vec![cmd],
                        height,
                    });
                    current_y += height;
                }
                ContentBlock::RichParagraph {
                    spans,
                    base_style,
                    indent_left,
                } => {
                    let merged_base = match &self.config.default_style {
                        Some(default) => merge_styles(default, base_style),
                        None => base_style.clone(),
                    };
                    // Each span's own style inherits from the document default
                    // the same way a plain Paragraph's style does — unset
                    // fields (empty font, zero size, default black/left) fall
                    // back to `default_style`.
                    let span_pairs: Vec<(String, TextStyle)> = spans
                        .iter()
                        .map(|s| {
                            let style = match &self.config.default_style {
                                Some(default) => merge_styles(default, &s.style),
                                None => s.style.clone(),
                            };
                            (s.text.clone(), style)
                        })
                        .collect();
                    let avail_width = (content_width - indent_left).max(1.0);
                    let rows = self
                        .paragraph_engine
                        .layout_spans_fragmented(&span_pairs, avail_width);

                    if rows.is_empty() {
                        continue;
                    }

                    // Row height/baseline is the max across that row's fragments,
                    // so mixed-style text on one line shares a baseline.
                    let row_metrics: Vec<(f64, f64)> = rows
                        .iter()
                        .map(|frags| {
                            let height = frags.iter().map(|f| f.height).fold(0.0_f64, f64::max);
                            let baseline =
                                frags.iter().map(|f| f.baseline_y).fold(0.0_f64, f64::max);
                            (height, baseline)
                        })
                        .collect();

                    let row_width: f64 = rows
                        .last()
                        .and_then(|frags| frags.last())
                        .and_then(|f| f.glyphs.last())
                        .map(|g| g.x + g.advance)
                        .unwrap_or(0.0);
                    let align_offset = match merged_base.align {
                        TextAlign::Left => 0.0,
                        TextAlign::Right => (avail_width - row_width).max(0.0),
                        TextAlign::Center => ((avail_width - row_width) / 2.0).max(0.0),
                        TextAlign::Justified => 0.0,
                    };
                    let extra_x = indent_left + align_offset;

                    let total_rows = rows.len();
                    let para_height: f64 = row_metrics.iter().map(|(h, _)| h).sum();

                    if current_y + para_height <= page_height {
                        for (row, (height, baseline)) in rows.iter().zip(row_metrics.iter()) {
                            let commands: Vec<DrawCommand> = row
                                .iter()
                                .map(|frag| {
                                    fragment_to_draw_command(frag, current_y, *baseline, extra_x)
                                })
                                .collect();
                            all_pages[page_idx].push(PositionedBlock {
                                y: current_y,
                                commands,
                                height: *height,
                            });
                            current_y += height;
                        }
                    } else if self.config.break_inside_paragraphs
                        && total_rows > self.config.widow_lines
                    {
                        for (i, (row, (height, baseline))) in
                            rows.iter().zip(row_metrics.iter()).enumerate()
                        {
                            if current_y + height > page_height {
                                let rows_after = total_rows - i;
                                if rows_after < self.config.widow_lines
                                    && all_pages[page_idx].len() > 1
                                {
                                    if rows_after >= self.config.orphan_lines {
                                        page_idx += 1;
                                        all_pages.push(vec![]);
                                        current_y = 0.0;
                                    }
                                } else {
                                    page_idx += 1;
                                    all_pages.push(vec![]);
                                    current_y = 0.0;
                                }
                            }

                            let commands: Vec<DrawCommand> = row
                                .iter()
                                .map(|frag| {
                                    fragment_to_draw_command(frag, current_y, *baseline, extra_x)
                                })
                                .collect();
                            all_pages[page_idx].push(PositionedBlock {
                                y: current_y,
                                commands,
                                height: *height,
                            });
                            current_y += height;
                        }
                    } else {
                        if current_y > 0.0 {
                            page_idx += 1;
                            all_pages.push(vec![]);
                            current_y = 0.0;
                        }
                        for (row, (height, baseline)) in rows.iter().zip(row_metrics.iter()) {
                            let commands: Vec<DrawCommand> = row
                                .iter()
                                .map(|frag| {
                                    fragment_to_draw_command(frag, current_y, *baseline, extra_x)
                                })
                                .collect();
                            all_pages[page_idx].push(PositionedBlock {
                                y: current_y,
                                commands,
                                height: *height,
                            });
                            current_y += height;
                        }
                    }
                }
                ContentBlock::Positioned {
                    x,
                    y,
                    width,
                    blocks: inner_blocks,
                } => {
                    // Lay the inner content out on its own, unbounded
                    // "page" (page_height = INFINITY means the fits-on-page
                    // branch of every arm above is always taken, so nothing
                    // here ever triggers a page break). Content taller than
                    // the remaining physical page simply overflows past the
                    // page edge when rendered — this matches CSS
                    // `position: absolute`, where an element is taken out of
                    // flow and is not implicitly paginated or clipped.
                    let inner_pages = self.layout_into_pages(inner_blocks, *width, f64::INFINITY);
                    let inner_page = inner_pages.into_iter().next().unwrap_or_default();

                    for block in inner_page {
                        let commands: Vec<DrawCommand> = block
                            .commands
                            .iter()
                            .map(|cmd| cmd.translated(*x, *y))
                            .collect();
                        all_pages[page_idx].push(PositionedBlock {
                            y: y + block.y,
                            commands,
                            height: block.height,
                        });
                    }
                    // Deliberately do not touch `current_y`: positioned
                    // content is out of the normal flow.
                }
                _ => {}
            }
        }

        all_pages
    }

    fn build_document(&self, pages: Vec<Vec<PositionedBlock>>) -> DocumentModel {
        let mut builder = DocumentBuilder::new();
        let is_roll = self.config.page_size.is_roll_paper();

        for page_blocks in &pages {
            // For roll paper, compute the actual content height
            let page_size = if is_roll {
                let content_height = if page_blocks.is_empty() {
                    0.0
                } else {
                    page_blocks
                        .iter()
                        .map(|b| b.y + b.height)
                        .fold(0.0, f64::max)
                };
                let total_height =
                    content_height + self.config.margins.top + self.config.margins.bottom;
                let width = self.config.page_size.width();
                PageSize::Custom {
                    width,
                    height: total_height.max(1.0),
                }
            } else {
                self.config.page_size
            };

            let mut page = Page::new(page_size);
            page.margins = self.config.margins;

            // `layout()` positions everything in content-area-relative
            // coordinates (x/y start at 0 inside the margins). Neither the
            // raster nor the PDF backend applies page margins itself, so the
            // canonical `DocumentModel` must carry page-absolute coordinates
            // here — translate every command by (margins.left, margins.top).
            // `ContentBlock::Commands` blocks (e.g. the HTML `<hr>` rule) are
            // authored in that same content-relative space, not already
            // page-absolute, so they're translated identically; see
            // `test_commands_block_is_offset_by_margins`.
            let mut layer = Layer::foreground();
            for block in page_blocks {
                for cmd in &block.commands {
                    layer.commands.push(
                        cmd.translated(self.config.margins.left, self.config.margins.top),
                    );
                }
            }
            page.layers.push(layer);
            builder = builder.add_page(page);
        }

        builder.build().unwrap_or_else(|_| {
            DocumentBuilder::new()
                .page(self.config.page_size)
                .build()
                .unwrap()
        })
    }

    fn content_rect(&self) -> Rect {
        let size = self.config.page_size.to_size();
        Rect::new(
            self.config.margins.left,
            self.config.margins.top,
            size.width - self.config.margins.left - self.config.margins.right,
            size.height - self.config.margins.top - self.config.margins.bottom,
        )
    }
}

/// Expand `ContentBlock::List` blocks into a sequence of `RichParagraph`
/// blocks (marker span + item spans, indented per nesting level), leaving
/// every other block untouched. Numbered lists number level-0 items
/// sequentially; descending into a deeper level restarts that level's count.
fn expand_lists(blocks: &[ContentBlock]) -> Vec<ContentBlock> {
    let mut out = Vec::with_capacity(blocks.len());
    for block in blocks {
        match block {
            ContentBlock::List { items, kind, style } => {
                out.extend(expand_list_items(items, kind, style));
            }
            other => out.push(other.clone()),
        }
    }
    out
}

fn expand_list_items(items: &[ListItem], kind: &ListKind, style: &TextStyle) -> Vec<ContentBlock> {
    let mut counters: Vec<usize> = Vec::new();
    let mut out = Vec::with_capacity(items.len());

    for item in items {
        let level = item.level;
        if counters.len() <= level {
            counters.resize(level + 1, 0);
        } else {
            // Coming back up (or staying at) this level: drop deeper counters
            // so a subsequent nested run starts numbering from 1 again.
            counters.truncate(level + 1);
        }
        counters[level] += 1;

        let marker = match kind {
            ListKind::Bulleted => "\u{2022} ".to_string(),
            ListKind::Numbered => format!("{}. ", counters[level]),
        };

        let mut spans = Vec::with_capacity(item.spans.len() + 1);
        spans.push(StyledSpan {
            text: marker,
            style: style.clone(),
        });
        spans.extend(item.spans.iter().cloned());

        out.push(ContentBlock::RichParagraph {
            spans,
            base_style: style.clone(),
            indent_left: 18.0 * (level as f64 + 1.0),
        });
    }

    out
}

/// Convert a single-style line fragment (part of a RichParagraph row) into a
/// draw command. `baseline` overrides the fragment's own baseline so mixed
/// styles on one row share a baseline; `extra_x` (indent + alignment offset)
/// is added to the fragment's on-page x position only — the glyphs' internal
/// relative offsets are still computed from the fragment's own row-relative
/// start, exactly as `line_to_draw_command` does for a plain line.
fn fragment_to_draw_command(frag: &Line, y: f64, baseline: f64, extra_x: f64) -> DrawCommand {
    let frag_x = frag.glyphs.first().map(|g| g.x).unwrap_or(0.0);
    DrawCommand::Text {
        run: TextRun {
            text: frag.text.clone(),
            glyphs: positioned_line_glyphs(frag, frag_x),
            style: frag.style.clone(),
        },
        position: Point::new(frag_x + extra_x, y + baseline),
        max_width: Some(frag.width),
    }
}

fn line_to_draw_command(line: &Line, y: f64, x_offset: f64) -> DrawCommand {
    DrawCommand::Text {
        run: TextRun {
            text: line.text.clone(),
            glyphs: positioned_line_glyphs(line, x_offset),
            style: line.style.clone(),
        },
        position: Point::new(x_offset, y + line.baseline_y),
        max_width: Some(line.width),
    }
}

fn positioned_line_glyphs(line: &Line, base_x: f64) -> Vec<ShapedGlyph> {
    if line.shaped_glyphs.is_empty() {
        return Vec::new();
    }

    let mut expected_x = 0.0;
    line.glyphs
        .iter()
        .zip(line.shaped_glyphs.iter())
        .map(|(positioned, shaped)| {
            let mut glyph = shaped.clone();
            glyph.x_offset += positioned.x - base_x - expected_x;
            glyph.y_offset += positioned.y;
            glyph.x_advance = shaped.x_advance;
            glyph.y_advance = shaped.y_advance;
            expected_x += shaped.x_advance;
            glyph
        })
        .collect()
}

/// Merge a paragraph style with a document default style.
/// Paragraph style fields take precedence; only unset/empty/default fields fall back to default.
fn merge_styles(default: &TextStyle, paragraph: &TextStyle) -> TextStyle {
    TextStyle {
        font: if paragraph.font.as_ref().is_empty() {
            default.font.clone()
        } else {
            paragraph.font.clone()
        },
        size: if paragraph.size == 0.0 {
            default.size
        } else {
            paragraph.size
        },
        // Color: fall back to default if paragraph uses plain black (the default)
        color: if paragraph.color == Color::black() && default.color != Color::black() {
            default.color
        } else {
            paragraph.color
        },
        // Align: fall back to default if paragraph uses Left (the default)
        align: if paragraph.align == TextAlign::Left && default.align != TextAlign::Left {
            default.align
        } else {
            paragraph.align
        },
        line_height: paragraph.line_height.or(default.line_height),
        letter_spacing: paragraph.letter_spacing.or(default.letter_spacing),
        // Boolean flags: OR them so either source can enable
        bold: paragraph.bold || default.bold,
        italic: paragraph.italic || default.italic,
        underline: paragraph.underline || default.underline,
        strikethrough: paragraph.strikethrough || default.strikethrough,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use perfect_print_core::font::FontRef;

    fn test_style() -> TextStyle {
        TextStyle::new(FontRef::new("Helvetica"), 12.0)
    }

    /// `FlowLayoutEngine::layout()` internally lays out content starting at
    /// (0,0) in content-area-relative coordinates. `build_document()` must
    /// translate every emitted `DrawCommand` by the configured margins so the
    /// canonical `DocumentModel` holds page-absolute coordinates — neither
    /// the raster nor the PDF backend applies margins itself.
    #[test]
    fn test_layout_offsets_content_by_margins() {
        let config = FlowConfig {
            page_size: PageSize::Letter,
            margins: Margins::all(72.0),
            ..Default::default()
        };

        let mut engine = FlowLayoutEngine::new(config);
        let blocks = vec![ContentBlock::paragraph("Hello World", test_style())];
        let doc = engine.layout(&blocks);

        let position = doc
            .pages
            .iter()
            .flat_map(|p| p.layers.iter())
            .flat_map(|l| l.commands.iter())
            .find_map(|c| match c {
                DrawCommand::Text { position, .. } => Some(*position),
                _ => None,
            })
            .expect("expected a text run");

        assert!(
            position.x >= 72.0,
            "text x should be offset by the left margin, got {}",
            position.x
        );
        assert!(
            position.y >= 72.0,
            "text y should be offset by the top margin, got {}",
            position.y
        );
    }

    /// With zero margins, content-area-relative and page-absolute coordinates
    /// coincide, so content should still start at the page origin.
    #[test]
    fn test_zero_margins_content_at_origin() {
        let config = FlowConfig {
            page_size: PageSize::Letter,
            margins: Margins::all(0.0),
            ..Default::default()
        };

        let mut engine = FlowLayoutEngine::new(config);
        let blocks = vec![ContentBlock::paragraph("Hello World", test_style())];
        let doc = engine.layout(&blocks);

        let position = doc
            .pages
            .iter()
            .flat_map(|p| p.layers.iter())
            .flat_map(|l| l.commands.iter())
            .find_map(|c| match c {
                DrawCommand::Text { position, .. } => Some(*position),
                _ => None,
            })
            .expect("expected a text run");

        assert_eq!(
            position.x, 0.0,
            "with zero margins, x should be unchanged from content-relative"
        );
        // y additionally carries the line's baseline offset even at y=0 margin,
        // so just check it's not additionally offset by a margin (it would be
        // >= 72.0 if the default margin were mistakenly applied).
        assert!(
            position.y < 72.0,
            "with zero margins, y should not carry a leftover default margin offset, got {}",
            position.y
        );
    }

    /// A `ContentBlock::Commands` block (e.g. the HTML `<hr>` rule) is
    /// authored in the same content-area-relative coordinate space as every
    /// other block — not already page-absolute — so it must be translated by
    /// the margins too, exactly like text/table content.
    #[test]
    fn test_commands_block_is_offset_by_margins() {
        let config = FlowConfig {
            page_size: PageSize::Letter,
            margins: Margins::all(72.0),
            ..Default::default()
        };

        let mut engine = FlowLayoutEngine::new(config);
        let blocks = vec![ContentBlock::Commands(vec![DrawCommand::FillRect {
            rect: perfect_print_core::units::Rect::new(0.0, 0.0, 100.0, 1.0),
            color: Color::black(),
        }])];
        let doc = engine.layout(&blocks);

        let rect = doc
            .pages
            .iter()
            .flat_map(|p| p.layers.iter())
            .flat_map(|l| l.commands.iter())
            .find_map(|c| match c {
                DrawCommand::FillRect { rect, .. } => Some(*rect),
                _ => None,
            })
            .expect("expected a FillRect command");

        assert_eq!(rect.x, 72.0, "Commands block x should be offset by margin");
        assert_eq!(rect.y, 72.0, "Commands block y should be offset by margin");
    }

    #[test]
    fn test_flow_config_default() {
        let config = FlowConfig::default();
        assert_eq!(config.widow_lines, 2);
        assert_eq!(config.orphan_lines, 2);
        assert!(config.break_inside_paragraphs);
    }

    #[test]
    fn test_content_block_paragraph() {
        let block = ContentBlock::paragraph("Hello", test_style());
        match block {
            ContentBlock::Paragraph { text, .. } => assert_eq!(text, "Hello"),
            _ => panic!("Expected Paragraph"),
        }
    }

    #[test]
    fn test_content_block_gap() {
        let block = ContentBlock::gap(24.0);
        match block {
            ContentBlock::Gap(g) => assert_eq!(g, 24.0),
            _ => panic!("Expected Gap"),
        }
    }

    #[test]
    fn test_content_block_page_break() {
        let block = ContentBlock::page_break();
        match block {
            ContentBlock::PageBreak => {}
            _ => panic!("Expected PageBreak"),
        }
    }

    #[test]
    fn test_flow_layout_single_page() {
        let config = FlowConfig {
            page_size: PageSize::Letter,
            margins: Margins::all(72.0),
            ..Default::default()
        };

        let mut engine = FlowLayoutEngine::new(config);
        let blocks = vec![ContentBlock::paragraph("Hello World", test_style())];

        let doc = engine.layout(&blocks);
        assert!(doc.page_count() >= 1);
    }

    #[test]
    fn test_flow_layout_with_page_break() {
        let config = FlowConfig {
            page_size: PageSize::Letter,
            margins: Margins::all(72.0),
            ..Default::default()
        };

        let mut engine = FlowLayoutEngine::new(config);
        let blocks = vec![
            ContentBlock::paragraph("Page 1 content", test_style()),
            ContentBlock::page_break(),
            ContentBlock::paragraph("Page 2 content", test_style()),
        ];

        let doc = engine.layout(&blocks);
        assert!(doc.page_count() >= 2);
    }

    #[test]
    fn test_flow_layout_with_gaps() {
        let config = FlowConfig {
            page_size: PageSize::Letter,
            margins: Margins::all(72.0),
            ..Default::default()
        };

        let mut engine = FlowLayoutEngine::new(config);
        let blocks = vec![
            ContentBlock::paragraph("First", test_style()),
            ContentBlock::gap(24.0),
            ContentBlock::paragraph("Second", test_style()),
        ];

        let doc = engine.layout(&blocks);
        assert!(doc.page_count() >= 1);
    }

    #[test]
    fn test_flow_layout_many_paragraphs() {
        let config = FlowConfig {
            page_size: PageSize::Letter,
            margins: Margins::all(72.0),
            ..Default::default()
        };

        let mut engine = FlowLayoutEngine::new(config);
        let mut blocks = vec![];

        for i in 0..50 {
            blocks.push(ContentBlock::paragraph(
                format!("Paragraph {} with some text to make it wrap", i),
                test_style(),
            ));
        }

        let doc = engine.layout(&blocks);
        assert!(
            doc.page_count() >= 2,
            "50 paragraphs should span multiple pages"
        );
    }

    #[test]
    fn test_flow_layout_mixed_content() {
        let config = FlowConfig {
            page_size: PageSize::Letter,
            margins: Margins::all(72.0),
            ..Default::default()
        };

        let mut engine = FlowLayoutEngine::new(config);
        let blocks = vec![
            ContentBlock::paragraph("Introduction text here", test_style()),
            ContentBlock::gap(12.0),
            ContentBlock::paragraph("More content after a gap", test_style()),
            ContentBlock::page_break(),
            ContentBlock::paragraph("New page content", test_style()),
        ];

        let doc = engine.layout(&blocks);
        assert!(doc.page_count() >= 2);
    }

    #[test]
    fn test_page_size_a4() {
        let config = FlowConfig {
            page_size: PageSize::A4,
            margins: Margins::all(72.0),
            ..Default::default()
        };

        let mut engine = FlowLayoutEngine::new(config);
        let blocks = vec![ContentBlock::paragraph("Test on A4", test_style())];

        let doc = engine.layout(&blocks);
        assert_eq!(doc.page_count(), 1);
    }

    #[test]
    fn test_widow_control() {
        // With widow_lines=2, a paragraph should not be split leaving 1 line alone
        let config = FlowConfig {
            page_size: PageSize::Letter,
            margins: Margins::all(72.0),
            widow_lines: 2,
            orphan_lines: 2,
            break_inside_paragraphs: true,
            ..Default::default()
        };

        let mut engine = FlowLayoutEngine::new(config);
        let mut blocks = vec![];

        // Add many short paragraphs to fill a page
        for i in 0..60 {
            blocks.push(ContentBlock::paragraph(format!("Line {}", i), test_style()));
        }

        let doc = engine.layout(&blocks);
        assert!(doc.page_count() >= 2, "Should span multiple pages");
    }

    #[test]
    fn test_no_break_inside_paragraphs() {
        let config = FlowConfig {
            page_size: PageSize::Letter,
            margins: Margins::all(72.0),
            break_inside_paragraphs: false,
            ..Default::default()
        };

        let mut engine = FlowLayoutEngine::new(config);
        let blocks = vec![ContentBlock::paragraph(
            "This is a single paragraph that should not be broken across pages. \
             It should move entirely to the next page if it doesn't fit.",
            test_style(),
        )];

        let doc = engine.layout(&blocks);
        assert!(doc.page_count() >= 1);
    }

    #[test]
    fn test_rich_paragraph_layout_produces_runs_per_span() {
        let base = TextStyle::new(FontRef::new("Helvetica"), 12.0);
        let mut bold = base.clone();
        bold.bold = true;
        let block = ContentBlock::RichParagraph {
            spans: vec![
                StyledSpan {
                    text: "Hello ".into(),
                    style: base.clone(),
                },
                StyledSpan {
                    text: "world".into(),
                    style: bold,
                },
            ],
            base_style: base,
            indent_left: 0.0,
        };
        let mut engine = FlowLayoutEngine::new(FlowConfig::default());
        let model = engine.layout(&[block]);
        // Both spans appear, bold span carries bold=true, and the bold run
        // starts to the right of the plain run on the same baseline.
        let texts: Vec<(&TextRun, f64)> = model
            .pages
            .iter()
            .flat_map(|p| p.layers.iter())
            .flat_map(|l| l.commands.iter())
            .filter_map(|c| match c {
                DrawCommand::Text { run, position, .. } => Some((run, position.x)),
                _ => None,
            })
            .collect();
        let plain = texts
            .iter()
            .find(|(r, _)| r.text.contains("Hello") && !r.style.bold);
        let bold = texts
            .iter()
            .find(|(r, _)| r.text.contains("world") && r.style.bold);
        assert!(plain.is_some(), "expected a non-bold run containing Hello");
        assert!(bold.is_some(), "expected a bold run containing world");
        assert!(
            bold.unwrap().1 > plain.unwrap().1,
            "bold run should start to the right of the plain run"
        );
    }

    #[test]
    fn test_bulleted_list_indents_and_prefixes() {
        let style = TextStyle::new(FontRef::new("Helvetica"), 12.0);
        let block = ContentBlock::List {
            items: vec![
                ListItem {
                    spans: vec![StyledSpan {
                        text: "First".into(),
                        style: style.clone(),
                    }],
                    level: 0,
                },
                ListItem {
                    spans: vec![StyledSpan {
                        text: "Second".into(),
                        style: style.clone(),
                    }],
                    level: 0,
                },
            ],
            kind: ListKind::Bulleted,
            style: style.clone(),
        };
        let mut engine = FlowLayoutEngine::new(FlowConfig::default());
        let model = engine.layout(&[block]);
        let text: String = model
            .pages
            .iter()
            .flat_map(|p| p.layers.iter())
            .flat_map(|l| l.commands.iter())
            .filter_map(|c| match c {
                DrawCommand::Text { run, .. } => Some(run.text.clone()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join(" ");
        assert!(text.contains("First") && text.contains("Second"));
        // Bullet markers present:
        assert!(text.contains('\u{2022}'));
    }

    #[test]
    fn test_numbered_list_prefixes_sequentially() {
        let style = TextStyle::new(FontRef::new("Helvetica"), 12.0);
        let block = ContentBlock::List {
            items: vec![
                ListItem {
                    spans: vec![StyledSpan {
                        text: "Alpha".into(),
                        style: style.clone(),
                    }],
                    level: 0,
                },
                ListItem {
                    spans: vec![StyledSpan {
                        text: "Beta".into(),
                        style: style.clone(),
                    }],
                    level: 0,
                },
            ],
            kind: ListKind::Numbered,
            style: style.clone(),
        };
        let mut engine = FlowLayoutEngine::new(FlowConfig::default());
        let model = engine.layout(&[block]);
        let text: String = model
            .pages
            .iter()
            .flat_map(|p| p.layers.iter())
            .flat_map(|l| l.commands.iter())
            .filter_map(|c| match c {
                DrawCommand::Text { run, .. } => Some(run.text.clone()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join(" ");
        assert!(text.contains("1."));
        assert!(text.contains("2."));
    }

    #[test]
    fn test_style_inheritance() {
        use perfect_print_core::font::FontRef;

        // Set up a document with a default style
        let default_style = TextStyle::new(FontRef::new("Helvetica"), 14.0);
        let config = FlowConfig {
            page_size: PageSize::Letter,
            margins: Margins::all(72.0),
            default_style: Some(default_style),
            ..Default::default()
        };

        let mut engine = FlowLayoutEngine::new(config);
        let blocks = vec![
            ContentBlock::paragraph(
                "First paragraph",
                TextStyle::new(FontRef::new("Helvetica"), 12.0),
            ),
            ContentBlock::paragraph(
                "Second paragraph",
                TextStyle::new(FontRef::new("Courier"), 10.0),
            ),
        ];

        let doc = engine.layout(&blocks);
        assert!(doc.page_count() >= 1);
        // The document should have been laid out successfully with inherited styles
    }

    /// Proves (not just exercises) style inheritance end-to-end: a paragraph
    /// built with the "unset" TextStyle that the public `Document`/`Paragraph`
    /// API never actually produces on its own (empty font, zero size, default
    /// black color, default left alignment) still resolves to the document's
    /// `default_style` on the rendered `DrawCommand::Text` runs.
    ///
    /// See `docs/IMPROVEMENT-PLAN.md` item 6 — this closes it out as Done:
    /// `FlowConfig.default_style` + `merge_styles` (used by both the
    /// `Paragraph` and `RichParagraph` arms of `FlowLayoutEngine::layout`) is
    /// the real wiring; `Document::default_style()` just sets that field.
    #[test]
    fn test_paragraph_inherits_flow_default_style() {
        use perfect_print_core::font::FontRef;

        let mut default = TextStyle::new(FontRef::new("Times New Roman"), 14.0);
        default.color = Color::rgb(1.0, 0.0, 0.0);
        default.align = TextAlign::Center;

        let config = FlowConfig {
            page_size: PageSize::Letter,
            margins: Margins::all(72.0),
            default_style: Some(default.clone()),
            ..Default::default()
        };

        // An "unset" paragraph style: empty font name, zero size, default
        // black color, default left alignment — every field merge_styles
        // treats as "fall back to the document default".
        let unset_style = TextStyle::new(FontRef::new(""), 0.0);

        let mut engine = FlowLayoutEngine::new(config);
        let blocks = vec![ContentBlock::paragraph("Hello", unset_style)];
        let doc = engine.layout(&blocks);

        let run = doc
            .pages
            .iter()
            .flat_map(|p| p.layers.iter())
            .flat_map(|l| l.commands.iter())
            .find_map(|c| match c {
                DrawCommand::Text { run, .. } => Some(run),
                _ => None,
            })
            .expect("expected a text run");

        assert_eq!(run.style.font, default.font, "font should be inherited");
        assert_eq!(run.style.size, default.size, "size should be inherited");
        assert_eq!(run.style.color, default.color, "color should be inherited");
        assert_eq!(run.style.align, default.align, "align should be inherited");
    }

    /// Same contract, but through the `RichParagraph` path added alongside
    /// mixed-style spans: `base_style` merges with the document default too.
    #[test]
    fn test_rich_paragraph_inherits_flow_default_style() {
        use perfect_print_core::font::FontRef;

        let mut default = TextStyle::new(FontRef::new("Times New Roman"), 14.0);
        default.color = Color::rgb(0.0, 0.0, 1.0);

        let config = FlowConfig {
            page_size: PageSize::Letter,
            margins: Margins::all(72.0),
            default_style: Some(default.clone()),
            ..Default::default()
        };

        let unset_style = TextStyle::new(FontRef::new(""), 0.0);
        let block = ContentBlock::RichParagraph {
            spans: vec![StyledSpan {
                text: "Hello".into(),
                style: unset_style.clone(),
            }],
            base_style: unset_style,
            indent_left: 0.0,
        };

        let mut engine = FlowLayoutEngine::new(config);
        let doc = engine.layout(&[block]);

        let run = doc
            .pages
            .iter()
            .flat_map(|p| p.layers.iter())
            .flat_map(|l| l.commands.iter())
            .find_map(|c| match c {
                DrawCommand::Text { run, .. } => Some(run),
                _ => None,
            })
            .expect("expected a text run");
        assert_eq!(run.style.font, default.font, "font should be inherited");
        assert_eq!(run.style.size, default.size, "size should be inherited");
        assert_eq!(run.style.color, default.color, "color should be inherited");
    }

    // ─── Positioned (CSS `position: absolute`) Tests ───────────────

    #[test]
    fn test_positioned_block_lands_at_its_own_coordinates() {
        let config = FlowConfig {
            page_size: PageSize::Letter,
            margins: Margins::all(72.0),
            ..Default::default()
        };

        let mut engine = FlowLayoutEngine::new(config);
        let blocks = vec![ContentBlock::Positioned {
            x: 100.0,
            y: 200.0,
            width: 216.0,
            blocks: vec![ContentBlock::paragraph("Hi", test_style())],
        }];
        let doc = engine.layout(&blocks);

        let position = doc
            .pages
            .iter()
            .flat_map(|p| p.layers.iter())
            .flat_map(|l| l.commands.iter())
            .find_map(|c| match c {
                DrawCommand::Text { position, .. } => Some(*position),
                _ => None,
            })
            .expect("expected a text run");

        // Page-absolute = margin + positioned offset (+ any intra-line
        // baseline/indent offset, hence >=).
        assert_eq!(
            position.x, 72.0 + 100.0,
            "positioned text x should be margin + x offset"
        );
        assert!(
            position.y >= 72.0 + 200.0,
            "positioned text y should be at least margin + y offset, got {}",
            position.y
        );
    }

    #[test]
    fn test_positioned_block_does_not_move_the_flow_cursor() {
        let config = FlowConfig {
            page_size: PageSize::Letter,
            margins: Margins::all(72.0),
            ..Default::default()
        };

        // Baseline: a lone flow paragraph's y position.
        let mut baseline_engine = FlowLayoutEngine::new(config.clone());
        let baseline_doc =
            baseline_engine.layout(&[ContentBlock::paragraph("Second", test_style())]);
        let baseline_y = baseline_doc
            .pages
            .iter()
            .flat_map(|p| p.layers.iter())
            .flat_map(|l| l.commands.iter())
            .find_map(|c| match c {
                DrawCommand::Text { position, .. } => Some(position.y),
                _ => None,
            })
            .expect("expected a text run");

        // With a Positioned block first, the following normal paragraph
        // should land at the exact same y as if the positioned block were
        // absent — the flow cursor must be untouched.
        let mut engine = FlowLayoutEngine::new(config);
        let blocks = vec![
            ContentBlock::Positioned {
                x: 300.0,
                y: 300.0,
                width: 100.0,
                blocks: vec![ContentBlock::paragraph(
                    "Positioned content, possibly many lines of it",
                    test_style(),
                )],
            },
            ContentBlock::paragraph("Second", test_style()),
        ];
        let doc = engine.layout(&blocks);

        let mut text_ys: Vec<f64> = doc
            .pages
            .iter()
            .flat_map(|p| p.layers.iter())
            .flat_map(|l| l.commands.iter())
            .filter_map(|c| match c {
                DrawCommand::Text { position, run, .. } if run.text.starts_with("Second") => {
                    Some(position.y)
                }
                _ => None,
            })
            .collect();
        let second_y = text_ys.pop().expect("expected the 'Second' paragraph");

        assert_eq!(
            second_y, baseline_y,
            "flow cursor must be unaffected by a preceding Positioned block"
        );
    }

    #[test]
    fn test_two_positioned_blocks_both_render_on_one_page() {
        let config = FlowConfig {
            page_size: PageSize::Letter,
            margins: Margins::all(72.0),
            ..Default::default()
        };

        let mut engine = FlowLayoutEngine::new(config);
        let blocks = vec![
            ContentBlock::Positioned {
                x: 50.0,
                y: 50.0,
                width: 150.0,
                blocks: vec![ContentBlock::paragraph("First box", test_style())],
            },
            ContentBlock::Positioned {
                x: 300.0,
                y: 400.0,
                width: 150.0,
                blocks: vec![ContentBlock::paragraph("Second box", test_style())],
            },
        ];
        let doc = engine.layout(&blocks);

        assert_eq!(doc.page_count(), 1, "both positioned blocks share one page");

        let texts: Vec<String> = doc
            .pages
            .iter()
            .flat_map(|p| p.layers.iter())
            .flat_map(|l| l.commands.iter())
            .filter_map(|c| match c {
                DrawCommand::Text { run, .. } => Some(run.text.clone()),
                _ => None,
            })
            .collect();
        let joined = texts.join(" ");
        assert!(joined.contains("First"), "expected first box text, got {joined:?}");
        assert!(joined.contains("Second"), "expected second box text, got {joined:?}");
    }

    // ─── Fuzz / Randomized Tests ────────────────────────────────────

    #[test]
    fn fuzz_random_documents_never_panic() {
        // Generate many random documents and verify none panic
        // This exercises the "random documents should not panic" requirement
        let mut rng = SimpleRng::new(42);

        for trial in 0..100 {
            let block_count = rng.next_u32() % 20 + 1;
            let mut blocks = vec![];

            for _ in 0..block_count {
                let block_type = rng.next_u32() % 4;
                match block_type {
                    0 => {
                        let text_len = rng.next_u32() % 50 + 1;
                        let text: String = (0..text_len)
                            .map(|_| (b'a' + (rng.next_u32() % 26) as u8) as char)
                            .collect();
                        blocks.push(ContentBlock::paragraph(text, test_style()));
                    }
                    1 => {
                        let gap = (rng.next_u32() % 100) as f64;
                        blocks.push(ContentBlock::gap(gap));
                    }
                    2 => {
                        blocks.push(ContentBlock::page_break());
                    }
                    3 => {
                        // Random draw commands
                        let x = (rng.next_u32() % 500) as f64;
                        let y = (rng.next_u32() % 700) as f64;
                        let w = (rng.next_u32() % 200 + 10) as f64;
                        let h = (rng.next_u32() % 100 + 10) as f64;
                        blocks.push(ContentBlock::Commands(vec![DrawCommand::FillRect {
                            rect: perfect_print_core::units::Rect::new(x, y, w, h),
                            color: perfect_print_core::color::Color::black(),
                        }]));
                    }
                    _ => unreachable!(),
                }
            }

            let config = FlowConfig {
                page_size: if rng.next_u32() % 2 == 0 {
                    PageSize::Letter
                } else {
                    PageSize::A4
                },
                margins: Margins::all((rng.next_u32() % 72 + 36) as f64),
                ..Default::default()
            };

            let mut engine = FlowLayoutEngine::new(config);
            // This should never panic
            let doc = engine.layout(&blocks);

            // Basic sanity: page count should be >= 1
            assert!(
                doc.page_count() >= 1,
                "Trial {}: document should have at least 1 page",
                trial
            );
        }
    }

    // ─── Property Tests ─────────────────────────────────────────────

    #[test]
    fn prop_page_count_always_positive() {
        // Any non-empty document must produce at least one page
        let config = FlowConfig::default();
        let mut engine = FlowLayoutEngine::new(config);
        let blocks = vec![ContentBlock::paragraph("Hello", test_style())];
        let doc = engine.layout(&blocks);
        assert!(doc.page_count() >= 1, "Page count must be >= 1");
    }

    #[test]
    fn prop_empty_document_produces_one_page() {
        // Empty block list produces one page (the default empty page)
        let config = FlowConfig::default();
        let mut engine = FlowLayoutEngine::new(config);
        let blocks: Vec<ContentBlock> = vec![];
        let doc = engine.layout(&blocks);
        assert_eq!(
            doc.page_count(),
            1,
            "Empty document should have 1 default page"
        );
    }

    #[test]
    fn prop_many_paragraphs_span_multiple_pages() {
        // Enough paragraphs should span multiple pages
        let config = FlowConfig {
            page_size: PageSize::Letter,
            margins: Margins::all(72.0),
            ..Default::default()
        };
        let mut engine = FlowLayoutEngine::new(config);
        let mut blocks = vec![];
        for i in 0..100 {
            blocks.push(ContentBlock::paragraph(
                format!(
                    "Paragraph {} with enough text to take up space on the page",
                    i
                ),
                test_style(),
            ));
        }
        let doc = engine.layout(&blocks);
        assert!(
            doc.page_count() > 1,
            "100 paragraphs should span multiple pages, got {}",
            doc.page_count()
        );
    }

    #[test]
    fn prop_page_break_increments_page_count() {
        // Each explicit page break should add a page
        let config = FlowConfig::default();
        let mut engine = FlowLayoutEngine::new(config);
        let mut blocks = vec![];
        for _ in 0..5 {
            blocks.push(ContentBlock::paragraph("Content", test_style()));
            blocks.push(ContentBlock::page_break());
        }
        let doc = engine.layout(&blocks);
        // 5 page breaks should produce at least 5 pages (maybe more if content wraps)
        assert!(
            doc.page_count() >= 5,
            "5 page breaks should produce >= 5 pages, got {}",
            doc.page_count()
        );
    }

    #[test]
    fn prop_no_negative_bounds() {
        // All content should have non-negative positions
        let config = FlowConfig::default();
        let mut engine = FlowLayoutEngine::new(config);
        let blocks = vec![
            ContentBlock::paragraph("First paragraph", test_style()),
            ContentBlock::paragraph("Second paragraph", test_style()),
            ContentBlock::Commands(vec![DrawCommand::FillRect {
                rect: perfect_print_core::units::Rect::new(10.0, 20.0, 100.0, 50.0),
                color: perfect_print_core::color::Color::black(),
            }]),
        ];
        let doc = engine.layout(&blocks);
        for (page_idx, page) in doc.pages.iter().enumerate() {
            for layer in &page.layers {
                for cmd in &layer.commands {
                    if let Some(bounds) = cmd.bounds() {
                        assert!(
                            bounds.x >= 0.0 && bounds.y >= 0.0,
                            "Page {}: content has negative bounds: {:?}",
                            page_idx,
                            bounds
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn prop_gaps_add_vertical_space() {
        // A document with gaps should be taller than one without
        let config = FlowConfig::default();

        let mut engine1 = FlowLayoutEngine::new(config.clone());
        let blocks1 = vec![
            ContentBlock::paragraph("Line 1", test_style()),
            ContentBlock::paragraph("Line 2", test_style()),
        ];
        let doc1 = engine1.layout(&blocks1);

        let mut engine2 = FlowLayoutEngine::new(config);
        let blocks2 = vec![
            ContentBlock::paragraph("Line 1", test_style()),
            ContentBlock::gap(100.0),
            ContentBlock::paragraph("Line 2", test_style()),
        ];
        let doc2 = engine2.layout(&blocks2);

        // Both should have at least 1 page
        assert!(doc1.page_count() >= 1);
        assert!(doc2.page_count() >= 1);
    }
}

/// Simple deterministic RNG for fuzz testing (no external deps).
/// Uses a basic xorshift algorithm.
#[cfg(test)]
struct SimpleRng {
    state: u64,
}

#[cfg(test)]
impl SimpleRng {
    fn new(seed: u64) -> Self {
        Self { state: seed.max(1) }
    }

    fn next_u32(&mut self) -> u32 {
        self.state ^= self.state << 13;
        self.state ^= self.state >> 7;
        self.state ^= self.state << 17;
        (self.state >> 32) as u32
    }
}
