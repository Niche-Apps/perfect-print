# Printer Settings Reference

## PrintSettings

The `PrintSettings` struct controls all aspects of a print job.

```rust
pub struct PrintSettings {
    pub paper_size: PageSize,
    pub orientation: PageOrientation,
    pub copies: u32,
    pub color_mode: ColorMode,
    pub duplex: DuplexMode,
    pub resolution: Option<u32>,
    pub page_range: PageRange,
    pub scaling: PrintScaling,
    pub borderless: bool,
    pub collate: bool,
}
```

## Defaults

```rust
PrintSettings::default() == PrintSettings {
    paper_size: PageSize::Letter,
    orientation: PageOrientation::Portrait,
    copies: 1,
    color_mode: ColorMode::Color,
    duplex: DuplexMode::Simplex,
    resolution: None,        // Use printer default
    page_range: PageRange::All,
    scaling: PrintScaling::FitToPage,
    borderless: false,
    collate: true,
}
```

## Builder API

```rust
let settings = PrintSettings::default()
    .paper_size(PageSize::A4)
    .orientation(PageOrientation::Landscape)
    .copies(3)
    .duplex(DuplexMode::LongEdge)
    .resolution(600)
    .page_range(PageRange::Range(1, 5))
    .scaling(PrintScaling::None);
```

## Paper Sizes

| Variant | Dimensions (pt) | Dimensions (in) | Dimensions (mm) |
|---------|-----------------|-----------------|-----------------|
| `Letter` | 612 × 792 | 8.5 × 11 | 216 × 279 |
| `A4` | 595 × 842 | 8.27 × 11.69 | 210 × 297 |
| `Legal` | 612 × 1008 | 8.5 × 14 | 216 × 356 |
| `Tabloid` | 792 × 1224 | 11 × 17 | 279 × 432 |
| `A3` | 842 × 1191 | 11.69 × 16.54 | 297 × 420 |
| `A5` | 420 × 595 | 5.83 × 8.27 | 148 × 210 |
| `Custom { w, h }` | w × h | — | — |

## Orientation

```rust
pub enum PageOrientation {
    Portrait,         // Normal
    Landscape,        // Rotated 90° clockwise
    ReversePortrait,  // Upside down
    ReverseLandscape, // Rotated 90° counter-clockwise
}
```

## Color Mode

```rust
pub enum ColorMode {
    Color,       // Full color
    Monochrome,  // Black and white (1-bit)
    Grayscale,   // Gray levels
}
```

## Duplex Mode

```rust
pub enum DuplexMode {
    Simplex,    // Single-sided
    LongEdge,   // Double-sided, flip on long edge (book-style)
    ShortEdge,  // Double-sided, flip on short edge (calendar-style)
}
```

## Page Range

```rust
pub enum PageRange {
    All,              // Print all pages
    Range(u32, u32),  // Print pages start..=end (1-indexed, inclusive)
    Pages(Vec<u32>),  // Print specific pages
}
```

Examples:
```rust
PageRange::All
PageRange::Range(1, 5)      // Pages 1 through 5
PageRange::Pages(vec![1, 3, 5]) // Pages 1, 3, and 5 only
```

## Scaling

```rust
pub enum PrintScaling {
    FitToPage,    // Scale to fit printable area
    FillPage,     // Scale to fill (may crop)
    None,         // No scaling (1:1)
    Custom(f64),  // Custom factor (1.0 = 100%)
}
```

## Validation

`PrintSettings::validate()` checks settings against `PrinterCapabilities` and returns warnings for unsupported values:

```rust
let warnings = settings.validate(&printer.capabilities)?;
for warning in &warnings {
    eprintln!("Warning: {}", warning);
}
```

### Warning Types

| Warning | Trigger | Fallback |
|---------|---------|----------|
| `UnsupportedPaperSize` | Requested size not in printer's list | First supported size |
| `UnsupportedColorMode` | Color requested but printer is mono | Monochrome |
| `UnsupportedDuplex` | Duplex requested but printer is simplex | Simplex |
| `UnsupportedResolution` | DPI not supported | Closest lower DPI |
| `UnsupportedBorderless` | Borderless not supported | Bordered |

### Apply Fallbacks

```rust
let compatible_settings = settings.apply_fallbacks(&printer.capabilities);
// Returns a new PrintSettings with unsupported values replaced
```

## Resolution

Resolution is specified in DPI (dots per inch). Common values:

| DPI | Use Case |
|-----|----------|
| 150 | Draft quality |
| 300 | Standard quality (default for most printers) |
| 600 | High quality |
| 1200 | Photo quality (inkjet) |

If `resolution` is `None`, the printer's default resolution is used.

## Strictness Modes

When validation warnings are found, the strictness mode determines behavior:

| Mode | Behavior |
|------|----------|
| `BestEffort` | Silently apply fallbacks, never fail |
| `Warn` | Print warnings, apply fallbacks, continue (default) |
| `Exact` | Fail on any unsupported setting |

```rust
match strictness {
    Strictness::BestEffort => { /* continue */ }
    Strictness::Warn => { eprintln!("Warning: {}", warning); }
    Strictness::Exact => { return Err(PrintError::InvalidSettings(warning.to_string())); }
}
```
