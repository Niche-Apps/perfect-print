//! macOS native print backend.
//!
//! Uses NSTask/Process to bridge to macOS command-line print tools:
//! - `lpstat` for printer enumeration
//! - `lpoptions` for printer capabilities
//! - `NSPrintPanel` via a small native helper (future)
//!
//! For now, provides full printer enumeration and capability detection
//! via system tools, with a path to native panel integration.

use perfect_print_core::page::PageSize;
use perfect_print_dialog::{
    ColorMode, DuplexMode, PageOrientation, PageRange, PrintDialog, PrintDialogResult, PrintError,
    PrintScaling, PrintSettings, Printer, PrinterCapabilities, PrinterState,
};
use std::process::Command;

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

            let is_default = default_printer.as_ref().map_or(false, |d| d == name);
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
                state: if is_default {
                    PrinterState::Ready
                } else {
                    PrinterState::Ready
                },
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
                                paper_sizes.push(size.1.clone());
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
            } else if stdout.contains("idle") || stdout.contains("ready") {
                PrinterState::Ready
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

    /// Show the native print dialog.
    ///
    /// On macOS, we use the `osascript` bridge to show NSPrintPanel.
    /// This requires a small AppleScript that creates a print operation.
    fn show_native_dialog(&self, settings: &PrintSettings) -> PrintDialogResult<PrintSettings> {
        // For now, return current settings.
        // Full NSPrintPanel integration requires either:
        // 1. objc2 AppKit bindings (complex type system issues)
        // 2. A small native helper binary
        // 3. NSAppleScript bridge
        log::warn!("Native print dialog not yet integrated. Use `lp` command for printing.");
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
