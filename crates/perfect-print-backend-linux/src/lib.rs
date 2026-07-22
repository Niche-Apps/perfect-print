//! Linux CUPS print backend.
//!
//! Uses the CUPS command-line tools (`lp`, `lpstat`, `lpoptions`, `cancel`) —
//! the same tools `perfect-print-backend-macos` uses, since macOS's print
//! system is also CUPS-based. Shelling out avoids depending on `libcups`'s
//! C ABI/header surface (which drifts across CUPS versions and distros) and
//! means this backend's logic is exercised by the exact same code path on
//! any machine with CUPS command-line tools installed — including macOS,
//! which is how this crate's tests validate real behavior in CI/dev even
//! when the target OS is not Linux.

use perfect_print_core::page::PageSize;
use perfect_print_dialog::{
    DuplexMode, PageOrientation, PageRange, PrintDialog, PrintDialogResult,
    PrintError, PrintScaling, PrintSettings, Printer, PrinterCapabilities, PrinterState,
};
use std::path::Path;
use std::process::Command;

/// Linux CUPS print backend (command-line, not `libcups` FFI).
pub struct LinuxPrintDialog;

impl LinuxPrintDialog {
    pub fn new() -> Self {
        Self
    }

    /// Enumerate printers via `lpstat -a`.
    fn enumerate_printers(&self) -> Vec<Printer> {
        let mut printers = Vec::new();

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
            let mut caps = self.get_printer_caps(name);
            caps.is_default = is_default;

            printers.push(Printer::new(caps));
        }

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
                    if lower.starts_with("pagesize=") || lower.contains("pagesize=") {
                        for size in &[
                            ("letter", PageSize::Letter),
                            ("a4", PageSize::A4),
                            ("legal", PageSize::Legal),
                            ("tabloid", PageSize::Tabloid),
                            ("a3", PageSize::A3),
                            ("a5", PageSize::A5),
                        ] {
                            if lower.contains(size.0) && !paper_sizes.contains(&size.1) {
                                paper_sizes.push(size.1);
                            }
                        }
                    }
                }
            }
        }

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

    /// Submit a print job via `lp`.
    pub fn submit_print_job(
        &self,
        pdf_path: &Path,
        settings: &PrintSettings,
    ) -> PrintDialogResult<Option<String>> {
        let mut cmd = Command::new("lp");

        let printers = self.enumerate_printers();
        if let Some(default) = printers.iter().find(|p| p.capabilities.is_default) {
            cmd.arg("-d").arg(&default.capabilities.name);
        }

        if settings.copies > 1 {
            cmd.arg("-n").arg(settings.copies.to_string());
        }

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

        if settings.duplex != DuplexMode::Simplex {
            cmd.arg("-o").arg("sides=two-sided-long-edge");
        }

        if settings.orientation == PageOrientation::Landscape {
            cmd.arg("-o").arg("orientation-requested=4");
        }

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

        if settings.collate && settings.copies > 1 {
            cmd.arg("-o").arg("Collate=True");
        }

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

        cmd.arg("-t").arg("perfect-print job");
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

    /// Poll job status via `lpstat -o`. Returns `Ok(true)` if completed
    /// (no longer in the queue), `Ok(false)` if still queued/printing.
    pub fn poll_job_status(&self, job_id: &str) -> PrintDialogResult<bool> {
        let output = Command::new("lpstat")
            .args(["-o"])
            .output()
            .map_err(|e| PrintError::Platform(format!("Failed to run lpstat: {}", e)))?;

        if !output.status.success() {
            return Err(PrintError::Platform("lpstat -o failed".to_string()));
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let in_queue = stdout.lines().any(|line| line.starts_with(job_id));
        Ok(!in_queue)
    }

    /// List all pending print jobs as (job_id, printer, status) tuples.
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
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 3 {
                let job_id = parts[0].to_string();
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

    /// Cancel a print job by ID via `cancel`.
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

impl PrintDialog for LinuxPrintDialog {
    fn show_print_dialog(
        &self,
        settings: &PrintSettings,
        _document_title: Option<&str>,
    ) -> PrintDialogResult<PrintSettings> {
        log::warn!("Native print dialog not implemented on Linux. Use submit_print_job for printing.");
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
    fn test_available_printers_does_not_panic() {
        let dialog = LinuxPrintDialog::new();
        let result = dialog.available_printers();
        assert!(result.is_ok());
        eprintln!("Found {} printer(s) via lpstat", result.unwrap().len());
    }

    #[test]
    fn test_default_printer_reports_no_printers_or_a_real_one() {
        let dialog = LinuxPrintDialog::new();
        match dialog.default_printer() {
            Ok(p) => eprintln!("Default printer: {}", p.capabilities.name),
            Err(PrintError::NoPrinters) => eprintln!("No printers configured (expected in CI)"),
            Err(e) => panic!("Unexpected error: {e}"),
        }
    }

    #[test]
    fn test_submit_job_nonexistent_file() {
        let dialog = LinuxPrintDialog::new();
        let path = Path::new("/tmp/nonexistent_perfect_print_test_12345.pdf");
        let result = dialog.submit_print_job(path, &PrintSettings::default());
        assert!(result.is_err(), "Should fail for nonexistent file");
    }

    #[test]
    fn test_parse_job_id_extracts_id() {
        let stdout = "request id is HP_LaserJet-42 (1 file(s))\n";
        assert_eq!(
            LinuxPrintDialog::parse_job_id(stdout),
            Some("HP_LaserJet-42".to_string())
        );
    }

    #[test]
    fn test_parse_job_id_handles_missing_marker() {
        assert_eq!(LinuxPrintDialog::parse_job_id("lp: no destinations added"), None);
    }
}
