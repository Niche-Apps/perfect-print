# Canonical Page Model

## Overview

The canonical page model is the single source of truth for all document output.
Every backend (PDF, raster, preview, native print) consumes this same model.
No backend may create its own layout or rendering path.

## Architecture

```
User API (perfect-print)
    |
    v
DocumentBuilder
    |
    v
Canonical Model (perfect-print-core)
    |
    +---> PDF Backend (perfect-print-pdf)
    +---> Raster Backend (perfect-print-render)
    +---> Preview (perfect-print-preview)
    +---> Native Print (perfect-print-backend-*)
```

## Core Types

### Units

All internal measurements are in **points** (1/72 inch). The `Length` type
supports conversion between points, inches, mm, and px-at-DPI.

```rust
let width = Length::inches(8.5).to_points();  // 612.0
let height = Length::mm(210.0).to_points();    // ~595.0 (A4)
```

### Document

```rust
pub struct DocumentModel {
    pub pages: Vec<Page>,
    pub resources: ResourceStore,
    pub metadata: DocumentMetadata,
}
```

### Page

```rust
pub struct Page {
    pub size: Size,           // in points
    pub margins: Margins,     // in points
    pub layers: Vec<Layer>,   // background, foreground, header, footer
}
```

### Layer

Layers provide z-ordering:
- `Background` - drawn first
- `Foreground` - drawn last (default for user content)
- `Header` - repeated at top of each page
- `Footer` - repeated at bottom of each page

### DrawCommand

The canonical rendering instructions. Every backend must handle all variants:

- `Text` - shaped text run at a position
- `FillRect` / `StrokeRect` - rectangles
- `FillPath` / `StrokePath` - vector paths
- `Image` - raster images
- `PushClip` / `PopClip` - clipping regions
- `PushTransform` / `PopTransform` - affine transforms
- `PushOpacity` / `PopOpacity` - opacity stacking
- `Block` - nested flow layout results

### ResourceStore

Holds references to fonts and images. The actual binary data is stored
separately (the model only stores handles for serialization).

## Serialization

The model serializes to stable, deterministic JSON. This is critical for:
- Golden tests (byte-identical output across runs)
- Debugging (inspect the model before rendering)
- Caching (hash the model to detect changes)

```rust
let json = model.to_json()?;
// Running twice produces byte-identical output
assert_eq!(model.to_json()?, model.to_json()?);
```

## Why One Model?

1. **WYSIWYG guarantee** - same input produces same output everywhere
2. **Testability** - verify the model once, trust all backends
3. **Debugging** - inspect the model to understand rendering issues
4. **Caching** - hash the model to avoid redundant rendering
5. **Composability** - backends can be swapped without changing the model

## Strictness Modes

When a backend cannot honor a requested setting:

- `BestEffort` - try to print, report warnings
- `Warn` (default) - print only if differences are non-destructive
- `Exact` - fail with a structured error

No backend may silently ignore unsupported settings.
