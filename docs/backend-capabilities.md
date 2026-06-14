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
| `perfect-print-backend-macos` | macOS | Active | `lpstat`/`lp`/`cancel` CLI bridge |
| `perfect-print-backend-windows` | Windows | Stub | Planned: `winspool` or `PrintDocument` API |
| `perfect-print-backend-linux` | Linux | Stub | Planned: CUPS via `cups-sys` or `ipp` crate |
| `perfect-print` | All | Stable | Ergonomic public API (`Document`, `Paragraph`, etc.) |
| `perfect-print-cli` | All | Stable | CLI: model, render, verify, print, diagnostics |
| `perfect-print-preview` | All | Stub | Planned: live preview pane |
| `perfect-print-tauri` | All | Stub | Planned: Tauri app integration |
| `perfect-print-egui` | All | Stub | Planned: egui native print dialog |
| `perfect-print-iced` | All | Stub | Planned: iced native print dialog |

## macOS Backend (`perfect-print-backend-macos`)

### Capabilities
- **Printer enumeration**: `lpstat -a` — lists all available printers
- **Default printer**: `lpstat -d` — system default destination
- **Printer capabilities**: `lpoptions -p <name> -l` — paper sizes, color, duplex
- **Print submission**: `lp -d <printer>` — with full settings support
- **Job tracking**: `lpstat -o` — list pending jobs
- **Job cancellation**: `cancel <job_id>` — cancel a queued job

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
- No native `NSPrintPanel` integration (uses CLI bridge)
- No per-page preview in print dialog
- Resolution options not exposed via `lpoptions` parsing
- Borderless printing not detected

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
