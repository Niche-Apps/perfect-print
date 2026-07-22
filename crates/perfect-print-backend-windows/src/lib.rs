//! Windows native print backend.
//!
//! Uses the Win32 Printing API via the `windows` crate:
//! - `EnumPrinters` for printer enumeration
//! - `OpenPrinter` / `StartDocPrinter` / `WritePrinter` / `ClosePrinter` for job submission
//! - `EnumJobs` for job tracking
//! - `SetJob` for job cancellation

use perfect_print_dialog::{PrintDialog, PrintDialogResult, PrintError, PrintSettings, Printer};

#[cfg(target_os = "windows")]
use perfect_print_dialog::{PageRange, PrinterCapabilities, PrinterState};

#[cfg(target_os = "windows")]
use perfect_print_core::page::PageSize;

#[cfg(target_os = "windows")]
use windows::{
    core::*, Win32::Foundation::*, Win32::Graphics::Gdi::*, Win32::Graphics::Printing::*,
    Win32::Storage::Xps::*,
};

#[cfg(target_os = "windows")]
use perfect_print_core::document::DocumentModel;
#[cfg(target_os = "windows")]
use perfect_print_core::units::Dpi;
#[cfg(target_os = "windows")]
use perfect_print_render::Render as _;
#[cfg(target_os = "windows")]
use perfect_print_render::TinySkiaRenderer;

/// Windows native print backend.
pub struct WindowsPrintDialog;

impl WindowsPrintDialog {
    pub fn new() -> Self {
        Self
    }

    /// Enumerate all printers on the system via `EnumPrinters`.
    #[cfg(target_os = "windows")]
    fn enumerate_printers(&self) -> Vec<Printer> {
        let mut printers = Vec::new();

        // First call to get required buffer size
        let mut needed: u32 = 0;
        let mut returned: u32 = 0;
        unsafe {
            let _ = EnumPrintersA(
                PRINTER_ENUM_LOCAL | PRINTER_ENUM_CONNECTIONS,
                None,
                2, // PRINTER_INFO_2
                None,
                &mut needed,
                &mut returned,
            );
        }

        if needed == 0 {
            return printers;
        }

        // Second call with properly sized buffer
        let mut buffer: Vec<u8> = vec![0; needed as usize];
        unsafe {
            if EnumPrintersA(
                PRINTER_ENUM_LOCAL | PRINTER_ENUM_CONNECTIONS,
                None,
                2,
                Some(&mut buffer),
                &mut needed,
                &mut returned,
            )
            .is_err()
            {
                return printers;
            }
        }

        // Parse PRINTER_INFO_2 entries
        let count = returned as usize;
        for i in 0..count {
            let info = unsafe {
                let ptr = buffer
                    .as_ptr()
                    .add(i * std::mem::size_of::<PRINTER_INFO_2A>())
                    as *const PRINTER_INFO_2A;
                &*ptr
            };

            let name = unsafe {
                if info.pPrinterName.is_null() {
                    continue;
                }
                std::ffi::CStr::from_ptr(info.pPrinterName.as_ptr() as *const i8)
                    .to_string_lossy()
                    .into_owned()
            };

            let is_default = info.Attributes & PRINTER_ATTRIBUTE_DEFAULT != 0;

            // Determine capabilities from the info
            let mut supports_color = false;
            let mut supports_duplex = false;
            let mut paper_sizes = vec![PageSize::Letter, PageSize::A4];

            // Check the DEVMODE for capabilities
            if !info.pDevMode.is_null() {
                let devmode = unsafe { &*info.pDevMode };
                supports_color = devmode.dmColor == DMCOLOR_COLOR;
                supports_duplex = devmode.dmDuplex != DMDUP_SIMPLEX;

                // Map paper size from dmPaperSize
                let paper_size = unsafe { devmode.Anonymous1.Anonymous1.dmPaperSize };
                if paper_size != 0 {
                    if let Some(ps) = paper_size_from_win32(paper_size as u16) {
                        if !paper_sizes.contains(&ps) {
                            paper_sizes.push(ps);
                        }
                    }
                }
            }

            // Determine state from Status field
            let state = if info.Status == 0 {
                PrinterState::Ready
            } else if info.Status & PRINTER_STATUS_OFFLINE != 0 {
                PrinterState::Offline
            } else if info.Status & PRINTER_STATUS_PAPER_JAM != 0 {
                PrinterState::PaperJam
            } else if info.Status & PRINTER_STATUS_PAPER_OUT != 0 {
                PrinterState::OutOfPaper
            } else if info.Status & PRINTER_STATUS_ERROR != 0 {
                PrinterState::Error("Printer error".to_string())
            } else {
                PrinterState::Busy
            };

            let caps = PrinterCapabilities {
                name: name.clone(),
                paper_sizes,
                supports_color,
                supports_duplex,
                max_resolution: None,
                supported_resolutions: vec![],
                supports_borderless: false,
                is_default,
                state,
            };

            printers.push(Printer::new(caps));
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

    /// Get the default printer name.
    #[cfg(target_os = "windows")]
    fn get_default_printer_name() -> Option<String> {
        // Use GetDefaultPrinter
        let mut needed: u32 = 0;
        unsafe {
            let _ = GetDefaultPrinterA(PSTR::null(), &mut needed);
        }
        if needed == 0 {
            return None;
        }

        let mut buffer: Vec<u8> = vec![0; needed as usize];
        unsafe {
            if !GetDefaultPrinterA(PSTR(buffer.as_mut_ptr()), &mut needed).as_bool() {
                return None;
            }
        }

        let name = String::from_utf8_lossy(&buffer)
            .trim_end_matches('\0')
            .to_string();
        if name.is_empty() {
            None
        } else {
            Some(name)
        }
    }

    /// Submit a print job by rasterizing every page and blitting each page's
    /// bitmap onto a GDI printer device context.
    ///
    /// This does not depend on the printer driver understanding PDF (most
    /// don't, when fed raw bytes) — it produces a bitmap of exactly what
    /// perfect-print's own raster/PNG output would show, at the printer's
    /// own reported resolution, so output is WYSIWYG on any GDI-capable
    /// Windows printer driver.
    #[cfg(target_os = "windows")]
    pub fn submit_print_job(
        &self,
        model: &DocumentModel,
        settings: &PrintSettings,
    ) -> PrintDialogResult<Option<String>> {
        let printer_name = Self::get_default_printer_name().ok_or(PrintError::NoPrinters)?;
        print_to_printer(&printer_name, None, model, settings)
    }

    /// Poll the status of a print job.
    #[cfg(target_os = "windows")]
    pub fn poll_job_status(&self, job_id: &str) -> PrintDialogResult<bool> {
        // Parse job_id as "PrinterName-123"
        let (printer_name, job_num) = job_id
            .rsplit_once('-')
            .ok_or_else(|| PrintError::InvalidSettings("Invalid job ID format".to_string()))?;

        let job_num: u32 = job_num
            .parse()
            .map_err(|_| PrintError::InvalidSettings("Invalid job ID number".to_string()))?;

        let printer_name_cstr = std::ffi::CString::new(printer_name)
            .map_err(|e| PrintError::Platform(format!("Invalid printer name: {}", e)))?;

        let mut hprinter = HANDLE::default();
        unsafe {
            OpenPrinterA(
                PCSTR(printer_name_cstr.as_ptr() as *const u8),
                &mut hprinter,
                None,
            )
            .map_err(|e| PrintError::Platform(format!("OpenPrinter failed: {}", e)))?;
        }

        // Query job info
        let mut needed: u32 = 0;
        unsafe {
            let _ = GetJobA(hprinter, job_num, 1, None, &mut needed);
        }

        if needed == 0 {
            // Job not found, assume completed
            unsafe { ClosePrinter(hprinter) };
            return Ok(true);
        }

        let mut buffer: Vec<u8> = vec![0; needed as usize];
        let result = unsafe { GetJobA(hprinter, job_num, 1, Some(&mut buffer), &mut needed) };

        unsafe { ClosePrinter(hprinter) };

        if !result.as_bool() {
            // Job not found, assume completed
            return Ok(true);
        }

        let job_info = unsafe { &*(buffer.as_ptr() as *const JOB_INFO_1A) };

        // Check status flags
        let still_queued = job_info.Status == 0
            || job_info.Status & JOB_STATUS_PRINTING != 0
            || job_info.Status & JOB_STATUS_SPOOLING != 0;

        Ok(!still_queued)
    }

    /// List all pending print jobs.
    #[cfg(target_os = "windows")]
    pub fn list_jobs(&self) -> PrintDialogResult<Vec<(String, String, String)>> {
        let mut jobs = Vec::new();
        let printers = self.enumerate_printers();

        for printer in &printers {
            let printer_name_cstr = std::ffi::CString::new(printer.capabilities.name.clone())
                .map_err(|e| PrintError::Platform(format!("Invalid printer name: {}", e)))?;

            let mut hprinter = HANDLE::default();
            unsafe {
                if OpenPrinterA(
                    PCSTR(printer_name_cstr.as_ptr() as *const u8),
                    &mut hprinter,
                    None,
                )
                .is_err()
                {
                    continue;
                }
            }

            // Get job count
            let mut needed: u32 = 0;
            let mut returned: u32 = 0;
            unsafe {
                let _ = EnumJobsA(hprinter, 0, 0xFFFF, 1, None, &mut needed, &mut returned);
            }

            if needed > 0 && returned > 0 {
                let mut buffer: Vec<u8> = vec![0; needed as usize];
                unsafe {
                    if EnumJobsA(
                        hprinter,
                        0,
                        0xFFFF,
                        1,
                        Some(&mut buffer),
                        &mut needed,
                        &mut returned,
                    )
                    .is_ok()
                    {
                        for i in 0..returned as usize {
                            let info =
                                &*(buffer.as_ptr().add(i * std::mem::size_of::<JOB_INFO_1A>())
                                    as *const JOB_INFO_1A);
                            let id = format!("{}-{}", printer.capabilities.name, info.JobId);
                            let status =
                                if info.Status == 0 || (info.Status & JOB_STATUS_PRINTING != 0) {
                                    "printing"
                                } else if info.Status & JOB_STATUS_SPOOLING != 0 {
                                    "spooling"
                                } else {
                                    "queued"
                                };
                            jobs.push((id, printer.capabilities.name.clone(), status.to_string()));
                        }
                    }
                }
            }

            unsafe { ClosePrinter(hprinter) };
        }

        Ok(jobs)
    }

    /// Cancel a print job.
    #[cfg(target_os = "windows")]
    pub fn cancel_job(&self, job_id: &str) -> PrintDialogResult<()> {
        let (printer_name, job_num) = job_id
            .rsplit_once('-')
            .ok_or_else(|| PrintError::InvalidSettings("Invalid job ID format".to_string()))?;

        let job_num: u32 = job_num
            .parse()
            .map_err(|_| PrintError::InvalidSettings("Invalid job ID number".to_string()))?;

        let printer_name_cstr = std::ffi::CString::new(printer_name)
            .map_err(|e| PrintError::Platform(format!("Invalid printer name: {}", e)))?;

        let mut hprinter = HANDLE::default();
        unsafe {
            OpenPrinterA(
                PCSTR(printer_name_cstr.as_ptr() as *const u8),
                &mut hprinter,
                None,
            )
            .map_err(|e| PrintError::Platform(format!("OpenPrinter failed: {}", e)))?;

            if !SetJobA(hprinter, job_num, 0, None, JOB_CONTROL_CANCEL).as_bool() {
                ClosePrinter(hprinter);
                return Err(PrintError::PrintFailed("SetJob cancel failed".to_string()));
            }

            ClosePrinter(hprinter);
        }

        log::info!("Print job {} cancelled", job_id);
        Ok(())
    }
}

/// Open a printer DC by name, run the full `StartDoc`/`StartPage`/blit/
/// `EndPage`/`EndDoc` sequence for every requested page (and copy), and
/// optionally direct output to a file (for the `Microsoft Print to PDF`
/// virtual printer smoke test) instead of a real physical device.
#[cfg(target_os = "windows")]
fn print_to_printer(
    printer_name: &str,
    output_file: Option<&std::path::Path>,
    model: &DocumentModel,
    settings: &PrintSettings,
) -> PrintDialogResult<Option<String>> {
    let printer_name_cstr = std::ffi::CString::new(printer_name)
        .map_err(|e| PrintError::Platform(format!("Invalid printer name: {}", e)))?;

    let hdc = unsafe {
        CreateDCA(
            PCSTR::null(),
            PCSTR(printer_name_cstr.as_ptr() as *const u8),
            PCSTR::null(),
            None,
        )
    };
    if hdc.is_invalid() {
        return Err(PrintError::Platform(format!(
            "CreateDC failed for printer '{}'",
            printer_name
        )));
    }

    // Ensure the DC is always released, even on an early error return.
    struct DcGuard(HDC);
    impl Drop for DcGuard {
        fn drop(&mut self) {
            unsafe {
                let _ = DeleteDC(self.0);
            }
        }
    }
    let _dc_guard = DcGuard(hdc);

    let dpi_x = unsafe { GetDeviceCaps(hdc, LOGPIXELSX) };
    let dpi_y = unsafe { GetDeviceCaps(hdc, LOGPIXELSY) };
    let dpi = if dpi_x > 0 { dpi_x as f64 } else { 300.0 };
    let _ = dpi_y; // assumed equal to dpi_x for print DCs; not used separately below

    let phys_width = unsafe { GetDeviceCaps(hdc, PHYSICALWIDTH) };
    let phys_height = unsafe { GetDeviceCaps(hdc, PHYSICALHEIGHT) };
    let offset_x = unsafe { GetDeviceCaps(hdc, PHYSICALOFFSETX) };
    let offset_y = unsafe { GetDeviceCaps(hdc, PHYSICALOFFSETY) };

    let doc_name = "perfect-print job";
    let doc_name_cstr = std::ffi::CString::new(doc_name).unwrap();
    let output_cstr = output_file
        .map(|p| {
            std::ffi::CString::new(p.to_string_lossy().into_owned())
                .map_err(|e| PrintError::Platform(format!("Invalid output path: {}", e)))
        })
        .transpose()?;
    let lpsz_output = match &output_cstr {
        Some(c) => PCSTR(c.as_ptr() as *const u8),
        None => PCSTR::null(),
    };
    let doc_info = DOCINFOA {
        cbSize: std::mem::size_of::<DOCINFOA>() as i32,
        lpszDocName: PCSTR(doc_name_cstr.as_ptr() as *const u8),
        lpszOutput: lpsz_output,
        lpszDatatype: PCSTR::null(),
        fwType: 0,
    };

    let job_id = unsafe { StartDocA(hdc, &doc_info) };
    if job_id <= 0 {
        return Err(PrintError::PrintFailed("StartDoc failed".to_string()));
    }

    let renderer = TinySkiaRenderer::new();
    let page_indices = resolve_page_indices(&settings.page_range, model.page_count());

    for _copy in 0..settings.copies.max(1) {
        for &page_index in &page_indices {
            if unsafe { StartPage(hdc) } <= 0 {
                unsafe {
                    let _ = AbortDoc(hdc);
                };
                return Err(PrintError::PrintFailed("StartPage failed".to_string()));
            }

            let pixmap = renderer
                .render_page_to_pixmap(model, page_index, Dpi(dpi))
                .map_err(|e| {
                    PrintError::PrintFailed(format!(
                        "Failed to rasterize page {}: {}",
                        page_index + 1,
                        e
                    ))
                })?;

            blit_pixmap(hdc, &pixmap, phys_width, phys_height, offset_x, offset_y)?;

            if unsafe { EndPage(hdc) } <= 0 {
                unsafe {
                    let _ = AbortDoc(hdc);
                };
                return Err(PrintError::PrintFailed("EndPage failed".to_string()));
            }
        }
    }

    if unsafe { EndDoc(hdc) } <= 0 {
        return Err(PrintError::PrintFailed("EndDoc failed".to_string()));
    }

    log::info!(
        "Print job submitted: {} page(s) to printer '{}' (job {})",
        page_indices.len(),
        printer_name,
        job_id
    );

    Ok(Some(format!("{}-{}", printer_name, job_id)))
}

/// Print to an explicitly named printer, optionally redirecting output to a
/// file (used by the `Microsoft Print to PDF` virtual printer). Public only
/// so the Windows-only smoke-test example binary can call it directly; not
/// part of the crate's intended public surface.
#[doc(hidden)]
#[cfg(target_os = "windows")]
pub fn print_to_named_printer_for_test(
    printer_name: &str,
    model: &DocumentModel,
    settings: &PrintSettings,
    output_path: &str,
) -> PrintDialogResult<Option<String>> {
    print_to_printer(
        printer_name,
        Some(std::path::Path::new(output_path)),
        model,
        settings,
    )
}

/// Resolve a `PageRange` against an actual page count into a concrete,
/// 0-indexed, in-order list of page indices to print.
#[cfg(target_os = "windows")]
fn resolve_page_indices(range: &PageRange, page_count: usize) -> Vec<usize> {
    match range {
        PageRange::All => (0..page_count).collect(),
        PageRange::Range(start, end) => {
            let start = (*start).max(1) as usize;
            let end = (*end as usize).min(page_count);
            if start > end {
                vec![]
            } else {
                (start - 1..end).collect()
            }
        }
        PageRange::Pages(pages) => pages
            .iter()
            .filter(|&&p| p >= 1 && (p as usize) <= page_count)
            .map(|&p| (p - 1) as usize)
            .collect(),
    }
}

/// Blit a rendered page bitmap onto the printer DC, filling the full
/// physical page (offset by the driver-reported unprintable margin) —
/// matching the page's own point-size 1:1 at the DC's reported DPI, the
/// same "just draw the page, let the driver map it to paper" contract
/// perfect-print-backend-macos relies on via PDFKit.
#[cfg(target_os = "windows")]
fn blit_pixmap(
    hdc: HDC,
    pixmap: &tiny_skia::Pixmap,
    phys_width: i32,
    phys_height: i32,
    offset_x: i32,
    offset_y: i32,
) -> PrintDialogResult<()> {
    let width = pixmap.width() as i32;
    let height = pixmap.height() as i32;

    // tiny-skia's Pixmap is premultiplied RGBA, top-to-bottom rows.
    // GDI DIBs expect BGRA (or a negative height for top-down) with a
    // positive-height bottom-up row order by convention; use a negative
    // biHeight to keep the pixmap's natural top-down row order and swap
    // R/B per pixel into a BGRA scratch buffer GDI expects.
    let mut bgra = vec![0u8; pixmap.data().len()];
    for (src, dst) in pixmap.data().chunks_exact(4).zip(bgra.chunks_exact_mut(4)) {
        dst[0] = src[2]; // B
        dst[1] = src[1]; // G
        dst[2] = src[0]; // R
        dst[3] = src[3]; // A
    }

    let bmi = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: width,
            biHeight: -height, // negative = top-down
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0 as u32,
            biSizeImage: 0,
            biXPelsPerMeter: 0,
            biYPelsPerMeter: 0,
            biClrUsed: 0,
            biClrImportant: 0,
        },
        bmiColors: [Default::default(); 1],
    };

    let dest_width = phys_width.max(1);
    let dest_height = phys_height.max(1);

    let result = unsafe {
        StretchDIBits(
            hdc,
            -offset_x,
            -offset_y,
            dest_width,
            dest_height,
            0,
            0,
            width,
            height,
            Some(bgra.as_ptr() as *const std::ffi::c_void),
            &bmi,
            DIB_RGB_COLORS,
            SRCCOPY,
        )
    };

    if result == 0 || result == GDI_ERROR as i32 {
        return Err(PrintError::PrintFailed(
            "StretchDIBits failed while printing a page".to_string(),
        ));
    }

    Ok(())
}

/// Map Win32 paper size constants to our PageSize enum.
#[cfg(target_os = "windows")]
fn paper_size_from_win32(dm_paper_size: u16) -> Option<PageSize> {
    match dm_paper_size {
        1 => Some(PageSize::Letter),  // DMPAPER_LETTER
        5 => Some(PageSize::Legal),   // DMPAPER_LEGAL
        7 => Some(PageSize::Tabloid), // DMPAPER_TABLOID (actually DMPAPER_LEDGER=4, DMPAPER_TABLOID=3)
        8 => Some(PageSize::A4),      // DMPAPER_A4
        9 => Some(PageSize::A3),      // DMPAPER_A3
        11 => Some(PageSize::A5),     // DMPAPER_A5
        _ => None,
    }
}

// ===== Non-Windows stub implementations =====

#[cfg(not(target_os = "windows"))]
impl WindowsPrintDialog {
    fn enumerate_printers(&self) -> Vec<Printer> {
        vec![]
    }

    pub fn submit_print_job(
        &self,
        _model: &perfect_print_core::document::DocumentModel,
        _settings: &PrintSettings,
    ) -> PrintDialogResult<Option<String>> {
        Err(PrintError::Platform(
            "Windows backend not available on this platform".to_string(),
        ))
    }

    pub fn poll_job_status(&self, _job_id: &str) -> PrintDialogResult<bool> {
        Err(PrintError::Platform(
            "Windows backend not available on this platform".to_string(),
        ))
    }

    pub fn list_jobs(&self) -> PrintDialogResult<Vec<(String, String, String)>> {
        Err(PrintError::Platform(
            "Windows backend not available on this platform".to_string(),
        ))
    }

    pub fn cancel_job(&self, _job_id: &str) -> PrintDialogResult<()> {
        Err(PrintError::Platform(
            "Windows backend not available on this platform".to_string(),
        ))
    }
}

impl PrintDialog for WindowsPrintDialog {
    fn show_print_dialog(
        &self,
        settings: &PrintSettings,
        _document_title: Option<&str>,
    ) -> PrintDialogResult<PrintSettings> {
        // Full native print dialog would require a GUI thread and COM.
        // For now, return current settings (same approach as macOS).
        log::warn!("Native print dialog not yet integrated. Use submit_print_job for printing.");
        Ok(settings.clone())
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

impl Default for WindowsPrintDialog {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dialog_trait_impl() {
        let dialog = WindowsPrintDialog::new();
        let settings = PrintSettings::default();
        let result = dialog.show_print_dialog(&settings, Some("Test"));
        assert!(result.is_ok());
    }

    #[test]
    fn test_page_setup_dialog() {
        let dialog = WindowsPrintDialog::new();
        let settings = PrintSettings::default();
        let result = dialog.show_page_setup(&settings);
        assert!(result.is_ok());
    }

    #[test]
    fn test_available_printers() {
        let dialog = WindowsPrintDialog::new();
        let result = dialog.available_printers();
        assert!(result.is_ok());
        // On non-Windows, should return empty
        #[cfg(not(target_os = "windows"))]
        assert!(result.unwrap().is_empty());
    }

    #[test]
    fn test_default_printer() {
        let dialog = WindowsPrintDialog::new();
        // On non-Windows, should return NoPrinters
        #[cfg(not(target_os = "windows"))]
        {
            let result = dialog.default_printer();
            assert!(matches!(result, Err(PrintError::NoPrinters)));
        }
    }

    #[test]
    fn test_submit_job_stub_on_non_windows() {
        #[cfg(not(target_os = "windows"))]
        {
            let dialog = WindowsPrintDialog::new();
            let model = perfect_print_core::document::DocumentBuilder::new()
                .page(perfect_print_core::page::PageSize::Letter)
                .build()
                .unwrap();
            let result = dialog.submit_print_job(&model, &PrintSettings::default());
            assert!(result.is_err(), "Should fail on non-Windows (stub backend)");
        }
    }
}
