# Rendering Backend Matrix

**Date:** 2026-06-08
**Status:** Research complete, decision pending ADR-0001

## Approaches Evaluated

### 1. Skia (via `skia-safe`)

| Aspect | Assessment |
|--------|-----------|
| Text shaping | Excellent (harfbuzz built in) |
| PDF output | Good (Skia has PDF backend) |
| Raster output | Excellent |
| Native print | No direct support |
| Cross-platform | Yes (macOS, Linux, Windows) |
| Rust integration | `skia-safe` crate, but heavy C++ dependency |
| Binary size | Large (~10MB+) |
| Build complexity | Complex (needs depot_tools, ninja) |
| Font embedding | Yes |
| License | BSD 3-clause |

**Verdict:** Overkill for a print library. Skia is a full 2D graphics engine designed for browsers and UI. The C++ build dependency and binary size make it unsuitable for a Rust crate that should be easy to depend on.

### 2. Vello

| Aspect | Assessment |
|--------|-----------|
| Text shaping | Good (via `parley` for layout, `swash` for shaping) |
| PDF output | None (GPU-focused renderer) |
| Raster output | Excellent (GPU-accelerated) |
| Native print | No |
| Cross-platform | Yes (via wgpu) |
| Rust integration | Pure Rust |
| Binary size | Moderate |
| Build complexity | Moderate (needs GPU) |
| Font embedding | N/A (no PDF) |
| License | Apache 2.0 / MIT |

**Verdict:** Vello is a next-gen GPU renderer. It has no PDF output path, which is a hard requirement. It's designed for screen rendering, not print. Not suitable as the primary backend.

### 3. Cairo + Pango

| Aspect | Assessment |
|--------|-----------|
| Text shaping | Excellent (Pango uses harfbuzz) |
| PDF output | Excellent (native PDF surface) |
| Raster output | Excellent (same API as PDF) |
| Native print | No direct support |
| Cross-platform | Yes |
| Rust integration | `cairo-rs`, `pango`, `pangocairo` crates |
| Binary size | Moderate (system libs) |
| Build complexity | **Problematic on macOS** (no Homebrew pango by default) |
| Font embedding | Yes |
| License | LGPL / MPL |

**Verdict:** Cairo+Pango is the most mature option for PDF+raster parity. The same drawing code produces both PDF and PNG output. However:
- Pango is NOT installed on this macOS system (would need `brew install pango cairo`)
- Pango builds on macOS are notoriously fragile
- System library dependency complicates distribution
- LGPL licensing may be a concern for some users

**Test result:** Build failed on this system - pango not installed via pkg-config.

### 4. Native Platform APIs (CoreGraphics/CoreText, Direct2D/DirectWrite, Cairo/Pango)

| Aspect | Assessment |
|--------|-----------|
| Text shaping | Excellent (each platform has best-in-class shaping) |
| PDF output | Good (each platform can generate PDF) |
| Raster output | Good |
| Native print | **Excellent** (direct OS print API access) |
| Cross-platform | Needs 3 separate implementations |
| Rust integration | Platform-specific crates |
| Binary size | Small (uses system frameworks) |
| Build complexity | Low on each platform |
| Font embedding | Yes |
| License | N/A (system frameworks) |

**Verdict:** Best native print integration, but requires 3 separate backend implementations. Text shaping is excellent on each platform (CoreText uses harfbuzz, DirectWrite uses harfbuzz, Pango uses harfbuzz).

**Test result:** CoreText font loading works. CoreGraphics PDF context API changed in v0.24 (no `create_pdf_context`). Would need to use `CGPDFContext` directly.

### 5. Pure Rust: `printpdf` + `rustybuzz` + `image`

| Aspect | Assessment |
|--------|-----------|
| Text shaping | Excellent (via rustybuzz = harfbuzz port) |
| PDF output | Good (printpdf) |
| Raster output | Via separate path (image crate or custom) |
| Native print | No (would need platform-specific code) |
| Cross-platform | Yes (pure Rust) |
| Rust integration | Pure Rust crates |
| Binary size | Small |
| Build complexity | **Low** (just cargo) |
| Font embedding | Yes (printpdf supports TTF embedding) |
| License | MIT |

**Verdict:** The simplest approach. Pure Rust, no system dependencies. But:
- printpdf uses `ttf-parser` for font loading (not rustybuzz) - text shaping is limited
- No built-in text layout (no line breaking, no bidi)
- Raster output requires a separate rendering path
- No native print support

**Test result:** printpdf 0.7 builds and runs. PDF output works. But text shaping is via rusttype (not harfbuzz), so complex scripts are limited.

### 6. PDF-first with `genpdf` + custom text shaping

| Aspect | Assessment |
|--------|-----------|
| Text shaping | Would need rustybuzz integration |
| PDF output | Good (genpdf is higher-level than printpdf) |
| Raster output | Would need separate renderer |
| Native print | No |
| Cross-platform | Yes |
| Rust integration | Pure Rust |
| Build complexity | Low |
| License | MIT |

**Verdict:** genpdf is built on printpdf + rusttype. Same text shaping limitations. Not actively maintained (no commits in ~3 years).

## Text Shaping Deep Dive

Tested with `rustybuzz` 0.20.1 on macOS:

| Script | Font | Result |
|--------|------|--------|
| Latin | Helvetica | Perfect - ligatures (ffi, fi, fl) work |
| CJK | Arial Unicode | Perfect - correct glyph selection |
| Arabic (RTL) | Arial Unicode | Perfect - correct RTL ordering |
| Hebrew (RTL) | Arial Unicode | Perfect - correct RTL ordering |
| Emoji | Arial Unicode | Glyph ID 0 (needs color emoji font fallback) |
| Latin | Al Nile | Glyph ID 0 (no Latin coverage - proves need for fallback) |

**Key finding:** rustybuzz provides production-quality text shaping for all scripts. Font fallback is mandatory (no single font covers everything).

## Font Embedding/Subsetting

| Approach | Status |
|----------|--------|
| printpdf TTF embedding | Works, but no subsetting |
| genpdf font embedding | Works via printpdf |
| CoreGraphics PDF | Automatic font embedding |
| Cairo PDF | Automatic font embedding with subsetting |

**Key finding:** Font subsetting (embedding only used glyphs) is important for PDF size. printpdf embeds full fonts. For a production library, we need subsetting.

## PDF/Raster Parity

| Approach | Same code path? | Quality |
|----------|----------------|---------|
| Cairo | Yes (same drawing calls) | Excellent |
| Skia | Yes (same Skia canvas) | Excellent |
| Native APIs | No (separate PDF/raster contexts) | Good |
| printpdf + image | No (completely separate) | Poor |

**Key finding:** Cairo provides true PDF/raster parity because the same drawing code produces both. This is the gold standard for WYSIWYG.

## Native Print API Assessment (macOS)

macOS print APIs available via Rust:
- `NSPrintOperation` / `NSPrintInfo` - high-level print dialog and job submission
- `PMPrinter` / `PMPrintSession` - lower-level Carbon Print Manager (deprecated but functional)
- `CGPDFContext` + `NSPrintOperation` - render PDF then print it

**Current practical macOS approach:**
1. Generate a PDF/output artifact from the canonical page model
2. Submit through the CUPS CLI bridge (`lp`/`lpstat`/`cancel`)
3. Track native `NSPrintOperation` support as a future GUI backend, not current functionality

## Recommendation

**Hybrid approach:**

1. **Text shaping:** `rustybuzz` (pure Rust, harfbuzz-quality, all scripts)
2. **Font management:** `ttf-parser` + custom font stack with fallback
3. **PDF output:** Custom PDF writer (or `lopdf` for low-level PDF objects) with font subsetting
4. **Raster output:** Custom rasterizer using `tiny-skia` (pure Rust, Skia subset) or `image` crate
5. **Native print:** Platform-specific backends that consume the canonical page model

**Why not Cairo?** System library dependency is a dealbreaker for a Rust crate. Users shouldn't need to install pango/cairo system libraries.

**Why not Skia?** Too heavy, complex build, overkill for print.

**Why not pure printpdf?** Text shaping is inadequate for complex scripts.

**The winning architecture:**
- Pure Rust core with `rustybuzz` for text shaping
- `tiny-skia` for raster output (pure Rust, no system deps)
- Custom PDF writer for PDF output (or `lopdf` + `rusttype` for font embedding)
- Platform-specific print backends that render the canonical model to PDF/output artifacts, then submit via OS print systems

This gives us:
- No system library dependencies
- Excellent text shaping foundations
- Measurable parity checks from a shared model
- Cross-platform
- Small binary size
- Easy `cargo add perfect-print` experience
