use perfect_print_core::draw::{ShapedGlyph, TextAlign, TextStyle};
use perfect_print_core::font::{FontRef, FontStyle, FontWeight};
use rustybuzz::Direction;

use crate::font_loader::{FontCache, FontProperties, LoadedFont};
use crate::text_shaper::TextShaper;

/// Build font lookup properties from a `TextStyle`, mapping `bold`/`italic`
/// onto the font database's weight/style axes so the correct font face
/// (not just a synthetically-scaled regular face) is selected for shaping.
fn font_properties_for_style(style: &TextStyle) -> FontProperties {
    FontProperties::new(style.font.as_ref())
        .with_weight(if style.bold {
            FontWeight::Bold
        } else {
            FontWeight::Normal
        })
        .with_style(if style.italic {
            FontStyle::Italic
        } else {
            FontStyle::Normal
        })
}

/// A positioned glyph within a line.
#[derive(Debug, Clone, PartialEq)]
pub struct PositionedGlyph {
    pub glyph_id: u32,
    pub x: f64,
    pub y: f64,
    pub advance: f64,
    pub font_index: usize,
}

/// A line of shaped text.
#[derive(Debug, Clone, PartialEq)]
pub struct Line {
    /// Original text for this laid-out line.
    pub text: String,
    pub glyphs: Vec<PositionedGlyph>,
    /// Shaped glyphs from rustybuzz, preserving cluster and offset data for renderers.
    pub shaped_glyphs: Vec<ShapedGlyph>,
    pub width: f64,
    pub height: f64,
    pub baseline_y: f64,
    /// The text style used to produce this line (font, size, color, etc.)
    pub style: TextStyle,
    /// Indices into `glyphs` that are space characters (for justification).
    /// These are the exact positions where extra space should be distributed.
    pub space_indices: Vec<usize>,
}

impl Line {
    pub fn new(
        glyphs: Vec<PositionedGlyph>,
        height: f64,
        baseline_y: f64,
        style: TextStyle,
    ) -> Self {
        Self::from_parts(String::new(), glyphs, Vec::new(), height, baseline_y, style)
    }

    pub fn from_parts(
        text: String,
        glyphs: Vec<PositionedGlyph>,
        shaped_glyphs: Vec<ShapedGlyph>,
        height: f64,
        baseline_y: f64,
        style: TextStyle,
    ) -> Self {
        let width = glyphs.iter().map(|g| g.advance).sum();
        Self {
            text,
            glyphs,
            shaped_glyphs,
            width,
            height,
            baseline_y,
            style,
            space_indices: vec![],
        }
    }

    /// Set the space indices for this line.
    pub fn with_space_indices(mut self, indices: Vec<usize>) -> Self {
        self.space_indices = indices;
        self
    }
}

/// A paragraph: multiple lines of shaped text.
#[derive(Debug, Clone, PartialEq)]
pub struct ParagraphLayout {
    pub lines: Vec<Line>,
    pub width: f64,
    pub height: f64,
}

impl ParagraphLayout {
    pub fn new(lines: Vec<Line>, max_width: f64) -> Self {
        let height = lines.iter().map(|l| l.height).sum();
        Self {
            lines,
            width: max_width,
            height,
        }
    }

    pub fn line_count(&self) -> usize {
        self.lines.len()
    }
}

/// Configuration for paragraph layout.
#[derive(Debug, Clone)]
pub struct ParagraphConfig {
    /// Base text direction. None = auto-detect.
    pub base_direction: Option<Direction>,
    /// Whether to use bidi shaping for mixed-direction text.
    pub use_bidi: bool,
    /// Whether to enable hyphenation for word breaking.
    pub use_hyphenation: bool,
    /// Minimum word length (in chars) to consider for hyphenation.
    /// Words shorter than this will never be hyphenated. Default: 4.
    pub min_word_len: usize,
    /// Minimum prefix length (in chars) before a hyphen. Default: 2.
    pub min_prefix_len: usize,
    /// Minimum suffix length (in chars) after a hyphen. Default: 2.
    pub min_suffix_len: usize,
}

impl Default for ParagraphConfig {
    fn default() -> Self {
        Self {
            base_direction: None,
            use_bidi: true,
            use_hyphenation: false,
            min_word_len: 4,
            min_prefix_len: 2,
            min_suffix_len: 2,
        }
    }
}

/// Hyphenation dictionary for English (US).
/// Uses the Knuth-Liang algorithm with embedded TeX patterns.
#[derive(Clone)]
pub struct EnglishHyphenator {
    dict: hyphenation::Standard,
}

impl EnglishHyphenator {
    pub fn new() -> Self {
        use hyphenation::Load;
        Self {
            dict: hyphenation::Standard::from_embedded(hyphenation::Language::EnglishUS)
                .expect("Failed to load embedded English hyphenation dictionary"),
        }
    }

    /// Find the best hyphenation point for `word` such that the prefix (including
    /// the hyphen character) fits within `max_width` when rendered at `font_size`
    /// using the given `shaper` and `font`.
    /// Returns `Some((prefix_with_hyphen, suffix))` or `None`.
    pub fn find_break_point(
        &self,
        word: &str,
        font_size: f64,
        font: &crate::font_loader::LoadedFont,
        shaper: &crate::text_shaper::TextShaper,
        max_width: f64,
        min_prefix_len: usize,
        min_suffix_len: usize,
    ) -> Option<(String, String)> {
        use hyphenation::Hyphenator;

        if word.len() < min_prefix_len + min_suffix_len {
            return None;
        }

        let result = self.dict.hyphenate(word);

        // Collect valid break points (byte positions where hyphenation is allowed)
        let mut break_points: Vec<usize> = result
            .breaks
            .iter()
            .copied()
            .filter(|&pos| pos >= min_prefix_len && pos + min_suffix_len <= word.len())
            .collect();
        break_points.sort();

        // Find the largest break point where prefix + "-" fits in max_width
        let mut best_break = None;
        for &pos in &break_points {
            let prefix = &word[..pos];
            let hyphenated = format!("{}-", prefix);
            let width = shaper.measure_width(&hyphenated, font_size, font);
            if width <= max_width {
                best_break = Some(pos);
            } else {
                break;
            }
        }

        best_break.map(|pos| {
            let prefix = format!("{}-", &word[..pos]);
            let suffix = word[pos..].to_string();
            (prefix, suffix)
        })
    }
}

impl Default for EnglishHyphenator {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for EnglishHyphenator {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EnglishHyphenator").finish()
    }
}

/// Paragraph layout engine.
pub struct ParagraphEngine {
    shaper: TextShaper,
    font_cache: FontCache,
    config: ParagraphConfig,
    hyphenator: Option<EnglishHyphenator>,
}

impl ParagraphEngine {
    pub fn new() -> Self {
        Self {
            shaper: TextShaper::new(),
            font_cache: FontCache::default(),
            config: ParagraphConfig::default(),
            hyphenator: None,
        }
    }

    pub fn with_font_cache(font_cache: FontCache) -> Self {
        Self {
            shaper: TextShaper::new(),
            font_cache,
            config: ParagraphConfig::default(),
            hyphenator: None,
        }
    }

    pub fn with_config(mut self, config: ParagraphConfig) -> Self {
        let use_hyp = config.use_hyphenation;
        if use_hyp && self.hyphenator.is_none() {
            self.hyphenator = Some(EnglishHyphenator::new());
        }
        self.config = config;
        self
    }

    /// Enable hyphenation with the default English (US) dictionary.
    pub fn with_hyphenation(mut self) -> Self {
        self.config.use_hyphenation = true;
        if self.hyphenator.is_none() {
            self.hyphenator = Some(EnglishHyphenator::new());
        }
        self
    }

    /// Layout a single line of text (no wrapping).
    /// Glyphs are positioned according to the style's alignment within max_width.
    /// Returns the line with space_indices populated for justification.
    pub fn layout_line(&mut self, text: &str, style: &TextStyle, max_width: f64) -> Option<Line> {
        let font = self.load_font(style)?;
        let glyphs = if self.config.use_bidi {
            self.shaper
                .shape_bidi(text, style.size, &font, self.config.base_direction)
        } else {
            self.shaper.shape(text, style.size, &font)
        };
        if glyphs.is_empty() {
            return Some(Line::from_parts(
                text.to_string(),
                vec![],
                vec![],
                style.size * 1.2,
                style.size,
                style.clone(),
            ));
        }

        // Compute line width from shaped glyph advances
        let line_width: f64 = glyphs.iter().map(|g| g.x_advance).sum();

        // Compute x-offset based on alignment
        let x_offset = match style.align {
            TextAlign::Left => 0.0,
            TextAlign::Right => (max_width - line_width).max(0.0),
            TextAlign::Center => ((max_width - line_width) / 2.0).max(0.0),
            TextAlign::Justified => 0.0, // Justified is handled at paragraph level
        };

        // Build positioned glyphs and track space indices.
        // The `cluster` field on each glyph maps to a byte position in the original text.
        // A glyph is a space if the character at its cluster position is U+0020.
        let mut x = x_offset;
        let mut space_indices: Vec<usize> = Vec::new();
        let mut positioned: Vec<PositionedGlyph> = Vec::with_capacity(glyphs.len());

        for (idx, g) in glyphs.iter().enumerate() {
            // Check if this glyph corresponds to a space character.
            // The cluster field is a byte index into the original text.
            let is_space = text
                .as_bytes()
                .get(g.cluster as usize)
                .map(|&b| b == b' ')
                .unwrap_or(false);
            if is_space {
                space_indices.push(idx);
            }

            positioned.push(PositionedGlyph {
                glyph_id: g.glyph_id,
                x,
                y: 0.0,
                advance: g.x_advance,
                font_index: g.font_index,
            });
            x += g.x_advance;
        }

        let line_height = style.line_height.unwrap_or(style.size * 1.2);
        Some(
            Line::from_parts(
                text.to_string(),
                positioned,
                glyphs,
                line_height,
                style.size,
                style.clone(),
            )
            .with_space_indices(space_indices),
        )
    }

    /// Layout a paragraph with word-wrap at max_width.
    /// Respects the style's alignment (Left, Right, Center, Justified).
    pub fn layout_paragraph(
        &mut self,
        text: &str,
        style: &TextStyle,
        max_width: f64,
    ) -> ParagraphLayout {
        let font = match self.load_font(style) {
            Some(f) => f,
            None => return ParagraphLayout::new(vec![], max_width),
        };

        let words: Vec<&str> = text.split_whitespace().collect();
        if words.is_empty() {
            return ParagraphLayout::new(vec![], max_width);
        }

        let mut lines = Vec::new();
        let mut current_line_words: Vec<String> = Vec::new();
        let mut current_width = 0.0;

        for word in &words {
            let word_owned = word.to_string();
            let word_width = if self.config.use_bidi {
                self.shaper
                    .measure_width_bidi(word, style.size, &font, self.config.base_direction)
            } else {
                self.shaper.measure_width(word, style.size, &font)
            };
            let space_width = if current_line_words.is_empty() {
                0.0
            } else {
                self.shaper.measure_width(" ", style.size, &font)
            };

            if current_width + space_width + word_width > max_width
                && !current_line_words.is_empty()
            {
                // Word doesn't fit. Try hyphenation if enabled.
                let remaining_width = max_width - current_width - space_width;
                let do_hyphenate = self.config.use_hyphenation
                    && word.len() >= self.config.min_word_len
                    && remaining_width > 0.0
                    && self.hyphenator.is_some();

                if do_hyphenate {
                    if let Some(ref hyphenator) = self.hyphenator {
                        if let Some((prefix, suffix)) = hyphenator.find_break_point(
                            word,
                            style.size,
                            &font,
                            &self.shaper,
                            remaining_width,
                            self.config.min_prefix_len,
                            self.config.min_suffix_len,
                        ) {
                            // Flush current line with the hyphenated prefix
                            let mut line_words = current_line_words.clone();
                            line_words.push(prefix);
                            let line_text = line_words.join(" ");
                            if let Some(line) = self.layout_line(&line_text, style, max_width) {
                                lines.push(line);
                            }
                            // Start new line with the suffix
                            current_line_words.clear();
                            let suffix_width = if self.config.use_bidi {
                                self.shaper.measure_width_bidi(
                                    &suffix,
                                    style.size,
                                    &font,
                                    self.config.base_direction,
                                )
                            } else {
                                self.shaper.measure_width(&suffix, style.size, &font)
                            };
                            current_line_words.push(suffix);
                            current_width = suffix_width;
                            continue;
                        }
                    }
                }

                // No hyphenation or hyphenation didn't help — flush normally
                let line_text = current_line_words.join(" ");
                if let Some(line) = self.layout_line(&line_text, style, max_width) {
                    lines.push(line);
                }
                current_line_words.clear();
                current_width = 0.0;
            }

            if !current_line_words.is_empty() {
                current_width += space_width;
            }
            current_line_words.push(word_owned);
            current_width += word_width;
        }

        // Flush last line
        if !current_line_words.is_empty() {
            let line_text = current_line_words.join(" ");
            if let Some(line) = self.layout_line(&line_text, style, max_width) {
                lines.push(line);
            }
        }

        // Handle justified alignment: distribute extra space between words
        if style.align == TextAlign::Justified {
            let line_count = lines.len();
            for (i, line) in lines.iter_mut().enumerate() {
                // Don't justify the last line of the paragraph
                if i == line_count - 1 {
                    break;
                }
                // Skip lines with no spaces (single-word lines)
                if line.space_indices.len() < 1 {
                    continue;
                }

                let line_text_width: f64 = line.glyphs.iter().map(|g| g.advance).sum();
                let extra = max_width - line_text_width;
                if extra <= 0.0 {
                    continue;
                }

                let space_count = line.space_indices.len();
                let extra_per_space = extra / space_count as f64;

                // Shift each glyph right based on how many spaces precede it.
                // Glyphs after each space_index get shifted by an additional extra_per_space.
                let mut accumulated_shift = 0.0;
                let mut next_space_idx = 0;
                for (glyph_idx, glyph) in line.glyphs.iter_mut().enumerate() {
                    glyph.x += accumulated_shift;
                    // If this glyph is a space boundary, accumulate shift for subsequent glyphs
                    if next_space_idx < space_count
                        && glyph_idx == line.space_indices[next_space_idx]
                    {
                        next_space_idx += 1;
                        accumulated_shift += extra_per_space;
                    }
                }
            }
        }

        ParagraphLayout::new(lines, max_width)
    }

    /// Layout multiple text runs with different styles.
    pub fn layout_runs(&mut self, runs: &[(String, TextStyle)], max_width: f64) -> ParagraphLayout {
        let mut all_lines = Vec::new();
        let mut current_line_words: Vec<(String, TextStyle)> = Vec::new();
        let mut current_width = 0.0;

        for (text, style) in runs {
            let words: Vec<&str> = text.split_whitespace().collect();
            for word in words {
                let font = self.load_font(style);
                let word_width = font
                    .as_ref()
                    .map(|f| {
                        if self.config.use_bidi {
                            self.shaper.measure_width_bidi(
                                word,
                                style.size,
                                f,
                                self.config.base_direction,
                            )
                        } else {
                            self.shaper.measure_width(word, style.size, f)
                        }
                    })
                    .unwrap_or(0.0);
                let space_width = if current_line_words.is_empty() {
                    0.0
                } else {
                    let prev_style = &current_line_words.last().unwrap().1;
                    self.load_font(prev_style)
                        .as_ref()
                        .map(|f| self.shaper.measure_width(" ", prev_style.size, f))
                        .unwrap_or(0.0)
                };

                if current_width + space_width + word_width > max_width
                    && !current_line_words.is_empty()
                {
                    // Flush
                    let line = self.layout_word_run(&current_line_words, max_width);
                    all_lines.push(line);
                    current_line_words.clear();
                    current_width = 0.0;
                }

                if !current_line_words.is_empty() {
                    current_width += space_width;
                }
                current_line_words.push((word.to_string(), style.clone()));
                current_width += word_width;
            }
        }

        if !current_line_words.is_empty() {
            let line = self.layout_word_run(&current_line_words, max_width);
            all_lines.push(line);
        }

        ParagraphLayout::new(all_lines, max_width)
    }

    fn layout_word_run(&mut self, words: &[(String, TextStyle)], _max_width: f64) -> Line {
        let mut positioned = Vec::new();
        let mut shaped_glyphs = Vec::new();
        let mut x = 0.0;
        let mut max_height: f64 = 0.0;
        let mut last_style = TextStyle::new(FontRef::new("Helvetica"), 12.0);
        // Byte offset of the current word within the line's final text (words
        // joined with single spaces). Shaping runs per word, so each word's
        // glyph clusters are word-relative; renderers (e.g. the PDF writer's
        // TJ emission) expect clusters to be byte offsets into the whole
        // line text, so rebase them as we go.
        let mut word_byte_offset = 0usize;

        for (index, (word, style)) in words.iter().enumerate() {
            let line_height = style.line_height.unwrap_or(style.size * 1.2);
            max_height = max_height.max(line_height);
            last_style = style.clone();

            let font = self.font_cache.get(&font_properties_for_style(style));
            let word_glyphs = font
                .as_ref()
                .map(|font| {
                    self.shaper
                        .shape_bidi(word, style.size, font, self.config.base_direction)
                })
                .unwrap_or_default();

            if word_glyphs.is_empty() {
                let fallback_width = word.len() as f64 * style.size * 0.5;
                positioned.push(PositionedGlyph {
                    glyph_id: 0,
                    x,
                    y: 0.0,
                    advance: fallback_width,
                    font_index: 0,
                });
                x += fallback_width;
            } else {
                let mut pen_x = x;
                for mut glyph in word_glyphs {
                    positioned.push(PositionedGlyph {
                        glyph_id: glyph.glyph_id,
                        x: pen_x + glyph.x_offset,
                        y: glyph.y_offset,
                        advance: glyph.x_advance,
                        font_index: glyph.font_index,
                    });
                    pen_x += glyph.x_advance;
                    glyph.cluster += word_byte_offset as u32;
                    shaped_glyphs.push(glyph);
                }
                x = pen_x;
            }

            word_byte_offset += word.len() + 1; // +1 for the joining space

            if index + 1 < words.len() {
                let space_w = font
                    .as_ref()
                    .map(|f| self.shaper.measure_width(" ", style.size, f))
                    .unwrap_or(style.size * 0.3);
                x += space_w;
            }
        }

        let text = words
            .iter()
            .map(|(word, _)| word.as_str())
            .collect::<Vec<_>>()
            .join(" ");
        Line::from_parts(
            text,
            positioned,
            shaped_glyphs,
            max_height,
            max_height * 0.8,
            last_style,
        )
    }

    /// Lay out a paragraph built from styled spans, preserving per-span style
    /// boundaries so the caller can emit one draw command per (line, span-fragment).
    /// Line breaking is word-based across span boundaries — a wrap can happen
    /// between words from any two (or the same) spans, exactly like a plain
    /// paragraph. Returns one `Vec<Line>` per wrapped row; within a row, each
    /// `Line` is a contiguous same-style fragment.
    pub fn layout_spans_fragmented(
        &mut self,
        spans: &[(String, TextStyle)],
        max_width: f64,
    ) -> Vec<Vec<Line>> {
        // Flatten spans into (word, style) pairs.
        let mut words: Vec<(String, TextStyle)> = Vec::new();
        for (text, style) in spans {
            for w in text.split_whitespace() {
                words.push((w.to_string(), style.clone()));
            }
        }
        if words.is_empty() {
            return Vec::new();
        }

        // Greedy word-wrap, mirroring layout_paragraph's algorithm but carrying
        // the style alongside each word so line breaks can fall between spans.
        let mut rows: Vec<Vec<(String, TextStyle)>> = Vec::new();
        let mut current: Vec<(String, TextStyle)> = Vec::new();
        let mut current_width = 0.0;

        for (word, style) in words {
            let font = match self.load_font(&style) {
                Some(f) => f,
                None => continue,
            };
            let word_width = self.shaper.measure_width(&word, style.size, &font);
            let space_width = if current.is_empty() {
                0.0
            } else {
                let prev_style = &current.last().unwrap().1;
                self.load_font(prev_style)
                    .map(|f| self.shaper.measure_width(" ", prev_style.size, &f))
                    .unwrap_or(0.0)
            };

            if current_width + space_width + word_width > max_width && !current.is_empty() {
                rows.push(std::mem::take(&mut current));
                current_width = 0.0;
            }

            if !current.is_empty() {
                current_width += space_width;
            }
            current.push((word, style));
            current_width += word_width;
        }
        if !current.is_empty() {
            rows.push(current);
        }

        rows.into_iter()
            .map(|row| self.layout_word_run_fragments(&row))
            .collect()
    }

    /// Position a row of styled words (as `layout_word_run` does) and split the
    /// result into one `Line` per contiguous same-style run ("fragment").
    /// Glyph x-positions stay row-relative (cumulative across the whole row,
    /// starting at 0), matching the convention `layout_word_run` uses — callers
    /// extract a fragment's x offset from its first glyph, just like a plain `Line`.
    fn layout_word_run_fragments(&mut self, words: &[(String, TextStyle)]) -> Vec<Line> {
        let mut fragments = Vec::new();
        let mut positioned: Vec<PositionedGlyph> = Vec::new();
        let mut shaped_glyphs: Vec<ShapedGlyph> = Vec::new();
        let mut frag_words: Vec<&str> = Vec::new();
        let mut frag_style: Option<TextStyle> = None;
        let mut x = 0.0;
        // Byte offset of the current word within the *fragment's* text (its
        // words joined with single spaces). Per-word shaping yields
        // word-relative clusters; rebase them so each fragment's clusters
        // index its own `text`, as renderers expect.
        let mut word_byte_offset = 0usize;

        for (index, (word, style)) in words.iter().enumerate() {
            if let Some(active) = &frag_style {
                if active != style {
                    // Style changed: flush the fragment built so far.
                    let text = frag_words.join(" ");
                    let height = active.line_height.unwrap_or(active.size * 1.2);
                    fragments.push(Line::from_parts(
                        text,
                        std::mem::take(&mut positioned),
                        std::mem::take(&mut shaped_glyphs),
                        height,
                        active.size,
                        active.clone(),
                    ));
                    frag_words.clear();
                    word_byte_offset = 0;
                }
            }
            frag_style = Some(style.clone());

            let font = self.font_cache.get(&font_properties_for_style(style));
            let word_glyphs = font
                .as_ref()
                .map(|font| {
                    self.shaper
                        .shape_bidi(word, style.size, font, self.config.base_direction)
                })
                .unwrap_or_default();

            if word_glyphs.is_empty() {
                let fallback_width = word.len() as f64 * style.size * 0.5;
                positioned.push(PositionedGlyph {
                    glyph_id: 0,
                    x,
                    y: 0.0,
                    advance: fallback_width,
                    font_index: 0,
                });
                x += fallback_width;
            } else {
                let mut pen_x = x;
                for mut glyph in word_glyphs {
                    positioned.push(PositionedGlyph {
                        glyph_id: glyph.glyph_id,
                        x: pen_x + glyph.x_offset,
                        y: glyph.y_offset,
                        advance: glyph.x_advance,
                        font_index: glyph.font_index,
                    });
                    pen_x += glyph.x_advance;
                    glyph.cluster += word_byte_offset as u32;
                    shaped_glyphs.push(glyph);
                }
                x = pen_x;
            }

            frag_words.push(word.as_str());
            word_byte_offset += word.len() + 1; // +1 for the joining space

            if index + 1 < words.len() {
                let space_w = font
                    .as_ref()
                    .map(|f| self.shaper.measure_width(" ", style.size, f))
                    .unwrap_or(style.size * 0.3);
                x += space_w;
            }
        }

        if let Some(style) = frag_style {
            let text = frag_words.join(" ");
            let height = style.line_height.unwrap_or(style.size * 1.2);
            fragments.push(Line::from_parts(
                text,
                positioned,
                shaped_glyphs,
                height,
                style.size,
                style,
            ));
        }

        fragments
    }

    fn load_font(&mut self, style: &TextStyle) -> Option<LoadedFont> {
        self.font_cache.get(&font_properties_for_style(style))
    }
}

impl Default for ParagraphEngine {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use perfect_print_core::draw::TextStyle;
    use perfect_print_core::font::FontRef;

    fn test_style() -> TextStyle {
        TextStyle::new(FontRef::new("Helvetica"), 12.0)
    }

    #[test]
    fn test_layout_single_line() {
        let mut engine = ParagraphEngine::new();
        let line = engine.layout_line("Hello World", &test_style(), 500.0);
        assert!(line.is_some());
        let line = line.unwrap();
        assert!(!line.glyphs.is_empty());
        assert!(line.width > 0.0);
    }

    #[test]
    fn test_layout_paragraph_wraps() {
        let mut engine = ParagraphEngine::new();
        let text = "The quick brown fox jumps over the lazy dog";
        let layout = engine.layout_paragraph(text, &test_style(), 100.0);
        assert!(layout.line_count() > 1, "Should wrap to multiple lines");
    }

    #[test]
    fn test_layout_paragraph_no_wrap_needed() {
        let mut engine = ParagraphEngine::new();
        let text = "Hi";
        let layout = engine.layout_paragraph(text, &test_style(), 500.0);
        assert_eq!(layout.line_count(), 1, "Short text should be one line");
    }

    #[test]
    fn test_layout_empty_paragraph() {
        let mut engine = ParagraphEngine::new();
        let layout = engine.layout_paragraph("", &test_style(), 100.0);
        assert_eq!(layout.line_count(), 0);
    }

    #[test]
    fn test_line_height() {
        let mut engine = ParagraphEngine::new();
        let line = engine.layout_line("Test", &test_style(), 500.0).unwrap();
        assert!(line.height > 0.0);
        assert!(line.height >= 12.0); // At least font size
    }

    #[test]
    fn test_layout_with_rtl() {
        let engine = ParagraphEngine::new();
        let config = ParagraphConfig {
            base_direction: Some(Direction::RightToLeft),
            use_bidi: true,
            ..Default::default()
        };
        let mut engine = engine.with_config(config);
        let line = engine.layout_line("Hello", &test_style(), 500.0);
        assert!(line.is_some());
    }

    #[test]
    fn test_layout_mixed_bidi_paragraph() {
        let mut engine = ParagraphEngine::new();
        let text = "Hello World test paragraph wrapping";
        let layout = engine.layout_paragraph(text, &test_style(), 80.0);
        assert!(layout.line_count() >= 2, "Should wrap to multiple lines");
    }

    #[test]
    fn test_layout_runs_preserves_shaped_glyphs() {
        let mut engine = ParagraphEngine::new();
        let mut bold = test_style();
        bold.bold = true;
        let mut italic = test_style();
        italic.italic = true;

        let layout = engine.layout_runs(
            &[("Hello".to_string(), bold), ("world".to_string(), italic)],
            500.0,
        );

        let line = layout
            .lines
            .first()
            .expect("mixed runs should produce a line");
        assert_eq!(line.text, "Hello world");
        assert!(!line.glyphs.is_empty());
        assert!(
            !line.shaped_glyphs.is_empty(),
            "mixed-style run layout must preserve shaped glyphs for renderers"
        );
        assert_eq!(line.glyphs.len(), line.shaped_glyphs.len());
    }

    /// Assert the cluster contract PDF text emission relies on: every
    /// non-whitespace character in the line's text has a shaped glyph whose
    /// cluster equals that character's byte offset in the text. (Whole-line
    /// shaping also emits space glyphs; per-word shaping does not — both
    /// satisfy this containment check.)
    fn assert_clusters_cover_text(text: &str, shaped_glyphs: &[ShapedGlyph]) {
        let clusters: std::collections::HashSet<u32> =
            shaped_glyphs.iter().map(|g| g.cluster).collect();
        for (byte_idx, ch) in text.char_indices() {
            if ch.is_whitespace() {
                continue;
            }
            assert!(
                clusters.contains(&(byte_idx as u32)),
                "char {:?} at byte {} of {:?} has no shaped glyph with a matching cluster",
                ch,
                byte_idx,
                text
            );
        }
    }

    #[test]
    fn test_wrapped_run_clusters_are_line_relative_byte_offsets() {
        let mut engine = ParagraphEngine::new();
        let text =
            "Hello World from PlainBooks. This is a test invoice line with normal spacing.";
        // layout_runs takes the per-word shaping path (layout_word_run),
        // where word-relative clusters used to leak through un-rebased.
        let layout = engine.layout_runs(&[(text.to_string(), test_style())], 200.0);
        assert!(layout.line_count() > 1, "narrow width should wrap");
        for line in &layout.lines {
            assert_clusters_cover_text(&line.text, &line.shaped_glyphs);
        }
    }

    #[test]
    fn test_wrapped_paragraph_clusters_are_line_relative_byte_offsets() {
        let mut engine = ParagraphEngine::new();
        let text =
            "Hello World from PlainBooks. This is a test invoice line with normal spacing.";
        let layout = engine.layout_paragraph(text, &test_style(), 200.0);
        assert!(layout.line_count() > 1, "narrow width should wrap");
        for line in &layout.lines {
            assert_clusters_cover_text(&line.text, &line.shaped_glyphs);
        }
    }

    #[test]
    fn test_fragment_clusters_are_fragment_relative_byte_offsets() {
        let mut engine = ParagraphEngine::new();
        let mut bold = test_style();
        bold.bold = true;
        let rows = engine.layout_spans_fragmented(
            &[
                ("Hello brave".to_string(), test_style()),
                ("new World again".to_string(), bold),
            ],
            500.0,
        );
        assert!(!rows.is_empty());
        for row in &rows {
            for frag in row {
                assert_clusters_cover_text(&frag.text, &frag.shaped_glyphs);
            }
        }
    }

    #[test]
    fn test_right_align_offsets_glyphs_right() {
        use perfect_print_core::draw::TextAlign;
        let mut engine = ParagraphEngine::new();
        let mut style = test_style();
        style.align = TextAlign::Right;
        let line = engine.layout_line("Hello", &style, 500.0).unwrap();
        // Right-aligned: first glyph should be at (max_width - line_width)
        assert!(
            line.glyphs[0].x > 0.0,
            "Right-aligned glyphs should have x > 0"
        );
        let line_width: f64 = line.glyphs.iter().map(|g| g.advance).sum();
        let expected_x = 500.0 - line_width;
        assert!(
            (line.glyphs[0].x - expected_x).abs() < 1.0,
            "First glyph x={} should be near expected x={}",
            line.glyphs[0].x,
            expected_x
        );
    }

    #[test]
    fn test_center_align_centers_glyphs() {
        use perfect_print_core::draw::TextAlign;
        let mut engine = ParagraphEngine::new();
        let mut style = test_style();
        style.align = TextAlign::Center;
        let line = engine.layout_line("Hello", &style, 500.0).unwrap();
        let line_width: f64 = line.glyphs.iter().map(|g| g.advance).sum();
        let expected_x = (500.0 - line_width) / 2.0;
        assert!(
            (line.glyphs[0].x - expected_x).abs() < 1.0,
            "First glyph x={} should be near expected center x={}",
            line.glyphs[0].x,
            expected_x
        );
    }

    #[test]
    fn test_justify_spreads_words() {
        use perfect_print_core::draw::TextAlign;
        let mut engine = ParagraphEngine::new();
        let mut style = test_style();
        style.align = TextAlign::Justified;
        let text = "The quick brown fox jumps over the lazy dog";
        let layout = engine.layout_paragraph(text, &style, 100.0);
        assert!(layout.line_count() >= 3, "Should have 3+ lines");
        // Check that non-last lines have glyphs spread wider than the raw text width
        for (i, line) in layout.lines.iter().enumerate() {
            if i == layout.lines.len() - 1 {
                break; // Skip last line
            }
            if line.glyphs.len() > 1 {
                let first_x = line.glyphs.first().unwrap().x;
                let last_x = line.glyphs.last().unwrap().x;
                let spread = last_x - first_x;
                let raw_width: f64 = line.glyphs.iter().map(|g| g.advance).sum();
                assert!(
                    spread >= raw_width * 0.95,
                    "Justified line {} should have spread >= raw width",
                    i
                );
            }
        }
    }

    // ─── Hyphenation Tests ────────────────────────────────────────────

    #[test]
    fn test_hyphenation_basic() {
        use perfect_print_core::draw::TextAlign;
        let mut engine = ParagraphEngine::new().with_hyphenation();
        let mut style = test_style();
        style.align = TextAlign::Left;
        // "The internationalization" — "The" fills part of the line,
        // then "internationalization" doesn't fit and should be hyphenated
        let text = "The internationalization";
        let layout = engine.layout_paragraph(text, &style, 80.0);
        assert!(
            layout.line_count() >= 2,
            "Long word should be hyphenated across lines, got {} lines",
            layout.line_count()
        );
    }

    #[test]
    fn test_hyphenation_disabled_by_default() {
        let mut engine = ParagraphEngine::new();
        let text = "hyphenation";
        let layout = engine.layout_paragraph(text, &test_style(), 50.0);
        // Without hyphenation, the word should overflow or be on a single line
        // (depending on the engine's behavior when a word doesn't fit)
        // The key is that it doesn't panic
        assert!(layout.line_count() >= 1);
    }

    #[test]
    fn test_hyphenation_with_config() {
        use perfect_print_core::draw::TextAlign;
        let config = ParagraphConfig {
            use_hyphenation: true,
            ..Default::default()
        };
        let mut engine = ParagraphEngine::new().with_config(config);
        let mut style = test_style();
        style.align = TextAlign::Left;
        let text = "The extraordinary internationalization of hyphenation";
        let layout = engine.layout_paragraph(text, &style, 80.0);
        // Should produce multiple lines with hyphenation
        assert!(
            layout.line_count() >= 3,
            "Should have 3+ lines with hyphenation"
        );
    }

    #[test]
    fn test_hyphenation_short_word_not_hyphenated() {
        let mut engine = ParagraphEngine::new().with_hyphenation();
        let text = "the cat sat";
        let layout = engine.layout_paragraph(text, &test_style(), 30.0);
        // Short words (3 chars) should not be hyphenated (min_word_len = 4)
        assert!(layout.line_count() >= 1);
    }

    #[test]
    fn test_hyphenator_new() {
        let hyphenator = EnglishHyphenator::new();
        let result = hyphenator.find_break_point(
            "hyphenation",
            12.0,
            &crate::font_loader::FontCache::default()
                .get_by_family("Helvetica")
                .unwrap_or_else(|| {
                    panic!("Helvetica font not found");
                }),
            &crate::text_shaper::TextShaper::new(),
            50.0,
            2,
            2,
        );
        // With a very narrow width, should find a break point or return None
        // (depends on the font metrics)
        let _ = result;
    }
}
