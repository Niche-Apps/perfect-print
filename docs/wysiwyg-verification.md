# WYSIWYG Verification Strategy

**Date:** 2026-06-08
**Status:** Draft

## Problem

"WYSIWYG" is a vibe word. We need measurable, automated verification that the same document produces identical output across:
1. PDF export
2. Raster (PNG) export
3. Native print submission
4. Preview window

## Strategy: Layered Verification

### Layer 1: Structured Geometry Checks (CI-friendly, no visual comparison)

These are deterministic, fast, and run in CI without a display.

```rust
// Verify page count
assert_eq!(model.pages.len(), 2);

// Verify page size in points
assert_eq!(model.pages[0].size.width, 612.0); // Letter
assert_eq!(model.pages[0].size.height, 792.0);

// Verify text baselines (y-position of text lines)
let baselines = extract_text_baselines(&model, page=0);
assert!((baselines[0] - 720.0).abs() < 0.01); // Title at y=720

// Verify table row positions
let rows = extract_table_rows(&model, page=0, table=0);
assert_eq!(rows.len(), 3); // 3 data rows
assert!((rows[0].y - 500.0).abs() < 0.5);

// Verify content bounds
let bounds = content_bounds(&model, page=0);
assert!(bounds.width > 0.0);
assert!(bounds.height > 0.0);
```

### Layer 2: Canonical Model Serialization (Golden Tests)

The canonical page model serializes to stable JSON. Any change in rendering produces a different JSON.

```rust
// Serialize model to JSON
let json = serde_json::to_string_pretty(&model).unwrap();

// Compare against golden file
let golden = std::fs::read_to_string("tests/golden/invoice-model.json").unwrap();
assert_eq!(json, golden); // Byte-identical
```

This catches:
- Font metric changes
- Text shaping differences
- Layout algorithm changes
- Page size changes

### Layer 3: PDF/Raster Pixel Diff (Visual Parity)

Render the same document to both PDF and PNG, then compare pixel-by-pixel.

```bash
# Render to PDF
cargo run -p perfect-print-cli -- render examples/invoice --pdf /tmp/invoice.pdf

# Render to PNG (direct raster)
cargo run -p perfect-print-cli -- render examples/invoice --png-dir /tmp/invoice-raster

# Convert PDF to PNG (via pdftoppm or similar)
pdftoppm -png -r 300 /tmp/invoice.pdf /tmp/invoice-pdf

# Compare
cargo run -p perfect-print-cli -- verify \
  --pdf-raster /tmp/invoice-pdf \
  --direct-raster /tmp/invoice-raster \
  --dpi 300 --tolerance 0.1
```

**Tolerance model:**
- 0.0 = pixel-perfect (too strict for text anti-aliasing)
- 0.1 = 10% of pixels may differ by up to 1/256 intensity (reasonable)
- Report per-page diff percentage and max intensity difference

### Layer 4: Print Output Verification (Virtual Printer)

On macOS, use the "Save as PDF" virtual printer to capture print output.

```bash
# Print to virtual PDF printer
cargo run -p perfect-print-cli -- print examples/invoice \
  --printer "Save as PDF" \
  --settings tests/settings/letter.json \
  --capture-output /tmp/invoice-printed.pdf

# Compare printed PDF against canonical PDF
cargo run -p perfect-print-cli -- verify \
  --reference /tmp/invoice.pdf \
  --against /tmp/invoice-printed.pdf \
  --mode exact
```

### Layer 5: Cross-Platform Model Verification

The canonical model is platform-independent. We can verify that the same Rust code produces the same model on macOS, Linux, and Windows.

```rust
#[test]
fn invoice_model_is_platform_independent() {
    let doc = examples::invoice();
    let model = doc.to_model();
    let json = serde_json::to_string_pretty(&model).unwrap();
    insta::assert_snapshot!("invoice-model", json);
}
```

## Verification CLI Commands

```bash
# List printers
perfect-print-cli printers list --json

# Show printer capabilities
perfect-print-cli capabilities --printer "EPSON ET-16650" --json

# Render document
perfect-print-cli render examples/invoice --pdf out.pdf --png-dir pages/

# Verify PDF/raster parity
perfect-print-cli verify --pdf-raster pages-pdf/ --direct-raster pages-raster/ --dpi 300

# Print with verification
perfect-print-cli print examples/invoice --printer "Save as PDF" --capture-output printed.pdf --verify

# Generate diagnostics bundle
perfect-print-cli diagnostics examples/invoice --out diagnostics.zip
```

## Tolerance Specifications

| Check | Tolerance | Notes |
|-------|-----------|-------|
| Page count | exact | Must match exactly |
| Page size | ±0.01 pt | Sub-point precision |
| Text baseline | ±0.1 pt | Anti-aliasing may shift by sub-pixel |
| Table row position | ±0.5 pt | Row height may vary slightly |
| Content bounds | ±1.0 pt | Overall content area |
| Pixel diff (PDF vs raster) | < 0.1% pixels, max 5/256 intensity | Anti-aliasing differences |
| Printed vs canonical PDF | exact (text), < 0.5% pixels (raster) | Printer may apply minor adjustments |

## What We Do NOT Rely On

- Screenshots for verification (too fragile, resolution-dependent)
- "Looks the same" human judgment
- Single-output testing (must test all output paths)
- Platform-specific golden files (model JSON is platform-independent)
