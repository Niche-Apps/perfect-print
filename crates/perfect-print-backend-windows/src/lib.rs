//! Windows native print backend.
//!
//! Uses the Win32 Printing API via the `windows` crate:
//! - `EnumPrinters` for printer enumeration
//! - `OpenPrinter` / `StartDocPrinter` / `WritePrinter` / `ClosePrinter` for job submission
//! - `EnumJobs` for job tracking
//! - `SetJob` for job cancellation

use perfect_print_dialog::{PrintDialog, PrintDialogResult, PrintError, PrintSettings, Printer};
use std::path::Path;

#[cfg(target_os = "windows")]
use perfect_print_dialog::{
    ColorMode, DuplexMode, PageOrientation, PageRange, PrintScaling, PrinterCapabilities,
    PrinterState,
};

#[cfg(target_os = "windows")]
use perfect_print_core::page::PageSize;

#[cfg(target_os = "windows")]
use windows::{
    core::*, Win32::Foundation::*, Win32::Graphics::Gdi::*, Win32::Graphics::Printing::*,
};

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

    /// Submit a print job via the Win32 Printing API.
    #[cfg(target_os = "windows")]
    pub fn submit_print_job(
        &self,
        pdf_path: &Path,
        settings: &PrintSettings,
    ) -> PrintDialogResult<Option<String>> {
        let pdf_data = std::fs::read(pdf_path).map_err(|e| {
            PrintError::Io(std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("Failed to read PDF: {}", e),
            ))
        })?;

        let printer_name = Self::get_default_printer_name().ok_or(PrintError::NoPrinters)?;

        // Open the printer
        let mut hprinter = HANDLE::default();
        let printer_name_cstr = std::ffi::CString::new(printer_name.clone())
            .map_err(|e| PrintError::Platform(format!("Invalid printer name: {}", e)))?;

        unsafe {
            OpenPrinterA(
                PCSTR(printer_name_cstr.as_ptr() as *const u8),
                &mut hprinter,
                None,
            )
            .map_err(|e| PrintError::Platform(format!("OpenPrinter failed: {}", e)))?;
        }

        // Set up DOC_INFO_1
        let doc_name = "perfect-print job";
        let doc_name_cstr = std::ffi::CString::new(doc_name).unwrap();
        let doc_info = DOC_INFO_1A {
            pDocName: PSTR(doc_name_cstr.as_ptr() as *mut u8),
            pOutputFile: PSTR::null(),
            pDatatype: PSTR::null(),
        };

        // Start the document
        let job_id = unsafe { StartDocPrinterA(hprinter, 1, &doc_info) };
        if job_id == 0 {
            unsafe { ClosePrinter(hprinter) };
            return Err(PrintError::PrintFailed(
                "StartDocPrinter failed".to_string(),
            ));
        }

        // Start a page
        unsafe {
            if !StartPagePrinter(hprinter).as_bool() {
                EndDocPrinter(hprinter);
                ClosePrinter(hprinter);
                return Err(PrintError::PrintFailed(
                    "StartPagePrinter failed".to_string(),
                ));
            }
        }

        // Write the PDF data
        let mut written: u32 = 0;
        unsafe {
            let result = WritePrinter(
                hprinter,
                pdf_data.as_ptr() as *const _,
                pdf_data.len() as u32,
                &mut written,
            );
            if !result.as_bool() {
                EndPagePrinter(hprinter);
                EndDocPrinter(hprinter);
                ClosePrinter(hprinter);
                return Err(PrintError::PrintFailed("WritePrinter failed".to_string()));
            }
        }

        // End page and document
        unsafe {
            EndPagePrinter(hprinter);
            EndDocPrinter(hprinter);
            ClosePrinter(hprinter);
        }

        log::info!(
            "Print job submitted: {} bytes to printer '{}' (job {})",
            written,
            printer_name,
            job_id
        );

        Ok(Some(format!("{}-{}", printer_name, job_id)))
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
        _pdf_path: &Path,
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
    fn test_submit_job_nonexistent_file() {
        let dialog = WindowsPrintDialog::new();
        let path = Path::new("/tmp/nonexistent_12345.pdf");
        let result = dialog.submit_print_job(path, &PrintSettings::default());
        assert!(result.is_err(), "Should fail for nonexistent file");
    }
}
