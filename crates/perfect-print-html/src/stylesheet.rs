//! CSS rule storage, selector matching, specificity, and cascade.
//!
//! Supports `tag`, `.class`, `#id`, `tag.class` selectors (and comma lists
//! thereof), plus a hand-extracted `@page` rule. Unknown at-rules are
//! recorded as warnings rather than causing a hard failure.

use perfect_print_core::color::Color;
use perfect_print_core::page::{Margins, PageSize};

use crate::css::{parse_declarations, parse_length, Declaration};

/// Parse a CSS `margin`-shorthand value into `Margins`, supporting the
/// standard 1/2/3/4-value forms (`margin: 0.5in`, `margin: 1in 0.5in`,
/// `margin: 1in 0.5in 0.75in`, `margin: 1in 0.5in 0.75in 0.25in`), each
/// token parsed with [`parse_length`]. Returns `None` if any token fails to
/// parse (e.g. a `%` or unrecognized unit) or the token count isn't 1-4.
fn parse_margin_shorthand(value: &str) -> Option<Margins> {
    let tokens: Vec<f64> = value
        .split_whitespace()
        .map(|tok| parse_length(tok, 12.0))
        .collect::<Option<Vec<f64>>>()?;
    match tokens.as_slice() {
        [all] => Some(Margins::all(*all)),
        [vertical, horizontal] => Some(Margins::symmetric(*vertical, *horizontal)),
        [top, horizontal, bottom] => Some(Margins::new(*top, *horizontal, *bottom, *horizontal)),
        [top, right, bottom, left] => Some(Margins::new(*top, *right, *bottom, *left)),
        _ => None,
    }
}

/// A single compound selector: `tag`, `.class`, `#id`, or `tag.class`.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SimpleSelector {
    pub tag: Option<String>,
    pub class: Option<String>,
    pub id: Option<String>,
}

impl SimpleSelector {
    fn parse(input: &str) -> Option<Self> {
        let input = input.trim();
        if input.is_empty() {
            return None;
        }

        let mut tag = None;
        let mut class = None;
        let mut id = None;
        let mut rest = input;

        if let Some(pos) = rest.find(['.', '#']) {
            if pos > 0 {
                tag = Some(rest[..pos].to_ascii_lowercase());
            }
            rest = &rest[pos..];
        } else {
            tag = Some(rest.to_ascii_lowercase());
            rest = "";
        }

        while !rest.is_empty() {
            if let Some(stripped) = rest.strip_prefix('.') {
                let end = stripped.find(['.', '#']).unwrap_or(stripped.len());
                class = Some(stripped[..end].to_string());
                rest = &stripped[end..];
            } else if let Some(stripped) = rest.strip_prefix('#') {
                let end = stripped.find(['.', '#']).unwrap_or(stripped.len());
                id = Some(stripped[..end].to_string());
                rest = &stripped[end..];
            } else {
                break;
            }
        }

        if tag.is_none() && class.is_none() && id.is_none() {
            return None;
        }

        Some(Self { tag, class, id })
    }

    fn matches(&self, tag: &str, classes: &[String], id: Option<&str>) -> bool {
        if let Some(want_tag) = &self.tag {
            if !want_tag.eq_ignore_ascii_case(tag) {
                return false;
            }
        }
        if let Some(want_class) = &self.class {
            if !classes.iter().any(|c| c == want_class) {
                return false;
            }
        }
        if let Some(want_id) = &self.id {
            if id != Some(want_id.as_str()) {
                return false;
            }
        }
        true
    }

    /// id(100) > class(10) > tag(1).
    fn specificity(&self) -> u32 {
        let mut spec = 0;
        if self.id.is_some() {
            spec += 100;
        }
        if self.class.is_some() {
            spec += 10;
        }
        if self.tag.is_some() {
            spec += 1;
        }
        spec
    }
}

/// `size: letter|a4|legal|<w> <h>` from an `@page` rule.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PageSizeSpec {
    Letter,
    A4,
    Legal,
    Custom { width: f64, height: f64 },
}

impl PageSizeSpec {
    fn parse(value: &str) -> Option<Self> {
        let lower = value.trim().to_ascii_lowercase();
        match lower.as_str() {
            "letter" => Some(Self::Letter),
            "a4" => Some(Self::A4),
            "legal" => Some(Self::Legal),
            _ => {
                let parts: Vec<&str> = lower.split_whitespace().collect();
                if parts.len() == 2 {
                    let width = parse_length(parts[0], 12.0)?;
                    let height = parse_length(parts[1], 12.0)?;
                    Some(Self::Custom { width, height })
                } else {
                    None
                }
            }
        }
    }

    pub fn to_page_size(self) -> PageSize {
        match self {
            PageSizeSpec::Letter => PageSize::Letter,
            PageSizeSpec::A4 => PageSize::A4,
            PageSizeSpec::Legal => PageSize::Legal,
            PageSizeSpec::Custom { width, height } => PageSize::Custom { width, height },
        }
    }
}

/// The parsed body of an `@page { ... }` rule.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct PageRule {
    pub size: Option<PageSizeSpec>,
    /// From the `margin` shorthand (1/2/3/4-value CSS form) and/or the
    /// individual `margin-top`/`margin-right`/`margin-bottom`/`margin-left`
    /// longhands, cascaded in source order (a longhand after the shorthand
    /// overrides just that side, matching normal CSS cascade behavior).
    pub margin: Option<Margins>,
}

#[derive(Debug, Clone)]
struct Rule {
    selectors: Vec<SimpleSelector>,
    declarations: Vec<Declaration>,
    source_order: usize,
}

/// A parsed CSS subset: ordinary rules plus an optional `@page` rule.
#[derive(Debug, Clone, Default)]
pub struct Stylesheet {
    rules: Vec<Rule>,
    pub page_rule: Option<PageRule>,
    pub warnings: Vec<String>,
}

impl Stylesheet {
    pub fn empty() -> Self {
        Self::default()
    }

    /// Parse a CSS subset: comments are stripped, `@page` is extracted, and
    /// other at-rules are skipped with a warning (never a hard error).
    pub fn parse(css: &str) -> Self {
        let css = strip_comments(css);
        let chars: Vec<char> = css.chars().collect();

        let mut rules = Vec::new();
        let mut page_rule = None;
        let mut warnings = Vec::new();
        let mut source_order = 0usize;

        let mut i = 0usize;
        while i < chars.len() {
            while i < chars.len() && chars[i].is_whitespace() {
                i += 1;
            }
            if i >= chars.len() {
                break;
            }

            let header_start = i;
            let mut j = i;
            while j < chars.len() && chars[j] != '{' {
                j += 1;
            }
            if j >= chars.len() {
                // Trailing garbage with no block; nothing more to parse.
                break;
            }
            let header: String = chars[header_start..j].iter().collect();
            let header = header.trim().to_string();

            let mut depth = 1i32;
            let mut k = j + 1;
            while k < chars.len() && depth > 0 {
                match chars[k] {
                    '{' => depth += 1,
                    '}' => depth -= 1,
                    _ => {}
                }
                k += 1;
            }
            let body_end = if depth == 0 { k - 1 } else { k };
            let body: String = chars[(j + 1).min(body_end)..body_end].iter().collect();

            if let Some(rest) = header.strip_prefix('@') {
                let name = rest
                    .split_whitespace()
                    .next()
                    .unwrap_or("")
                    .to_ascii_lowercase();
                if name == "page" {
                    let decls = parse_declarations(&body);
                    let mut rule = PageRule::default();
                    for d in &decls {
                        match d.property.as_str() {
                            "size" => rule.size = PageSizeSpec::parse(&d.value),
                            "margin" => match parse_margin_shorthand(&d.value) {
                                Some(m) => rule.margin = Some(m),
                                None => warnings
                                    .push(format!("unsupported @page margin: {}", d.value)),
                            },
                            "margin-top" | "margin-right" | "margin-bottom" | "margin-left" => {
                                match parse_length(&d.value, 12.0) {
                                    Some(v) => {
                                        let mut m = rule.margin.unwrap_or_default();
                                        match d.property.as_str() {
                                            "margin-top" => m.top = v,
                                            "margin-right" => m.right = v,
                                            "margin-bottom" => m.bottom = v,
                                            "margin-left" => m.left = v,
                                            _ => unreachable!(),
                                        }
                                        rule.margin = Some(m);
                                    }
                                    None => warnings.push(format!(
                                        "unsupported @page {}: {}",
                                        d.property, d.value
                                    )),
                                }
                            }
                            _ => {
                                warnings.push(format!("unsupported @page property: {}", d.property))
                            }
                        }
                    }
                    page_rule = Some(rule);
                } else {
                    warnings.push(format!("unsupported at-rule: @{name}"));
                }
            } else if !header.is_empty() {
                let selectors: Vec<SimpleSelector> = header
                    .split(',')
                    .filter_map(|s| SimpleSelector::parse(s.trim()))
                    .collect();
                if selectors.is_empty() {
                    warnings.push(format!("unsupported selector: {header}"));
                } else {
                    let declarations = parse_declarations(&body);
                    rules.push(Rule {
                        selectors,
                        declarations,
                        source_order,
                    });
                    source_order += 1;
                }
            }

            i = k;
        }

        Self {
            rules,
            page_rule,
            warnings,
        }
    }

    /// Merge `other`'s rules after this sheet's rules (later source order
    /// wins ties), keeping the earlier `page_rule` unless `other` sets one.
    pub fn merge(mut self, other: Stylesheet) -> Self {
        let offset = self.rules.len();
        for mut rule in other.rules {
            rule.source_order += offset;
            self.rules.push(rule);
        }
        if other.page_rule.is_some() {
            self.page_rule = other.page_rule;
        }
        self.warnings.extend(other.warnings);
        self
    }

    /// Declarations from every rule whose selector matches, sorted by
    /// `(specificity, source_order)` ascending — callers apply them in
    /// order so the last one wins (mirrors CSS cascade + tie-break rules).
    pub fn matching_declarations(
        &self,
        tag: &str,
        classes: &[String],
        id: Option<&str>,
    ) -> Vec<Declaration> {
        let mut matched: Vec<(u32, usize, &Declaration)> = Vec::new();
        for rule in &self.rules {
            let best_specificity = rule
                .selectors
                .iter()
                .filter(|sel| sel.matches(tag, classes, id))
                .map(|sel| sel.specificity())
                .max();
            if let Some(spec) = best_specificity {
                for decl in &rule.declarations {
                    matched.push((spec, rule.source_order, decl));
                }
            }
        }
        matched.sort_by_key(|(spec, order, _)| (*spec, *order));
        matched.into_iter().map(|(_, _, d)| d.clone()).collect()
    }
}

fn strip_comments(css: &str) -> String {
    let mut out = String::with_capacity(css.len());
    let mut chars = css.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '/' && chars.peek() == Some(&'*') {
            chars.next();
            while let Some(c) = chars.next() {
                if c == '*' && chars.peek() == Some(&'/') {
                    chars.next();
                    break;
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// Find the last `color` declaration among `decls` (cascade order) and
/// parse it.
pub fn resolve_color(decls: &[Declaration]) -> Option<Color> {
    decls
        .iter()
        .rev()
        .find(|d| d.property == "color")
        .and_then(|d| crate::css::parse_color(&d.value))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cascade_specificity_id_beats_class_beats_tag() {
        let sheet = Stylesheet::parse(
            "p { color: #00ff00 } .warn { color: #ffff00 } #boss { color: #ff0000 }",
        );
        let d = sheet.matching_declarations("p", &["warn".into()], Some("boss"));
        assert_eq!(resolve_color(&d), Some(Color::rgb(1.0, 0.0, 0.0)));
    }

    #[test]
    fn later_rule_wins_ties() {
        let sheet = Stylesheet::parse("p { color: #00ff00 } p { color: #0000ff }");
        let d = sheet.matching_declarations("p", &[], None);
        assert_eq!(resolve_color(&d), Some(Color::rgb(0.0, 0.0, 1.0)));
    }

    #[test]
    fn at_page_rule_extracted() {
        let sheet = Stylesheet::parse("@page { size: a4; margin: 36pt }");
        assert_eq!(
            sheet.page_rule.as_ref().unwrap().size,
            Some(PageSizeSpec::A4)
        );
        assert_eq!(
            sheet.page_rule.as_ref().unwrap().margin,
            Some(Margins::all(36.0))
        );
    }

    #[test]
    fn at_page_margin_shorthand_supports_2_3_and_4_value_forms() {
        let sheet = Stylesheet::parse("@page { margin: 10pt 20pt }");
        assert_eq!(
            sheet.page_rule.as_ref().unwrap().margin,
            Some(Margins::new(10.0, 20.0, 10.0, 20.0))
        );

        let sheet = Stylesheet::parse("@page { margin: 10pt 20pt 30pt }");
        assert_eq!(
            sheet.page_rule.as_ref().unwrap().margin,
            Some(Margins::new(10.0, 20.0, 30.0, 20.0))
        );

        let sheet = Stylesheet::parse("@page { margin: 10pt 20pt 30pt 40pt }");
        assert_eq!(
            sheet.page_rule.as_ref().unwrap().margin,
            Some(Margins::new(10.0, 20.0, 30.0, 40.0))
        );
    }

    #[test]
    fn at_page_margin_longhands_override_individual_sides() {
        let sheet = Stylesheet::parse("@page { margin: 36pt; margin-left: 100pt }");
        assert_eq!(
            sheet.page_rule.as_ref().unwrap().margin,
            Some(Margins::new(36.0, 36.0, 36.0, 100.0))
        );
    }

    #[test]
    fn at_page_rule_with_physical_units_resolves_to_points() {
        let sheet = Stylesheet::parse("@page { size: 8.5in 11in }");
        let size = sheet.page_rule.as_ref().unwrap().size.unwrap();
        assert_eq!(
            size,
            PageSizeSpec::Custom {
                width: 612.0,
                height: 792.0
            }
        );
        let page_size = size.to_page_size();
        match page_size {
            PageSize::Custom { width, height } => {
                assert_eq!(width, 612.0);
                assert_eq!(height, 792.0);
            }
            other => panic!("expected Custom page size, got {other:?}"),
        }
    }

    #[test]
    fn unknown_at_rule_produces_warning_not_error() {
        let sheet = Stylesheet::parse("@media print { p { color: red } }");
        assert!(sheet.warnings.iter().any(|w| w.contains("@media")));
    }

    #[test]
    fn tag_and_class_selector_combo() {
        let sheet = Stylesheet::parse("p.big { font-size: 20pt }");
        let d = sheet.matching_declarations("p", &["big".into()], None);
        assert_eq!(d.len(), 1);
        assert_eq!(d[0].value, "20pt");
        let none = sheet.matching_declarations("div", &["big".into()], None);
        assert!(none.is_empty());
    }

    #[test]
    fn comments_are_stripped() {
        let sheet = Stylesheet::parse("/* comment */ p { color: red } /* another */");
        let d = sheet.matching_declarations("p", &[], None);
        assert_eq!(resolve_color(&d), Some(Color::rgb(1.0, 0.0, 0.0)));
    }
}
