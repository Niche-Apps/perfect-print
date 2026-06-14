# ADR-0001: Rendering Stack

**Date:** 2026-06-08
**Status:** Accepted

## Context

We need to choose the rendering stack for perfect-print. The library must:
1. Shape text correctly for Latin, CJK, Arabic, Hebrew, emoji, bidi
2. Generate PDF output
3. Generate raster (PNG) output
4. Support native print dialogs and job submission
5. Guarantee WYSIWYG across all output paths
6. Work cross-platform (macOS, Linux, Windows)
7. Be easy to depend on (`cargo add perfect-print`)

## Decision

We will use a **hybrid pure-Rust approach** with platform-specific native print backends:

| Component | Choice | Rationale |
|-----------|--------|-----------|
| Text shaping | `rustybuzz` | Pure Rust harfbuzz port, handles all scripts |
| Font loading | `ttf-parser` + `fontdb` | Lightweight, cross-platform font discovery |
| Vector/PDF output | Custom PDF writer on `lopdf` | Full control over PDF generation, font embedding, subsetting |
| Raster output | `tiny-skia` | Pure Rust, Skia subset, no system deps, handles paths/text/images |
| Native print (macOS) | `NSPrintOperation` via `objc2-foundation` | Submit rendered PDF data via native dialog |
| Native print (Linux) | `cups-sys` / IPP protocol | CUPS is the standard Linux print system |

## Consequences

### Positive
- Pure Rust core = easy builds, no system dependencies
- `rustybuzz` = best-in-class text shaping
- `tiny-skia` = battle-tested rasterization (used by `resvg`, `iced`)
- Same canonical page model feeds all backends = true WYSIWYG
- Native print backends submit PDF data = guaranteed print accuracy

### Negative
- Custom PDF writer is significant engineering effort (PDF spec is complex)
- `tiny-skia` text rendering may not match `rustybuzz` shaping exactly (need to verify)
- Three native print backends needed for full cross-platform support

### Risks
- **PDF/text fidelity:** We must ensure text positioning from rustybuzz shaping maps correctly to both PDF and tiny-skia raster output
- **Font subsetting:** Custom PDF writer must subset fonts to keep PDF sizes reasonable
- **Performance:** Pure Rust PDF generation may be slower than C libraries for large documents

## Alternatives Considered

| Alternative | Why Rejected |
|-------------|-------------|
| Cairo+Pango | System library dependency, pango build issues on macOS |
| Skia | Too heavy, complex C++ build, overkill for print |
| Vello | No PDF output, GPU-focused |
| printpdf+rusttype | Inadequate text shaping for complex scripts |
| genpdf | Built on printpdf+rusttype, not maintained |

## Implementation Notes

The key insight is that **native print backends should submit PDF data**, not re-render the document. This guarantees WYSIWYG: the same PDF bytes used for preview and export are sent to the printer.

For platforms where PDF submission isn't possible, the backend rasterizes at printer DPI and sends raster data. This is a fallback, not the primary path.

## Verification

- [x] rustybuzz shapes Latin correctly with ligatures
- [x] rustybuzz shapes CJK correctly (Arial Unicode)
- [x] rustybuzz shapes Arabic RTL correctly
- [x] rustybuzz shapes Hebrew RTL correctly
- [x] Font fallback needed (Al Nile has no Latin glyphs)
- [ ] tiny-skia raster output matches PDF text positioning (TBD Phase 2)
- [ ] Custom PDF writer produces valid PDF (TBD Phase 2)
- [ ] Native print submission via NSPrintOperation (TBD Phase 5)
