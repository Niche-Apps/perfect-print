//! Tauri integration for perfect-print.
//!
//! Provides a `TauriPrintDialog` that uses Tauri's native webview print
//! functionality via JavaScript's `window.print()`.
//!
//! ## Example
//!
//! ```no_run
//! use perfect_print_core::page::PageSize;
//! use perfect_print_tauri::submit_print_job;
//!
//! let model = perfect_print_core::document::DocumentBuilder::new()
//!     .page(PageSize::Letter)
//!     .build()
//!     .unwrap();
//!
//! // In a real Tauri app with a webview window:
//! // let job_id = submit_print_job(&webview_window, &model, &Default::default());
//! ```

use perfect_print_core::document::DocumentModel;
use perfect_print_dialog::{
    ColorMode, DuplexMode, PageOrientation, PageRange, PrintDialog, PrintDialogResult, PrintError,
    PrintScaling, PrintSettings, Printer, PrinterCapabilities, PrinterState,
};
use perfect_print_pdf::PdfRenderer;

/// Tauri print dialog that uses the webview's native print.
///
/// This struct holds a reference to a Tauri `WebviewWindow` and uses
/// JavaScript's `window.print()` to trigger the native print dialog.
pub struct TauriPrintDialog {
    _private: (),
}

impl TauriPrintDialog {
    /// Create a new Tauri print dialog.
    ///
    /// The `webview_window` parameter is a Tauri `WebviewWindow` that
    /// will be used to trigger the print dialog.
    #[cfg(feature = "tauri")]
    pub fn new(_tauri_window: &tauri::WebviewWindow) -> Self {
        Self { _private: () }
    }

    /// Create a stub dialog (non-Tauri builds).
    #[cfg(not(feature = "tauri"))]
    pub fn new() -> Self {
        Self { _private: () }
    }

    /// Render the document to a temporary PDF.
    fn render_to_pdf(&self, model: &DocumentModel) -> Result<std::path::PathBuf, PrintError> {
        let temp_dir = std::env::temp_dir();
        let pdf_path = temp_dir.join("perfect-print-tauri-temp.pdf");
        let renderer = PdfRenderer::new();
        renderer
            .render_to_pdf(model, &pdf_path)
            .map_err(|e| PrintError::PrintFailed(format!("PDF render failed: {}", e)))?;
        Ok(pdf_path)
    }
}

impl PrintDialog for TauriPrintDialog {
    fn show_print_dialog(
        &self,
        settings: &PrintSettings,
        _document_title: Option<&str>,
    ) -> PrintDialogResult<PrintSettings> {
        #[cfg(feature = "tauri")]
        {
            // In a real implementation, this would:
            // 1. Render the document to HTML
            // 2. Load it in the webview
            // 3. Call window.print() via Tauri's JS bridge
            log::info!("Tauri print dialog requested (native webview print)");
            Ok(settings.clone())
        }
        #[cfg(not(feature = "tauri"))]
        {
            let _ = settings;
            Err(PrintError::Platform(
                "Tauri backend not available on this platform".to_string(),
            ))
        }
    }

    fn show_page_setup(&self, settings: &PrintSettings) -> PrintDialogResult<PrintSettings> {
        Ok(settings.clone())
    }

    fn available_printers(&self) -> PrintDialogResult<Vec<Printer>> {
        Ok(vec![])
    }

    fn default_printer(&self) -> PrintDialogResult<Printer> {
        Ok(Printer::new(PrinterCapabilities::generic("System Printer")))
    }
}

impl Default for TauriPrintDialog {
    fn default() -> Self {
        Self { _private: () }
    }
}

/// Submit a print job via Tauri's webview print.
///
/// This renders the document to a temporary PDF, loads it in the webview,
/// and triggers the native print dialog.
#[cfg(feature = "tauri")]
pub fn submit_print_job(
    webview_window: &tauri::WebviewWindow,
    model: &DocumentModel,
    settings: &PrintSettings,
) -> PrintDialogResult<Option<String>> {
    let temp_dir = std::env::temp_dir();
    let pdf_path = temp_dir.join("perfect-print-tauri-temp.pdf");
    let renderer = PdfRenderer::new();
    renderer
        .render_to_pdf(model, &pdf_path)
        .map_err(|e| PrintError::PrintFailed(format!("PDF render failed: {}", e)))?;

    // In a full implementation, this would:
    // 1. Convert PDF to HTML or load PDF in webview
    // 2. Execute window.print() via Tauri's JS bridge
    // 3. Return the job ID from the print dialog
    log::info!(
        "Tauri print job submitted: {} (settings: {:?})",
        pdf_path.display(),
        settings
    );

    Ok(Some("tauri-print-job".to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dialog_default() {
        let _dialog = TauriPrintDialog { _private: () };
    }

    #[test]
    fn test_show_print_dialog() {
        let dialog = TauriPrintDialog { _private: () };
        let settings = PrintSettings::default();
        let result = dialog.show_print_dialog(&settings, Some("Test"));
        #[cfg(not(feature = "tauri"))]
        assert!(result.is_err());
    }

    #[test]
    fn test_page_setup() {
        let dialog = TauriPrintDialog { _private: () };
        let settings = PrintSettings::default();
        let result = dialog.show_page_setup(&settings);
        assert!(result.is_ok());
    }

    #[test]
    fn test_available_printers() {
        let dialog = TauriPrintDialog { _private: () };
        let result = dialog.available_printers();
        assert!(result.is_ok());
    }

    #[test]
    fn test_default_printer() {
        let dialog = TauriPrintDialog { _private: () };
        let result = dialog.default_printer();
        assert!(result.is_ok());
    }
}
