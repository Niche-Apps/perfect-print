//! Linux CUPS/IPP backend.
//!
//! Uses the CUPS library via the `cups-sys` crate:
//! - `cupsGetDests` for printer enumeration
//! - `cupsPrintFile` for job submission
//! - `cupsGetJobs` / `cupsFreeJobs` for job tracking
//! - `cupsCancelJob` for job cancellation

use perfect_print_core::page::PageSize;
use perfect_print_dialog::{
    ColorMode, DuplexMode, PageOrientation, PageRange, PrintDialog, PrintDialogResult, PrintError,
    PrintScaling, PrintSettings, Printer, PrinterCapabilities, PrinterState,
};
use std::ffi::CString;
use std::path::Path;

#[cfg(target_os = "linux")]
use cups_sys::*;

/// Linux CUPS/IPP print backend.
pub struct LinuxPrintDialog;

impl LinuxPrintDialog {
    pub fn new() -> Self {
        Self
    }

    /// Enumerate all printers on the system via `cupsGetDests`.
    #[cfg(target_os = "linux")]
    fn enumerate_printers(&self) -> Vec<Printer> {
        let mut printers = Vec::new();

        let mut num_dests: i32 = 0;
        let mut dests: *mut cups_dest_t = std::ptr::null_mut();

        unsafe {
            num_dests = cupsGetDests(&mut dests);
        }

        if num_dests <= 0 || dests.is_null() {
            return printers;
        }

        let dest_slice = unsafe { std::slice::from_raw_parts(dests, num_dests as usize) };

        for dest in dest_slice {
            let name = if dest.name.is_null() {
                continue;
            } else {
                unsafe { std::ffi::CStr::from_ptr(dest.name) }
                    .to_string_lossy()
                    .to_string()
            };

            let is_default = dest.is_default != 0;

            // Parse options for capabilities
            let mut supports_color = false;
            let mut supports_duplex = false;
            let mut paper_sizes = vec![PageSize::Letter, PageSize::A4];

            let num_options = dest.num_options;
            if num_options > 0 && !dest.options.is_null() {
                let options =
                    unsafe { std::slice::from_raw_parts(dest.options, num_options as usize) };
                for opt in options {
                    if opt.name.is_null() || opt.value.is_null() {
                        continue;
                    }
                    let key = unsafe { std::ffi::CStr::from_ptr(opt.name) }
                        .to_string_lossy()
                        .to_lowercase();
                    let val = unsafe { std::ffi::CStr::from_ptr(opt.value) }
                        .to_string_lossy()
                        .to_lowercase();

                    if key.contains("color") || key.contains("cmyk") || key.contains("rgb") {
                        supports_color = true;
                    }
                    if key.contains("duplex")
                        || key.contains("double-sided")
                        || key.contains("two-sided")
                    {
                        supports_duplex = true;
                    }
                    if key == "media" || key == "papersize" {
                        if let Some(ps) = paper_size_from_cups(&val) {
                            if !paper_sizes.contains(&ps) {
                                paper_sizes.push(ps);
                            }
                        }
                    }
                }
            }

            let caps = PrinterCapabilities {
                name: name.clone(),
                paper_sizes,
                supports_color,
                supports_duplex,
                max_resolution: None,
                supported_resolutions: vec![],
                supports_borderless: false,
                is_default,
                state: PrinterState::Ready,
            };

            printers.push(Printer::new(caps));
        }

        unsafe {
            cupsFreeDests(num_dests, dests);
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
    #[cfg(target_os = "linux")]
    fn get_default_printer_name() -> Option<String> {
        let mut num_dests: i32 = 0;
        let mut dests: *mut cups_dest_t = std::ptr::null_mut();

        unsafe {
            num_dests = cupsGetDests(&mut dests);
        }

        if num_dests <= 0 || dests.is_null() {
            return None;
        }

        let dest_slice = unsafe { std::slice::from_raw_parts(dests, num_dests as usize) };
        let result = dest_slice.iter().find(|d| d.is_default != 0).map(|d| {
            unsafe { std::ffi::CStr::from_ptr(d.name) }
                .to_string_lossy()
                .to_string()
        });

        unsafe {
            cupsFreeDests(num_dests, dests);
        }

        result
    }

    /// Submit a print job via `cupsPrintFile`.
    #[cfg(target_os = "linux")]
    pub fn submit_print_job(
        &self,
        pdf_path: &Path,
        settings: &PrintSettings,
    ) -> PrintDialogResult<Option<String>> {
        let printer_name = self
            .get_default_printer_name()
            .ok_or(PrintError::NoPrinters)?;

        let path_cstr = CString::new(pdf_path.to_string_lossy().as_ref())
            .map_err(|e| PrintError::Platform(format!("Invalid path: {}", e)))?;

        let title_cstr = CString::new("perfect-print job").unwrap();

        // Build CUPS options from settings
        let mut options: Vec<(CString, CString)> = Vec::new();

        // Copies
        if settings.copies > 1 {
            options.push((
                CString::new("copies").unwrap(),
                CString::new(settings.copies.to_string()).unwrap(),
            ));
        }

        // Page range
        match &settings.page_range {
            PageRange::All => {}
            PageRange::Range(start, end) => {
                options.push((
                    CString::new("page-ranges").unwrap(),
                    CString::new(format!("{}-{}", start, end)).unwrap(),
                ));
            }
            PageRange::Pages(pages) => {
                let range: Vec<String> = pages.iter().map(|p| p.to_string()).collect();
                options.push((
                    CString::new("page-ranges").unwrap(),
                    CString::new(range.join(",")).unwrap(),
                ));
            }
        }

        // Duplex
        if settings.duplex != DuplexMode::Simplex {
            let sides = match settings.duplex {
                DuplexMode::LongEdge => "two-sided-long-edge",
                DuplexMode::ShortEdge => "two-sided-short-edge",
                DuplexMode::Simplex => unreachable!(),
            };
            options.push((CString::new("sides").unwrap(), CString::new(sides).unwrap()));
        }

        // Orientation
        if settings.orientation == PageOrientation::Landscape {
            options.push((
                CString::new("orientation-requested").unwrap(),
                CString::new("4").unwrap(),
            ));
        }

        // Fit to page
        match &settings.scaling {
            PrintScaling::FitToPage => {
                options.push((
                    CString::new("fit-to-page").unwrap(),
                    CString::new("true").unwrap(),
                ));
            }
            PrintScaling::FillPage => {
                options.push((CString::new("fill").unwrap(), CString::new("true").unwrap()));
            }
            PrintScaling::None => {}
            PrintScaling::Custom(s) => {
                options.push((
                    CString::new("scaling").unwrap(),
                    CString::new(format!("{}", (s * 100.0) as u32)).unwrap(),
                ));
            }
        }

        // Collate
        if settings.collate && settings.copies > 1 {
            options.push((
                CString::new("Collate").unwrap(),
                CString::new("True").unwrap(),
            ));
        }

        // Paper size
        let paper_str = match &settings.paper_size {
            PageSize::Letter => "Letter",
            PageSize::A4 => "A4",
            PageSize::Legal => "Legal",
            PageSize::Tabloid => "Tabloid",
            PageSize::A3 => "A3",
            PageSize::A5 => "A5",
            _ => "Letter",
        };
        options.push((
            CString::new("media").unwrap(),
            CString::new(paper_str).unwrap(),
        ));

        // Convert to raw pointers for the FFI call
        let mut opt_names: Vec<*const i8> = options.iter().map(|(k, _)| k.as_ptr()).collect();
        let mut opt_values: Vec<*const i8> = options.iter().map(|(_, v)| v.as_ptr()).collect();
        let num_options = options.len() as i32;

        let job_id = unsafe {
            cupsPrintFile(
                printer_name.as_ptr() as *const i8,
                path_cstr.as_ptr(),
                title_cstr.as_ptr(),
                num_options,
                opt_names.as_mut_ptr(),
                opt_values.as_mut_ptr(),
            )
        };

        if job_id == 0 {
            return Err(PrintError::PrintFailed("cupsPrintFile failed".to_string()));
        }

        log::info!(
            "Print job submitted: {} to printer '{}' (job {})",
            pdf_path.display(),
            printer_name,
            job_id
        );

        Ok(Some(format!("{}-{}", printer_name, job_id)))
    }

    /// Poll the status of a print job.
    #[cfg(target_os = "linux")]
    pub fn poll_job_status(&self, job_id: &str) -> PrintDialogResult<bool> {
        let (printer_name, job_num) = job_id
            .rsplit_once('-')
            .ok_or_else(|| PrintError::InvalidSettings("Invalid job ID format".to_string()))?;

        let job_num: i32 = job_num
            .parse()
            .map_err(|_| PrintError::InvalidSettings("Invalid job ID number".to_string()))?;

        let name_cstr = CString::new(printer_name)
            .map_err(|e| PrintError::Platform(format!("Invalid printer name: {}", e)))?;

        let mut num_jobs: i32 = 0;
        let mut jobs: *mut cups_job_t = std::ptr::null_mut();

        unsafe {
            num_jobs = cupsGetJobs(&mut jobs, name_cstr.as_ptr(), 0, 0);
        }

        if num_jobs <= 0 || jobs.is_null() {
            // No jobs found, assume completed
            return Ok(true);
        }

        let job_slice = unsafe { std::slice::from_raw_parts(jobs, num_jobs as usize) };
        let found = job_slice.iter().find(|j| j.id == job_num);

        let completed = match found {
            None => true, // Job not in list, assume completed
            Some(job) => {
                // Check state: CUPS_JOB_COMPLETED = 5, CUPS_JOB_CANCELED = 7, CUPS_JOB_ABORTED = 8
                job.state == 5 || job.state == 7 || job.state == 8
            }
        };

        unsafe {
            cupsFreeJobs(num_jobs, jobs);
        }

        Ok(completed)
    }

    /// List all pending print jobs.
    #[cfg(target_os = "linux")]
    pub fn list_jobs(&self) -> PrintDialogResult<Vec<(String, String, String)>> {
        let mut jobs = Vec::new();
        let printers = self.enumerate_printers();

        for printer in &printers {
            let name_cstr = CString::new(printer.capabilities.name.clone())
                .map_err(|e| PrintError::Platform(format!("Invalid printer name: {}", e)))?;

            let mut num_jobs: i32 = 0;
            let mut cups_jobs: *mut cups_job_t = std::ptr::null_mut();

            unsafe {
                num_jobs = cupsGetJobs(&mut cups_jobs, name_cstr.as_ptr(), 0, 0);
            }

            if num_jobs > 0 && !cups_jobs.is_null() {
                let job_slice = unsafe { std::slice::from_raw_parts(cups_jobs, num_jobs as usize) };
                for job in job_slice {
                    let id = format!("{}-{}", printer.capabilities.name, job.id);
                    let status = match job.state {
                        3 => "pending",    // CUPS_JOB_PENDING
                        4 => "held",       // CUPS_JOB_HELD
                        5 => "completed",  // CUPS_JOB_COMPLETED
                        6 => "processing", // CUPS_JOB_PROCESSING
                        7 => "canceled",   // CUPS_JOB_CANCELED
                        8 => "aborted",    // CUPS_JOB_ABORTED
                        _ => "unknown",
                    };
                    jobs.push((id, printer.capabilities.name.clone(), status.to_string()));
                }
            }

            unsafe {
                cupsFreeJobs(num_jobs, cups_jobs);
            }
        }

        Ok(jobs)
    }

    /// Cancel a print job.
    #[cfg(target_os = "linux")]
    pub fn cancel_job(&self, job_id: &str) -> PrintDialogResult<()> {
        let (printer_name, job_num) = job_id
            .rsplit_once('-')
            .ok_or_else(|| PrintError::InvalidSettings("Invalid job ID format".to_string()))?;

        let job_num: i32 = job_num
            .parse()
            .map_err(|_| PrintError::InvalidSettings("Invalid job ID number".to_string()))?;

        let name_cstr = CString::new(printer_name)
            .map_err(|e| PrintError::Platform(format!("Invalid printer name: {}", e)))?;

        let result = unsafe { cupsCancelJob(name_cstr.as_ptr(), job_num) };

        if result == 0 {
            return Err(PrintError::PrintFailed("cupsCancelJob failed".to_string()));
        }

        log::info!("Print job {} cancelled", job_id);
        Ok(())
    }
}

/// Map CUPS paper size names to our PageSize enum.
#[cfg(target_os = "linux")]
fn paper_size_from_cups(name: &str) -> Option<PageSize> {
    let lower = name.to_lowercase();
    if lower.contains("letter") {
        Some(PageSize::Letter)
    } else if lower.contains("a4") {
        Some(PageSize::A4)
    } else if lower.contains("legal") {
        Some(PageSize::Legal)
    } else if lower.contains("tabloid") || lower.contains("ledger") {
        Some(PageSize::Tabloid)
    } else if lower.contains("a3") {
        Some(PageSize::A3)
    } else if lower.contains("a5") {
        Some(PageSize::A5)
    } else {
        None
    }
}

// ===== Non-Linux stub implementations =====

#[cfg(not(target_os = "linux"))]
impl LinuxPrintDialog {
    fn enumerate_printers(&self) -> Vec<Printer> {
        vec![]
    }

    pub fn submit_print_job(
        &self,
        _pdf_path: &Path,
        _settings: &PrintSettings,
    ) -> PrintDialogResult<Option<String>> {
        Err(PrintError::Platform(
            "Linux backend not available on this platform".to_string(),
        ))
    }

    pub fn poll_job_status(&self, _job_id: &str) -> PrintDialogResult<bool> {
        Err(PrintError::Platform(
            "Linux backend not available on this platform".to_string(),
        ))
    }

    pub fn list_jobs(&self) -> PrintDialogResult<Vec<(String, String, String)>> {
        Err(PrintError::Platform(
            "Linux backend not available on this platform".to_string(),
        ))
    }

    pub fn cancel_job(&self, _job_id: &str) -> PrintDialogResult<()> {
        Err(PrintError::Platform(
            "Linux backend not available on this platform".to_string(),
        ))
    }
}

impl PrintDialog for LinuxPrintDialog {
    fn show_print_dialog(
        &self,
        settings: &PrintSettings,
        _document_title: Option<&str>,
    ) -> PrintDialogResult<PrintSettings> {
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

impl Default for LinuxPrintDialog {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dialog_trait_impl() {
        let dialog = LinuxPrintDialog::new();
        let settings = PrintSettings::default();
        let result = dialog.show_print_dialog(&settings, Some("Test"));
        assert!(result.is_ok());
    }

    #[test]
    fn test_page_setup_dialog() {
        let dialog = LinuxPrintDialog::new();
        let settings = PrintSettings::default();
        let result = dialog.show_page_setup(&settings);
        assert!(result.is_ok());
    }

    #[test]
    fn test_available_printers() {
        let dialog = LinuxPrintDialog::new();
        let result = dialog.available_printers();
        assert!(result.is_ok());
        #[cfg(not(target_os = "linux"))]
        assert!(result.unwrap().is_empty());
    }

    #[test]
    fn test_default_printer() {
        let dialog = LinuxPrintDialog::new();
        #[cfg(not(target_os = "linux"))]
        {
            let result = dialog.default_printer();
            assert!(matches!(result, Err(PrintError::NoPrinters)));
        }
    }

    #[test]
    fn test_submit_job_nonexistent_file() {
        let dialog = LinuxPrintDialog::new();
        let path = Path::new("/tmp/nonexistent_12345.pdf");
        let result = dialog.submit_print_job(path, &PrintSettings::default());
        assert!(result.is_err(), "Should fail for nonexistent file");
    }
}
