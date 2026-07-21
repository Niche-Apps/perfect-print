//! CSS declaration tokenizer: values, lengths, and colors.
//!
//! Pure functions, no DOM dependency. Malformed input is dropped silently
//! here — callers are responsible for surfacing warnings for anything they
//! choose to reject.

use perfect_print_core::color::Color;

/// A single `property: value` declaration.
#[derive(Debug, Clone, PartialEq)]
pub struct Declaration {
    pub property: String,
    pub value: String,
}

/// Split `"a: b; c: d"` into declarations. Lowercases property names, trims
/// values, and skips malformed entries (missing colon, empty key/value).
pub fn parse_declarations(input: &str) -> Vec<Declaration> {
    input
        .split(';')
        .filter_map(|segment| {
            let segment = segment.trim();
            if segment.is_empty() {
                return None;
            }
            let (property, value) = segment.split_once(':')?;
            let property = property.trim().to_ascii_lowercase();
            let value = value.trim().to_string();
            if property.is_empty() || value.is_empty() {
                return None;
            }
            Some(Declaration { property, value })
        })
        .collect()
}

/// Resolve a CSS length to points. `parent_size` resolves `em`. Returns
/// `None` for unrecognized units or unparseable numbers.
///
/// - `pt` → points as-is.
/// - `px` → points at 96dpi (`× 0.75`).
/// - `em` → relative to `parent_size`.
/// - bare number → points.
pub fn parse_length(value: &str, parent_size: f64) -> Option<f64> {
    let value = value.trim();
    if let Some(number) = value.strip_suffix("pt") {
        return number.trim().parse::<f64>().ok();
    }
    if let Some(number) = value.strip_suffix("px") {
        return number.trim().parse::<f64>().ok().map(|n| n * 0.75);
    }
    if let Some(number) = value.strip_suffix("em") {
        return number.trim().parse::<f64>().ok().map(|n| n * parent_size);
    }
    value.parse::<f64>().ok()
}

/// Parse a CSS color: `#rgb`, `#rrggbb`, `#rrggbbaa`, `rgb(r, g, b)`, or one
/// of the 16 CSS1 basic named colors.
pub fn parse_color(value: &str) -> Option<Color> {
    let value = value.trim();
    if let Some(hex) = value.strip_prefix('#') {
        return parse_hex_color(hex);
    }
    let lower = value.to_ascii_lowercase();
    if let Some(inner) = lower.strip_prefix("rgb(").and_then(|s| s.strip_suffix(')')) {
        let parts: Vec<&str> = inner.split(',').map(|p| p.trim()).collect();
        if parts.len() != 3 {
            return None;
        }
        let r = parts[0].parse::<f64>().ok()?;
        let g = parts[1].parse::<f64>().ok()?;
        let b = parts[2].parse::<f64>().ok()?;
        return Some(Color::rgb(r / 255.0, g / 255.0, b / 255.0));
    }
    named_color(&lower)
}

fn parse_hex_color(hex: &str) -> Option<Color> {
    match hex.len() {
        3 => {
            let expanded: String = hex.chars().flat_map(|c| [c, c]).collect();
            Color::from_hex(&format!("#{expanded}"))
        }
        6 | 8 => Color::from_hex(&format!("#{hex}")),
        _ => None,
    }
}

/// The 16 CSS1 basic named colors.
fn named_color(name: &str) -> Option<Color> {
    let c = match name {
        "black" => Color::from_rgb_u8(0, 0, 0),
        "silver" => Color::from_rgb_u8(192, 192, 192),
        "gray" | "grey" => Color::from_rgb_u8(128, 128, 128),
        "white" => Color::from_rgb_u8(255, 255, 255),
        "maroon" => Color::from_rgb_u8(128, 0, 0),
        "red" => Color::from_rgb_u8(255, 0, 0),
        "purple" => Color::from_rgb_u8(128, 0, 128),
        "fuchsia" => Color::from_rgb_u8(255, 0, 255),
        "green" => Color::from_rgb_u8(0, 128, 0),
        "lime" => Color::from_rgb_u8(0, 255, 0),
        "olive" => Color::from_rgb_u8(128, 128, 0),
        "yellow" => Color::from_rgb_u8(255, 255, 0),
        "navy" => Color::from_rgb_u8(0, 0, 128),
        "blue" => Color::from_rgb_u8(0, 0, 255),
        "teal" => Color::from_rgb_u8(0, 128, 128),
        "aqua" | "cyan" => Color::from_rgb_u8(0, 255, 255),
        _ => return None,
    };
    Some(c)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_declarations() {
        let d = parse_declarations("font-size: 14pt; color: #ff0000; font-weight: bold");
        assert_eq!(d.len(), 3);
        assert_eq!(
            d[0],
            Declaration {
                property: "font-size".into(),
                value: "14pt".into()
            }
        );
    }

    #[test]
    fn parses_declarations_skips_malformed() {
        let d = parse_declarations("color: red; ; garbage; font-size:;  :12pt");
        assert_eq!(
            d,
            vec![Declaration {
                property: "color".into(),
                value: "red".into()
            }]
        );
    }

    #[test]
    fn parses_lengths() {
        assert_eq!(parse_length("14pt", 12.0), Some(14.0));
        assert_eq!(parse_length("16px", 12.0), Some(12.0)); // px × 0.75
        assert_eq!(parse_length("1.5em", 12.0), Some(18.0)); // em × parent size
        assert_eq!(parse_length("12", 12.0), Some(12.0)); // bare number = pt
        assert_eq!(parse_length("banana", 12.0), None);
    }

    #[test]
    fn parses_colors() {
        assert_eq!(parse_color("#ff0000"), Some(Color::rgb(1.0, 0.0, 0.0)));
        assert_eq!(parse_color("#f00"), Some(Color::rgb(1.0, 0.0, 0.0)));
        assert_eq!(
            parse_color("rgb(0, 128, 255)"),
            Some(Color::rgb(0.0, 128.0 / 255.0, 1.0))
        );
        assert_eq!(parse_color("red"), Some(Color::rgb(1.0, 0.0, 0.0)));
        assert_eq!(parse_color("chartreuse-ish"), None);
    }
}
