# Backend Capabilities

## Overview

perfect-print has a layered backend architecture. The **core** and **layout** crates are platform-independent. Platform-specific **backend** crates implement the `PrintDialog` trait for native printing.

## Crate Map

| Crate | Platform | Status | Description |
|-------|----------|--------|-------------|
| `perfect-print-core` | All | Stable | Document model, units, draw commands |
| `perfect-print-layout` | All | Stable | Text shaping, flow layout, pagination, tables |
| `perfect-print-render` | All | Stable | `Render` trait + `TinySkiaRenderer` (raster) |
| `perfect-print-pdf` | All | Stable | PDF 1.5 output via lopdf |
| `perfect-print-dialog` | All | Stable | `PrintDialog` trait, `PrintSettings`, validation |
| `perfect-print-backend-macos` | macOS | Active | Native `NSPrintPanel` plus `lpstat`/`lp`/`cancel` job API |
| `perfect-print-backend-windows` | Windows | Stub | Planned: `winspool` or `PrintDocument` API |
| `perfect-print-backend-linux` | Linux | Stub | Planned: CUPS via `cups-sys` or `ipp` crate |
| `perfect-print` | All | Stable | Ergonomic public API (`Document`, `Paragraph`, etc.) |
| `perfect-print-cli` | All | Stable | CLI: model, render, verify, print, diagnostics |
| `perfect-print-preview` | All | Stub | Planned: live preview pane |
| `perfect-print-tauri` | All | Active | Tauri wrapper over canonical Perfect Print rendering and native submission |
| `perfect-print-egui` | All | Stub | Planned: egui native print dialog |
| `perfect-print-iced` | All | Stub | Planned: iced native print dialog |

## macOS Backend (`perfect-print-backend-macos`)

### Capabilities
- **Native interactive printing**: standard `NSPrintPanel` + `NSPrintOperation` + PDFKit (not a custom sheet)
- **In-memory PDF submission**: no shared temporary filename or page rasterization
- **Printer enumeration**: `lpstat -a` — lists all available printers
- **Default printer**: `lpstat -d` — system default destination
- **Printer capabilities**: `lpoptions -p <name> -l` — paper sizes, color, duplex
- **Print submission**: `lp -d <printer>` — with full settings support
- **Job tracking**: `lpstat -o` — list pending jobs
- **Job cancellation**: `cancel <job_id>` — cancel a queued job

### Native `NSPrintPanel` options

Interactive jobs (`print_pdf_bytes_with_dialog` / `NSPrintOperation`) set
`operation.printPanel.options` to the system mask (OR'd with AppKit defaults):

| Option | Effect |
|--------|--------|
| `ShowsCopies` | Copies, plus Two-Sided when the printer supports duplex (`PMSetDuplex` is applied as the default) |
| `ShowsPageRange` | Pages |
| `ShowsPaperSize` | Paper size (Letter, Legal, Tabloid 11×17, A4, A3, and other printer papers) |
| `ShowsOrientation` | Portrait / Landscape |
| `ShowsScaling` | Scale (Fit-to-page is the default content path; the user can change Scale) |
| `ShowsPreview` | Preview |
| `ShowsPageSetupAccessory` | Standard Page Setup accessory |

Printer, Presets, and the PDF menu are provided by `NSPrintPanel` itself.
`PrintSettings` paper / orientation / scaling / duplex are **defaults** — they
are not locked after the sheet opens. Pagination is `Automatic` (Clip is not
forced; Clip discarded the Scale field). There is **no in-app sheet**; Scale
lives on the native panel only.

`PerfectPrintPDFView` computes the print page count from the live Scale:

| Panel Scale | Result |
|-------------|--------|
| Fit, or any scale that fits the imageable content area | **1 page** |
| Scale up | **N poster tiles** (row-major: left→right, then top→bottom) |
| Scale down | **fewer tiles**, 1 when the content fits |

Each print page is a contiguous crop of the source PDF page, not a copy of the
whole page. Tiles do not overlap through content. The 28pt page margin is the
tape/join strip; optional ≤8pt registration ticks and a margin-only “n of N”
label (hidden when N = 1) live in that band. The label is the **post-scale**
page count. Callers must not bake a stale “n of N” into a single full-chart
page (Families may still draw chrome on a pre-tiled PDF — the view only adds
chrome when *it* splits a source page).

**Families / chart documents:** emit a **single-page PDF** whose MediaBox is
the full chart (1 image pixel = 1 point, or the natural size in points). Do
**not** pre-tile at 100% before opening `NSPrintPanel` — that freezes N so
Scale cannot reduce the page count. Paper Size / Orientation / Scale stay on
the system panel. There is no Families poster-scale slider.

Color vs B&W is left to the printer's own system controls; this crate does not
add a custom color accessory.

### Supported Print Settings
| Setting | Flag | Notes |
|---------|------|-------|
| Paper size | `-o media=` | Letter, A4, Legal, Tabloid, A3, A5 |
| Copies | `-n <count>` | Any positive integer |
| Page range | `-P <range>` | `1-5`, `1,3,5`, etc. |
| Duplex | `-o sides=two-sided-long-edge` | Long-edge flip |
| Orientation | `-o orientation-requested=4` | Landscape |
| Scaling | `-o fit-to-page`, `-o fill`, `-o scaling=N` | Fit, fill, custom % |
| Collation | `-o Collate=True` | When copies > 1 |
| Job name | `-t <title>` | Set to "perfect-print job" |

### Job Lifecycle
1. `submit_print_job()` → returns `Option<String>` (job ID like "PrinterName-42")
2. `poll_job_status(job_id)` → `true` = completed, `false` = still in queue
3. `list_jobs()` → all pending jobs with printer and status
4. `cancel_job(job_id)` → cancel a queued job

### Limitations
- Resolution options not exposed via `lpoptions` parsing
- Borderless printing not detected
- Printer-specific color controls remain owned by `NSPrintPanel`

## Windows Backend (`perfect-print-backend-windows`)

### Status: Stub

### Planned Implementation
- Use `winspool` API via `windows` crate or `winapi`
- Or use `PrintDocument` API via `System.Printing` (C++/CLI bridge)
- Printer enumeration via `EnumPrinters`
- Print settings via `DEVMODE` structure
- Job tracking via `FindFirstPrinterChangeNotification`

## Linux Backend (`perfect-print-backend-linux`)

### Status: Stub

### Planned Implementation
- Use `cups-sys` crate for CUPS bindings
- Or use `ipp` crate for IPP protocol directly
- Printer enumeration via `cupsGetDests`
- Print settings via `cupsAddOption` / `cupsPrintFile`
- Job tracking via `cupsGetJobs`

## Platform-Independent Features

These work on all platforms without a backend:

| Feature | Crate | Notes |
|---------|-------|-------|
| PDF generation | `perfect-print-pdf` | lopdf, pure Rust |
| Raster rendering | `perfect-print-render` | tiny-skia, pure Rust |
| Text shaping | `perfect-print-layout` | rustybuzz + fontdb |
| Font fallback | `perfect-print-layout` | CJK, Arabic, emoji fallbacks |
| Hyphenation | `perfect-print-layout` | Knuth-Liang, English |
| Table layout | `perfect-print-layout` | Auto-width, cell measurement |
| Style inheritance | `perfect-print-layout` | Document → paragraph → run |
| Image embedding | `perfect-print-pdf` | FlateDecode XObject |
| Font embedding | `perfect-print-pdf` | FontFile2 streams |
| Structured errors | `perfect-print-core` | `PrintError`, `Strictness` modes |
| Diagnostics bundle | `perfect-print-cli` | Zip with PDF, PNGs, fonts, system info |
