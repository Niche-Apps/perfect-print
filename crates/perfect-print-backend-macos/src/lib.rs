//! macOS native print backend.
//!
//! Interactive jobs open a standard `NSPrintPanel` via `NSPrintOperation`
//! (`src/native_print.m`). PrintSettings paper size, orientation, duplex,
//! and scaling are defaults only — the user can change them in the sheet.
//! There is no in-app sheet; Scale lives on the native panel only.
//!
//! `PerfectPrintPDFView` paginates from the live panel Scale: a source page
//! that fits the imageable content area is 1 print page; scaling up produces
//! row-major poster tiles; scaling down reduces the tile count (1 when it
//! fits). Callers that want this (notably Families charts) should send a
//! **single-page PDF of the full chart**, not a pre-tiled 100% poster.
//!
//! Unattended jobs still use the CUPS CLI bridge:
//! - `lpstat` for printer enumeration
//! - `lpoptions` for printer capabilities
//! - `lp` / `cancel` for submission and queue management

use perfect_print_core::page::PageSize;
#[cfg(target_os = "macos")]
use perfect_print_dialog::ColorMode;
use perfect_print_dialog::{
    DuplexMode, PageOrientation, PageRange, PrintDialog, PrintDialogResult, PrintError,
    PrintScaling, PrintSettings, Printer, PrinterCapabilities, PrinterState,
};
#[cfg(target_os = "macos")]
use std::ffi::CString;
use std::process::Command;

#[cfg(target_os = "macos")]
#[repr(C)]
struct NativePrintSettings {
    copies: u32,
    landscape: bool,
    duplex: u8,
    color_mode: u8,
    scaling: u8,
    custom_scale: f64,
    paper_width: f64,
    paper_height: f64,
    collate: bool,
    page_range_kind: u8,
    first_page: u32,
    last_page: u32,
}

/// `NSPrintPanelOptions` bits applied to interactive jobs.
///
/// Must stay in sync with `PerfectPrintStandardPanelOptions()` in
/// `native_print.m`. Values are from `<AppKit/NSPrintPanel.h>`.
pub const NATIVE_PRINT_PANEL_OPTIONS: u32 = {
    const SHOWS_COPIES: u32 = 1 << 0;
    const SHOWS_PAGE_RANGE: u32 = 1 << 1;
    const SHOWS_PAPER_SIZE: u32 = 1 << 2;
    const SHOWS_ORIENTATION: u32 = 1 << 3;
    const SHOWS_SCALING: u32 = 1 << 4;
    const SHOWS_PAGE_SETUP_ACCESSORY: u32 = 1 << 8;
    const SHOWS_PREVIEW: u32 = 1 << 17;
    SHOWS_COPIES
        | SHOWS_PAGE_RANGE
        | SHOWS_PAPER_SIZE
        | SHOWS_ORIENTATION
        | SHOWS_SCALING
        | SHOWS_PAGE_SETUP_ACCESSORY
        | SHOWS_PREVIEW
};

/// `NSPrintingPaginationModeClip`. Interactive jobs must not force this —
/// Clip discarded the panel Scale field.
pub const NATIVE_PRINT_PAGINATION_CLIP: u32 = 2;

/// Tape/join strip between poster tiles (pt). Matches Families' page margin.
pub const POSTER_JOIN_MARGIN_PT: f64 = 28.0;

/// Registration tick length in the join margin (pt). Must stay ≤ 8.
pub const POSTER_TICK_LENGTH_PT: f64 = 6.0;

/// FitToPage scaling mode passed to the native view.
pub const SCALING_FIT_TO_PAGE: u8 = 0;
/// FillPage scaling mode passed to the native view.
pub const SCALING_FILL_PAGE: u8 = 1;
/// None (1:1) scaling mode; panel `scalingFactor` still applies.
pub const SCALING_NONE: u8 = 2;
/// Custom scaling mode (seeded onto `printInfo.scalingFactor` as None).
pub const SCALING_CUSTOM: u8 = 3;

unsafe extern "C" {
    fn perfect_print_native_inspect_poster_grid(
        media_w: f64,
        media_h: f64,
        paper_w: f64,
        paper_h: f64,
        imageable_x: f64,
        imageable_y: f64,
        imageable_w: f64,
        imageable_h: f64,
        scaling_mode: u8,
        custom_scale: f64,
        panel_scale: f64,
        out_cols: *mut u32,
        out_rows: *mut u32,
        out_pages: *mut u32,
        out_scale: *mut f64,
    ) -> i32;

    fn perfect_print_native_inspect_poster_tile(
        index: u32,
        media_w: f64,
        media_h: f64,
        scale: f64,
        tile_w: f64,
        tile_h: f64,
        out_col: *mut u32,
        out_row: *mut u32,
        out_index: *mut u32,
        out_src_x: *mut f64,
        out_src_y: *mut f64,
        out_src_w: *mut f64,
        out_src_h: *mut f64,
        out_dest_w: *mut f64,
        out_dest_h: *mut f64,
    ) -> i32;
}

#[cfg(target_os = "macos")]
unsafe extern "C" {
    fn perfect_print_pdf_dialog(
        pdf_bytes: *const u8,
        pdf_length: usize,
        title_utf8: *const std::ffi::c_char,
        settings: NativePrintSettings,
        selected_pages: *const u32,
        selected_page_count: usize,
    ) -> i32;

    fn perfect_print_native_panel_options_mask() -> u32;

    fn perfect_print_native_inspect_panel_defaults(
        out_options: *mut u32,
        out_horizontal_pagination: *mut u32,
        out_vertical_pagination: *mut u32,
    ) -> i32;
}

/// Inspect the poster grid for one source page (no AppKit, all platforms).
///
/// `scaling_mode` is 0=FitToPage, 1=FillPage, 2=None, 3=Custom.
/// `panel_scale` is `NSPrintInfo.scalingFactor` (1.0 = 100%).
pub fn inspect_poster_grid(
    media_w: f64,
    media_h: f64,
    paper_w: f64,
    paper_h: f64,
    imageable: (f64, f64, f64, f64),
    scaling_mode: u8,
    custom_scale: f64,
    panel_scale: f64,
) -> (u32, u32, u32, f64) {
    let mut cols = 0u32;
    let mut rows = 0u32;
    let mut pages = 0u32;
    let mut scale = 0.0f64;
    let rc = unsafe {
        perfect_print_native_inspect_poster_grid(
            media_w,
            media_h,
            paper_w,
            paper_h,
            imageable.0,
            imageable.1,
            imageable.2,
            imageable.3,
            scaling_mode,
            custom_scale,
            panel_scale,
            &mut cols,
            &mut rows,
            &mut pages,
            &mut scale,
        )
    };
    assert_eq!(rc, 1, "poster grid inspect should always succeed");
    (cols, rows, pages, scale)
}

/// One poster tile crop (0-based row-major index).
#[derive(Debug, Clone, PartialEq)]
pub struct PosterTile {
    pub col: u32,
    pub row: u32,
    pub index: u32,
    pub src_x: f64,
    pub src_y: f64,
    pub src_w: f64,
    pub src_h: f64,
    pub dest_w: f64,
    pub dest_h: f64,
}

/// Inspect one poster tile crop (no AppKit, all platforms).
pub fn inspect_poster_tile(
    index: u32,
    media_w: f64,
    media_h: f64,
    scale: f64,
    tile_w: f64,
    tile_h: f64,
) -> Option<PosterTile> {
    let mut col = 0u32;
    let mut row = 0u32;
    let mut out_index = 0u32;
    let mut src_x = 0.0;
    let mut src_y = 0.0;
    let mut src_w = 0.0;
    let mut src_h = 0.0;
    let mut dest_w = 0.0;
    let mut dest_h = 0.0;
    let rc = unsafe {
        perfect_print_native_inspect_poster_tile(
            index,
            media_w,
            media_h,
            scale,
            tile_w,
            tile_h,
            &mut col,
            &mut row,
            &mut out_index,
            &mut src_x,
            &mut src_y,
            &mut src_w,
            &mut src_h,
            &mut dest_w,
            &mut dest_h,
        )
    };
    if rc != 1 {
        return None;
    }
    Some(PosterTile {
        col,
        row,
        index: out_index,
        src_x,
        src_y,
        src_w,
        src_h,
        dest_w,
        dest_h,
    })
}

/// Show the native macOS print panel for an in-memory PDF.
///
/// The sheet is a standard `NSPrintPanel` (Printer, Presets, Copies, Pages,
/// Paper Size, Orientation, Scale, Preview, Page Setup, PDF menu). Settings
/// passed here are initial defaults; paper, orientation, scale, and
/// two-sided remain user-overridable in the panel. Fit-to-page is the
/// default scaling path. Scale on the panel changes the print page count
/// (fit → 1 page; scale up → poster tiles).
///
/// For oversized charts (Families), send a **single full-chart page**. Do
/// not pre-tile at 100% — that freezes N before the panel opens.
///
/// Returns `Ok(true)` when the user submits the job and `Ok(false)` when the
/// panel is cancelled. The native bridge always runs the panel on AppKit's main
/// thread, even when called by an async/Tauri worker thread.
#[cfg(target_os = "macos")]
pub fn print_pdf_bytes_with_dialog(
    pdf_bytes: &[u8],
    title: Option<&str>,
    settings: &PrintSettings,
) -> PrintDialogResult<bool> {
    if pdf_bytes.len() < 5 || !pdf_bytes.starts_with(b"%PDF-") {
        return Err(PrintError::PrintFailed(
            "Document is not a valid PDF payload".to_string(),
        ));
    }

    let page_size = settings.paper_size.to_size();
    let (scaling, custom_scale) = match settings.scaling {
        PrintScaling::FitToPage => (0, 1.0),
        PrintScaling::FillPage => (1, 1.0),
        PrintScaling::None => (2, 1.0),
        PrintScaling::Custom(scale) => (3, scale),
    };
    let native_settings = NativePrintSettings {
        copies: settings.copies.max(1),
        landscape: matches!(
            settings.orientation,
            PageOrientation::Landscape | PageOrientation::ReverseLandscape
        ),
        duplex: match settings.duplex {
            DuplexMode::Simplex => 0,
            DuplexMode::LongEdge => 1,
            DuplexMode::ShortEdge => 2,
        },
        color_mode: match settings.color_mode {
            ColorMode::Color => 0,
            ColorMode::Monochrome => 1,
            ColorMode::Grayscale => 2,
        },
        scaling,
        custom_scale,
        paper_width: page_size.width,
        paper_height: page_size.height,
        collate: settings.collate,
        page_range_kind: match settings.page_range {
            PageRange::All => 0,
            PageRange::Range(_, _) => 1,
            PageRange::Pages(_) => 2,
        },
        first_page: match settings.page_range {
            PageRange::Range(first, _) => first,
            _ => 0,
        },
        last_page: match settings.page_range {
            PageRange::Range(_, last) => last,
            _ => 0,
        },
    };

    let selected_pages = match &settings.page_range {
        PageRange::Pages(pages) => pages.as_slice(),
        _ => &[],
    };

    let safe_title = title.unwrap_or("Perfect Print").replace('\0', " ");
    let title = CString::new(safe_title)
        .map_err(|_| PrintError::Platform("Print title contains invalid data".to_string()))?;
    let result = unsafe {
        perfect_print_pdf_dialog(
            pdf_bytes.as_ptr(),
            pdf_bytes.len(),
            title.as_ptr(),
            native_settings,
            selected_pages.as_ptr(),
            selected_pages.len(),
        )
    };

    match result {
        1 => Ok(true),
        0 => Ok(false),
        _ => Err(PrintError::PrintFailed(
            "macOS could not create the native PDF print operation".to_string(),
        )),
    }
}

#[cfg(not(target_os = "macos"))]
pub fn print_pdf_bytes_with_dialog(
    pdf_bytes: &[u8],
    _title: Option<&str>,
    _settings: &PrintSettings,
) -> PrintDialogResult<bool> {
    if pdf_bytes.len() < 5 || !pdf_bytes.starts_with(b"%PDF-") {
        return Err(PrintError::PrintFailed(
            "Document is not a valid PDF payload".to_string(),
        ));
    }
    Err(PrintError::Platform(
        "The macOS print panel is unavailable on this platform".to_string(),
    ))
}

/// macOS native print backend.
pub struct MacosPrintDialog;

impl MacosPrintDialog {
    pub fn new() -> Self {
        Self
    }

    /// Enumerate printers via `lpstat -a`.
    fn enumerate_printers(&self) -> Vec<Printer> {
        let mut printers = Vec::new();

        // Get all accepted jobs (available printers)
        let output = Command::new("lpstat").args(["-a", "--"]).output();
        let output = match output {
            Ok(o) if o.status.success() => o,
            _ => return printers,
        };

        let stdout = String::from_utf8_lossy(&output.stdout);
        let default_printer = Self::get_default_printer_name();

        for line in stdout.lines() {
            // Format: "printer_name accepting requests since .."
            let name = line.split_whitespace().next().unwrap_or("");
            if name.is_empty() {
                continue;
            }

            let is_default = default_printer.as_ref().is_some_and(|d| d == name);
            let caps = self.get_printer_caps(name);

            printers.push(Printer::new(PrinterCapabilities {
                name: name.to_string(),
                paper_sizes: caps.paper_sizes,
                supports_color: caps.supports_color,
                supports_duplex: caps.supports_duplex,
                max_resolution: caps.max_resolution,
                supported_resolutions: caps.supported_resolutions,
                supports_borderless: false,
                is_default,
                state: PrinterState::Ready,
            }));

            if is_default && printers.len() == 1 {
                // Put default first
            }
        }

        // Sort: default first, then alphabetical
        printers.sort_by(|a, b| {
            b.capabilities
                .is_default
                .cmp(&a.capabilities.is_default)
                .then_with(|| a.capabilities.name.cmp(&b.capabilities.name))
        });

        printers
    }

    fn get_default_printer_name() -> Option<String> {
        let output = Command::new("lpstat").args(["-d"]).output().ok()?;
        if !output.status.success() {
            return None;
        }
        let stdout = String::from_utf8_lossy(&output.stdout);
        // Format: "system default destination: printer_name"
        stdout.split(": ").nth(1).map(|s| s.trim().to_string())
    }

    fn get_printer_caps(&self, name: &str) -> PrinterCapabilities {
        let mut paper_sizes = vec![PageSize::Letter, PageSize::A4, PageSize::Legal];
        let mut supports_color = false;
        let mut supports_duplex = false;

        // Get printer options via lpoptions
        if let Ok(output) = Command::new("lpoptions").args(["-p", name, "-l"]).output() {
            if output.status.success() {
                let stdout = String::from_utf8_lossy(&output.stdout);
                for line in stdout.lines() {
                    let lower = line.to_lowercase();
                    if lower.contains("color") || lower.contains("cmyk") || lower.contains("rgb") {
                        supports_color = true;
                    }
                    if lower.contains("duplex")
                        || lower.contains("double-sided")
                        || lower.contains("two-sided")
                    {
                        supports_duplex = true;
                    }
                    // Check for paper size options
                    if lower.starts_with("pagesize=") || lower.contains("PageSize=") {
                        for size in &[
                            ("Letter", PageSize::Letter),
                            ("A4", PageSize::A4),
                            ("Legal", PageSize::Legal),
                            ("Tabloid", PageSize::Tabloid),
                            ("A3", PageSize::A3),
                            ("A5", PageSize::A5),
                        ] {
                            if lower.contains(&size.0.to_lowercase())
                                && !paper_sizes.contains(&size.1)
                            {
                                paper_sizes.push(size.1);
                            }
                        }
                    }
                }
            }
        }

        // Also check via lpstat -p for status
        let state = if let Ok(output) = Command::new("lpstat").args(["-p", name]).output() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            if stdout.contains("disabled") {
                PrinterState::Error("Printer disabled".to_string())
            } else {
                PrinterState::Ready
            }
        } else {
            PrinterState::Offline
        };

        PrinterCapabilities {
            name: name.to_string(),
            paper_sizes,
            supports_color,
            supports_duplex,
            max_resolution: None,
            supported_resolutions: vec![],
            supports_borderless: false,
            is_default: false,
            state,
        }
    }

    /// Return validated settings for the settings-only trait hook.
    ///
    /// A real `NSPrintPanel` requires document content. Interactive callers use
    /// `print_pdf_bytes_with_dialog`, which supplies the PDF and displays the
    /// panel through `NSPrintOperation`.
    fn show_native_dialog(&self, settings: &PrintSettings) -> PrintDialogResult<PrintSettings> {
        Ok(settings.clone())
    }

    /// Submit a print job via `lp` command.
    pub fn submit_print_job(
        &self,
        pdf_path: &std::path::Path,
        settings: &PrintSettings,
    ) -> PrintDialogResult<Option<String>> {
        let mut cmd = Command::new("lp");

        // Printer
        let printers = self.enumerate_printers();
        if let Some(default) = printers.iter().find(|p| p.capabilities.is_default) {
            cmd.arg("-d").arg(&default.capabilities.name);
        }

        // Copies
        if settings.copies > 1 {
            cmd.arg("-n").arg(settings.copies.to_string());
        }

        // Page range
        match &settings.page_range {
            PageRange::All => {}
            PageRange::Range(start, end) => {
                cmd.arg("-P").arg(format!("{}-{}", start, end));
            }
            PageRange::Pages(pages) => {
                let range: Vec<String> = pages.iter().map(|p| p.to_string()).collect();
                cmd.arg("-P").arg(range.join(","));
            }
        }

        // Duplex
        if settings.duplex != DuplexMode::Simplex {
            cmd.arg("-o").arg("sides=two-sided-long-edge");
        }

        // Orientation
        if settings.orientation == PageOrientation::Landscape {
            cmd.arg("-o").arg("orientation-requested=4");
        }

        // Fit to page
        match &settings.scaling {
            PrintScaling::FitToPage => {
                cmd.arg("-o").arg("fit-to-page");
            }
            PrintScaling::FillPage => {
                cmd.arg("-o").arg("fill");
            }
            PrintScaling::None => {}
            PrintScaling::Custom(s) => {
                cmd.arg("-o").arg(format!("scaling={}", (s * 100.0) as u32));
            }
        }

        // Collate
        if settings.collate && settings.copies > 1 {
            cmd.arg("-o").arg("Collate=True");
        }

        // Paper size
        let paper_arg = match &settings.paper_size {
            PageSize::Letter => "Letter",
            PageSize::A4 => "A4",
            PageSize::Legal => "Legal",
            PageSize::Tabloid => "Tabloid",
            PageSize::A3 => "A3",
            PageSize::A5 => "A5",
            _ => "Letter",
        };
        cmd.arg("-o").arg(format!("media={}", paper_arg));

        // Job name
        cmd.arg("-t").arg("perfect-print job");

        // File
        cmd.arg(pdf_path);

        let output = cmd
            .output()
            .map_err(|e| PrintError::Platform(format!("Failed to run lp: {}", e)))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(PrintError::PrintFailed(format!(
                "lp failed ({}): {}",
                output.status, stderr
            )));
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let job_id = Self::parse_job_id(&stdout);
        log::info!("Print job submitted: {} (id: {:?})", stdout.trim(), job_id);
        Ok(job_id)
    }

    /// Parse the job ID from `lp` output.
    /// Typical output: "request id is PrinterName-42 (1 file(s))"
    fn parse_job_id(stdout: &str) -> Option<String> {
        let start = stdout.find("request id is ")? + "request id is ".len();
        let end = stdout[start..].find(' ').map(|i| start + i)?;
        Some(stdout[start..end].to_string())
    }

    /// Poll the status of a print job by ID.
    ///
    /// Uses `lpstat -o` to check if the job is still in the queue.
    /// Returns `Some(true)` if completed, `Some(false)` if still printing,
    /// or `None` if the job ID was not found (assumed completed).
    pub fn poll_job_status(&self, job_id: &str) -> PrintDialogResult<bool> {
        let output = Command::new("lpstat")
            .args(["-o"])
            .output()
            .map_err(|e| PrintError::Platform(format!("Failed to run lpstat: {}", e)))?;

        if !output.status.success() {
            return Err(PrintError::Platform("lpstat -o failed".to_string()));
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        // If the job ID appears in the output, it's still in the queue
        let in_queue = stdout.lines().any(|line| line.starts_with(job_id));
        Ok(!in_queue) // true = completed, false = still printing
    }

    /// List all pending print jobs.
    ///
    /// Returns a list of (job_id, printer, status) tuples.
    pub fn list_jobs(&self) -> PrintDialogResult<Vec<(String, String, String)>> {
        let output = Command::new("lpstat")
            .args(["-o"])
            .output()
            .map_err(|e| PrintError::Platform(format!("Failed to run lpstat: {}", e)))?;

        if !output.status.success() {
            return Err(PrintError::Platform("lpstat -o failed".to_string()));
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let mut jobs = Vec::new();
        for line in stdout.lines() {
            // Format: "PrinterName-42   user   1234567890  12345 bytes"
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 3 {
                let job_id = parts[0].to_string();
                // Extract printer name from job_id (everything before the last '-')
                if let Some(dash_pos) = job_id.rfind('-') {
                    let printer = job_id[..dash_pos].to_string();
                    let status = if line.contains("ready") {
                        "ready".to_string()
                    } else {
                        "printing".to_string()
                    };
                    jobs.push((job_id, printer, status));
                }
            }
        }
        Ok(jobs)
    }

    /// Cancel a print job by ID.
    pub fn cancel_job(&self, job_id: &str) -> PrintDialogResult<()> {
        let output = Command::new("cancel")
            .arg(job_id)
            .output()
            .map_err(|e| PrintError::Platform(format!("Failed to run cancel: {}", e)))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(PrintError::PrintFailed(format!(
                "cancel failed: {}",
                stderr.trim()
            )));
        }
        log::info!("Print job {} cancelled", job_id);
        Ok(())
    }
}

impl PrintDialog for MacosPrintDialog {
    fn show_print_dialog(
        &self,
        settings: &PrintSettings,
        _title: Option<&str>,
    ) -> PrintDialogResult<PrintSettings> {
        self.show_native_dialog(settings)
    }

    fn show_page_setup(&self, settings: &PrintSettings) -> PrintDialogResult<PrintSettings> {
        Ok(settings.clone())
    }

    fn available_printers(&self) -> PrintDialogResult<Vec<Printer>> {
        Ok(self.enumerate_printers())
    }

    fn default_printer(&self) -> PrintDialogResult<Printer> {
        let printers = self.enumerate_printers();
        printers
            .into_iter()
            .find(|p| p.capabilities.is_default)
            .ok_or(PrintError::NoPrinters)
    }
}

impl Default for MacosPrintDialog {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn print_dialog_rejects_invalid_pdf_before_native_panel() {
        let result =
            print_pdf_bytes_with_dialog(b"not a pdf", Some("Invalid"), &PrintSettings::default());
        assert!(matches!(result, Err(PrintError::PrintFailed(_))));
    }

    #[test]
    fn native_panel_options_mask_includes_required_controls() {
        assert_ne!(NATIVE_PRINT_PANEL_OPTIONS & (1 << 0), 0, "ShowsCopies");
        assert_ne!(NATIVE_PRINT_PANEL_OPTIONS & (1 << 1), 0, "ShowsPageRange");
        assert_ne!(NATIVE_PRINT_PANEL_OPTIONS & (1 << 2), 0, "ShowsPaperSize");
        assert_ne!(NATIVE_PRINT_PANEL_OPTIONS & (1 << 3), 0, "ShowsOrientation");
        assert_ne!(NATIVE_PRINT_PANEL_OPTIONS & (1 << 4), 0, "ShowsScaling");
        assert_ne!(
            NATIVE_PRINT_PANEL_OPTIONS & (1 << 8),
            0,
            "ShowsPageSetupAccessory"
        );
        assert_ne!(NATIVE_PRINT_PANEL_OPTIONS & (1 << 17), 0, "ShowsPreview");
    }

    /// Current Apple SDKs (MacOSX15.4 / MacOSX26) omit `NSPrinter.paperList`
    /// and `NSPrintTwoSided`. Families' Xcode 26.6 release build fails if
    /// either identifier is compiled. This is a source contract because
    /// this crate's ObjC is not compiled on Linux CI.
    #[test]
    fn native_print_omits_removed_sdk_symbols() {
        let src = include_str!("native_print.m");
        let code: String = src
            .lines()
            .filter(|line| !line.trim_start().starts_with("//"))
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            !code.contains("paperList"),
            "native_print.m must not call -[NSPrinter paperList]"
        );
        assert!(
            !code.contains("NSPrintTwoSided"),
            "native_print.m must not reference NSPrintTwoSided"
        );
        assert!(
            code.contains("PMSetDuplex"),
            "duplex must still go through PMSetDuplex"
        );
        assert!(
            code.contains("paperSize"),
            "PrintSettings paper defaults must still set paperSize"
        );
        for token in [
            "NSPrintPanelShowsCopies",
            "NSPrintPanelShowsPageRange",
            "NSPrintPanelShowsPaperSize",
            "NSPrintPanelShowsOrientation",
            "NSPrintPanelShowsScaling",
            "NSPrintPanelShowsPreview",
            "NSPrintPanelShowsPageSetupAccessory",
            "NSPrintingPaginationModeAutomatic",
            "scalingFactor",
        ] {
            assert!(
                code.contains(token),
                "native_print.m must keep {token} from the standard panel contract"
            );
        }
        assert!(
            !code.contains("NSPrintingPaginationModeClip"),
            "interactive jobs must not force Clip pagination"
        );
        for token in [
            "PerfectPrintComputePosterGrid",
            "PerfectPrintPosterContentBounds",
            "currentPrintPageCount",
            "PerfectPrintPosterTileAt",
            "PERFECT_PRINT_TICK_LENGTH_PT",
            "drawPosterChromeForTile",
        ] {
            assert!(
                code.contains(token),
                "native_print.m must keep {token} for live Scale → page count"
            );
        }
        let tiles_h = include_str!("poster_tiles.h");
        assert!(
            tiles_h.contains("PERFECT_PRINT_JOIN_MARGIN_PT"),
            "poster_tiles.h must define the 28pt join"
        );
        assert!(
            !code.contains("range->length = self.pageNumbers.count"),
            "knowsPageRange must not return the pre-tiled PDF page count"
        );
    }

    fn letter_imageable() -> (f64, f64, f64, f64) {
        (0.0, 0.0, 612.0, 792.0)
    }

    fn landscape_letter_imageable() -> (f64, f64, f64, f64) {
        (0.0, 0.0, 792.0, 612.0)
    }

    #[test]
    fn poster_join_and_tick_constants_match_families() {
        assert_eq!(POSTER_JOIN_MARGIN_PT, 28.0);
        assert_eq!(POSTER_TICK_LENGTH_PT, 6.0);
        assert!(POSTER_TICK_LENGTH_PT <= 8.0);
        let src = include_str!("poster_tiles.h");
        assert!(src.contains("28.0"), "join margin");
        assert!(src.contains("6.0"), "tick length");
        assert!(src.contains("8.0"), "tick max");
    }

    #[test]
    fn fit_to_page_at_100_percent_is_one_page_for_oversized_chart() {
        // Typical Families 5-gen raster vs landscape Letter paper.
        let (cols, rows, pages, _) = inspect_poster_grid(
            2800.0,
            2000.0,
            792.0,
            612.0,
            landscape_letter_imageable(),
            SCALING_FIT_TO_PAGE,
            1.0,
            1.0,
        );
        assert_eq!((cols, rows, pages), (1, 1, 1));
    }

    #[test]
    fn scale_down_from_one_to_one_drops_page_count_to_one_when_it_fits() {
        let avail_w = 792.0 - POSTER_JOIN_MARGIN_PT * 2.0;
        let avail_h = 612.0 - POSTER_JOIN_MARGIN_PT * 2.0;
        let media_w = avail_w * 2.0;
        let media_h = avail_h * 2.0;

        let (_, _, pages_100, _) = inspect_poster_grid(
            media_w,
            media_h,
            792.0,
            612.0,
            landscape_letter_imageable(),
            SCALING_NONE,
            1.0,
            1.0,
        );
        assert_eq!(pages_100, 4, "100% of a 2×2 chart is 4 poster pages");

        let (_, _, pages_50, _) = inspect_poster_grid(
            media_w,
            media_h,
            792.0,
            612.0,
            landscape_letter_imageable(),
            SCALING_NONE,
            1.0,
            0.5,
        );
        assert_eq!(pages_50, 1, "Scale down until it fits must become 1 page");
    }

    #[test]
    fn scale_up_from_fit_grows_page_count() {
        let (cols_fit, rows_fit, pages_fit, _) = inspect_poster_grid(
            1472.0,
            1112.0,
            792.0,
            612.0,
            landscape_letter_imageable(),
            SCALING_FIT_TO_PAGE,
            1.0,
            1.0,
        );
        assert_eq!((cols_fit, rows_fit, pages_fit), (1, 1, 1));

        let (cols, rows, pages, _) = inspect_poster_grid(
            1472.0,
            1112.0,
            792.0,
            612.0,
            landscape_letter_imageable(),
            SCALING_FIT_TO_PAGE,
            1.0,
            2.0,
        );
        assert!(pages > 1, "Scale up from Fit must create poster pages");
        assert_eq!((cols, rows, pages), (2, 2, 4));
    }

    #[test]
    fn poster_tiles_are_row_major_ltr_then_ttb_without_overlap() {
        let scale = 1.0;
        let tiles: Vec<PosterTile> = (0..4)
            .map(|i| inspect_poster_tile(i, 80.0, 60.0, scale, 40.0, 30.0).expect("tile"))
            .collect();
        assert_eq!(
            tiles
                .iter()
                .map(|t| (t.col, t.row, t.index))
                .collect::<Vec<_>>(),
            vec![(0, 0, 0), (1, 0, 1), (0, 1, 2), (1, 1, 3)]
        );
        assert!((tiles[0].src_x - 0.0).abs() < 1e-9 && (tiles[0].src_y - 0.0).abs() < 1e-9);
        assert!((tiles[1].src_x - 40.0).abs() < 1e-9 && (tiles[1].src_y - 0.0).abs() < 1e-9);
        assert!((tiles[2].src_x - 0.0).abs() < 1e-9 && (tiles[2].src_y - 30.0).abs() < 1e-9);
        assert!((tiles[3].src_x - 40.0).abs() < 1e-9 && (tiles[3].src_y - 30.0).abs() < 1e-9);
        for a in 0..tiles.len() {
            for b in a + 1..tiles.len() {
                let ta = &tiles[a];
                let tb = &tiles[b];
                let overlap_x = ta.src_x < tb.src_x + tb.src_w && tb.src_x < ta.src_x + ta.src_w;
                let overlap_y = ta.src_y < tb.src_y + tb.src_h && tb.src_y < ta.src_y + ta.src_h;
                assert!(
                    !(overlap_x && overlap_y),
                    "tiles {a} and {b} overlap in content"
                );
            }
        }
        assert!(inspect_poster_tile(4, 80.0, 60.0, scale, 40.0, 30.0).is_none());
    }

    #[test]
    fn poster_last_column_is_remainder_not_stretched() {
        let tile = inspect_poster_tile(1, 100.0, 50.0, 1.0, 60.0, 50.0).expect("right tile");
        assert_eq!((tile.col, tile.row), (1, 0));
        assert!((tile.src_x - 60.0).abs() < 1e-9);
        assert!((tile.src_w - 40.0).abs() < 1e-9);
        assert!((tile.dest_w - 40.0).abs() < 1e-9);
        assert!((tile.dest_h - 50.0).abs() < 1e-9);
    }

    #[test]
    fn content_bounds_use_28pt_join_inside_imageable() {
        // Full-paper imageable (preview / Save PDF): content is paper − 56pt.
        let (_, _, pages, _) = inspect_poster_grid(
            736.0,
            556.0,
            792.0,
            612.0,
            landscape_letter_imageable(),
            SCALING_NONE,
            1.0,
            1.0,
        );
        assert_eq!(pages, 1);

        let (_, _, overflow, _) = inspect_poster_grid(
            737.0,
            556.0,
            792.0,
            612.0,
            landscape_letter_imageable(),
            SCALING_NONE,
            1.0,
            1.0,
        );
        assert_eq!(overflow, 2);
    }

    #[test]
    fn chrome_contract_hides_label_when_one_page() {
        let src = include_str!("native_print.m");
        assert!(
            src.contains("grid.pages <= 1 || totalPages <= 1"),
            "chrome must hide when N=1"
        );
        assert!(
            src.contains("%ld of %lu") || src.contains(" of "),
            "chrome must draw post-scale n of N when N>1"
        );
        assert!(
            src.contains("PERFECT_PRINT_TICK_LENGTH_PT"),
            "registration ticks live in the join margin"
        );
    }

    #[test]
    fn letter_portrait_paper_still_fits_small_content() {
        let (_, _, pages, _) = inspect_poster_grid(
            400.0,
            300.0,
            612.0,
            792.0,
            letter_imageable(),
            SCALING_NONE,
            1.0,
            1.0,
        );
        assert_eq!(pages, 1);
    }

    #[test]
    fn native_settings_pass_paper_orientation_and_scaling_as_defaults() {
        let settings = PrintSettings::default()
            .paper_size(PageSize::A3)
            .orientation(PageOrientation::Landscape)
            .scaling(PrintScaling::FitToPage)
            .duplex(DuplexMode::LongEdge);
        let page_size = settings.paper_size.to_size();
        assert_eq!(page_size.width, PageSize::A3.to_size().width);
        assert_eq!(page_size.height, PageSize::A3.to_size().height);
        assert!(matches!(settings.orientation, PageOrientation::Landscape));
        assert!(matches!(settings.scaling, PrintScaling::FitToPage));
        // The native helper applies these to NSPrintInfo before the sheet;
        // printPanel.options (ShowsPaperSize/Orientation/Scaling) keep them
        // user-overridable. This mapping must not treat them as locked.
        assert_ne!(NATIVE_PRINT_PANEL_OPTIONS & (1 << 2), 0);
        assert_ne!(NATIVE_PRINT_PANEL_OPTIONS & (1 << 3), 0);
        assert_ne!(NATIVE_PRINT_PANEL_OPTIONS & (1 << 4), 0);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn native_panel_applies_standard_options_and_does_not_force_clip() {
        let mut options = 0u32;
        let mut horizontal = 0u32;
        let mut vertical = 0u32;
        let rc = unsafe {
            perfect_print_native_inspect_panel_defaults(
                &mut options,
                &mut horizontal,
                &mut vertical,
            )
        };
        assert_eq!(rc, 1, "inspect helper should configure a print operation");
        assert_eq!(
            unsafe { perfect_print_native_panel_options_mask() },
            NATIVE_PRINT_PANEL_OPTIONS
        );
        assert_eq!(
            options & NATIVE_PRINT_PANEL_OPTIONS,
            NATIVE_PRINT_PANEL_OPTIONS,
            "operation.printPanel.options missing required bits: {options:#x}"
        );
        assert_ne!(
            horizontal, NATIVE_PRINT_PAGINATION_CLIP,
            "horizontal pagination must not be Clip"
        );
        assert_ne!(
            vertical, NATIVE_PRINT_PAGINATION_CLIP,
            "vertical pagination must not be Clip"
        );
    }

    #[test]
    fn test_enumerate_printers() {
        let dialog = MacosPrintDialog::new();
        let printers = dialog.enumerate_printers();
        eprintln!("Found {} printers", printers.len());
        for p in &printers {
            eprintln!(
                "  {} (default: {}, color: {}, duplex: {})",
                p.capabilities.name,
                p.capabilities.is_default,
                p.capabilities.supports_color,
                p.capabilities.supports_duplex
            );
        }
        // Should not panic
    }

    #[test]
    fn test_default_printer() {
        let dialog = MacosPrintDialog::new();
        match dialog.default_printer() {
            Ok(p) => eprintln!("Default printer: {}", p.capabilities.name),
            Err(PrintError::NoPrinters) => eprintln!("No printers (expected in CI)"),
            Err(e) => eprintln!("Error: {}", e),
        }
    }

    #[test]
    fn test_show_print_dialog() {
        let dialog = MacosPrintDialog::new();
        let settings = PrintSettings::default();
        let result = dialog.show_print_dialog(&settings, Some("Test"));
        assert!(result.is_ok());
    }

    #[test]
    fn test_submit_job_invalid_file() {
        let dialog = MacosPrintDialog::new();
        let path = std::path::Path::new("/tmp/nonexistent_12345.pdf");
        let result = dialog.submit_print_job(path, &PrintSettings::default());
        assert!(result.is_err(), "Should fail for nonexistent file");
    }

    #[test]
    fn test_paper_sizes_include_standard() {
        let dialog = MacosPrintDialog::new();
        let printers = dialog.enumerate_printers();
        for p in &printers {
            assert!(
                p.capabilities.paper_sizes.contains(&PageSize::Letter),
                "Printer {} should support Letter",
                p.capabilities.name
            );
        }
    }
}
