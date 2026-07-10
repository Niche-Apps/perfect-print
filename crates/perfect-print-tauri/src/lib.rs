//! Tauri integration for perfect-print.
//!
//! Provides a `TauriPrintDialog` that sends canonical Perfect Print documents
//! to the operating system's native print backend. It never depends on
//! JavaScript `window.print()` or a hidden webview.
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
use perfect_print_dialog::PrintError;
use perfect_print_dialog::{
    PrintDialog, PrintDialogResult, PrintSettings, Printer, PrinterCapabilities,
};

/// Tauri print dialog backed by Perfect Print's canonical renderer.
///
/// The window handle keeps this adapter aligned with the host Tauri app. On
/// macOS, submission renders the canonical model to PDF bytes and opens the
/// native `NSPrintPanel`; unattended platform backends remain available through
/// the core API.
#[derive(Default)]
pub struct TauriPrintDialog {
    _private: (),
}

impl TauriPrintDialog {
    /// Create a new Tauri print dialog.
    ///
    /// The window parameter associates this adapter with the host app. The
    /// document is rendered and submitted by Perfect Print rather than by
    /// executing JavaScript in the webview.
    #[cfg(feature = "tauri")]
    pub fn new(_tauri_window: &tauri::WebviewWindow) -> Self {
        Self { _private: () }
    }

    /// Create a stub dialog (non-Tauri builds).
    #[cfg(not(feature = "tauri"))]
    pub fn new() -> Self {
        Self { _private: () }
    }
}

impl PrintDialog for TauriPrintDialog {
    fn show_print_dialog(
        &self,
        settings: &PrintSettings,
        _document_title: Option<&str>,
    ) -> PrintDialogResult<PrintSettings> {
        let _ = settings;
        Err(PrintError::Platform(
            "A print dialog requires document content; use submit_print_job".to_string(),
        ))
    }

    fn show_page_setup(&self, settings: &PrintSettings) -> PrintDialogResult<PrintSettings> {
        Ok(settings.clone())
    }

    fn available_printers(&self) -> PrintDialogResult<Vec<Printer>> {
        #[cfg(target_os = "macos")]
        {
            return perfect_print_backend_macos::MacosPrintDialog::new().available_printers();
        }
        #[cfg(not(target_os = "macos"))]
        Ok(vec![])
    }

    fn default_printer(&self) -> PrintDialogResult<Printer> {
        #[cfg(target_os = "macos")]
        {
            return perfect_print_backend_macos::MacosPrintDialog::new()
                .default_printer()
                .or_else(|_| Ok(Printer::new(PrinterCapabilities::generic("System Printer"))));
        }
        #[cfg(not(target_os = "macos"))]
        Ok(Printer::new(PrinterCapabilities::generic("System Printer")))
    }
}

/// Submit a canonical document through the operating system print backend.
///
/// On macOS this renders directly to PDF bytes and presents NSPrintPanel on
/// AppKit's main thread. Other platforms use their native Perfect Print
/// backend. The webview parameter is retained for source compatibility.
#[cfg(feature = "tauri")]
pub fn submit_print_job(
    _webview_window: &tauri::WebviewWindow,
    model: &DocumentModel,
    settings: &PrintSettings,
) -> PrintDialogResult<Option<String>> {
    perfect_print::print_document_with(model, settings)
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
