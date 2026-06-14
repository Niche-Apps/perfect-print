//! Printer settings, printer capabilities, native dialog abstraction.
//!
//! This crate provides:
//! - `PrintSettings`: paper size, orientation, copies, color mode, duplex, etc.
//! - `PrinterCapabilities`: what a printer supports (paper sizes, color, duplex, resolution)
//! - `PrintDialog`: abstract interface for native print dialogs
//! - `PrintError`: structured errors for unsupported settings

use perfect_print_core::page::PageSize;
use thiserror::Error;

/// Print settings for a print job.
#[derive(Debug, Clone, PartialEq)]
pub struct PrintSettings {
    /// Paper size to print on.
    pub paper_size: PageSize,
    /// Page orientation.
    pub orientation: PageOrientation,
    /// Number of copies.
    pub copies: u32,
    /// Color mode.
    pub color_mode: ColorMode,
    /// Duplex mode.
    pub duplex: DuplexMode,
    /// Print resolution in DPI.
    pub resolution: Option<u32>,
    /// Page range to print.
    pub page_range: PageRange,
    /// Scaling mode.
    pub scaling: PrintScaling,
    /// Whether to print page borders.
    pub borderless: bool,
    /// Collate copies.
    pub collate: bool,
}

impl Default for PrintSettings {
    fn default() -> Self {
        Self {
            paper_size: PageSize::Letter,
            orientation: PageOrientation::Portrait,
            copies: 1,
            color_mode: ColorMode::Color,
            duplex: DuplexMode::Simplex,
            resolution: None,
            page_range: PageRange::All,
            scaling: PrintScaling::FitToPage,
            borderless: false,
            collate: true,
        }
    }
}

impl PrintSettings {
    /// Create new print settings with defaults.
    pub fn new() -> Self {
        Self::default()
    }

    /// Builder: set paper size.
    pub fn paper_size(mut self, size: PageSize) -> Self {
        self.paper_size = size;
        self
    }

    /// Builder: set orientation.
    pub fn orientation(mut self, orientation: PageOrientation) -> Self {
        self.orientation = orientation;
        self
    }

    /// Builder: set copies.
    pub fn copies(mut self, copies: u32) -> Self {
        self.copies = copies.max(1);
        self
    }

    /// Builder: set color mode.
    pub fn color_mode(mut self, mode: ColorMode) -> Self {
        self.color_mode = mode;
        self
    }

    /// Builder: set duplex mode.
    pub fn duplex(mut self, duplex: DuplexMode) -> Self {
        self.duplex = duplex;
        self
    }

    /// Builder: set resolution.
    pub fn resolution(mut self, dpi: u32) -> Self {
        self.resolution = Some(dpi);
        self
    }

    /// Builder: set page range.
    pub fn page_range(mut self, range: PageRange) -> Self {
        self.page_range = range;
        self
    }

    /// Builder: set scaling.
    pub fn scaling(mut self, scaling: PrintScaling) -> Self {
        self.scaling = scaling;
        self
    }

    /// Validate these settings against printer capabilities.
    ///
    /// Returns Ok(()) if all settings are supported, or a list of warnings/errors.
    pub fn validate(&self, caps: &PrinterCapabilities) -> PrintDialogResult<Vec<PrintWarning>> {
        let mut warnings = Vec::new();

        // Check paper size
        if !caps.paper_sizes.contains(&self.paper_size) {
            // Find the closest supported size
            warnings.push(PrintWarning::UnsupportedPaperSize {
                requested: self.paper_size,
                fallback: caps
                    .paper_sizes
                    .first()
                    .copied()
                    .unwrap_or(PageSize::Letter),
            });
        }

        // Check color mode
        if self.color_mode == ColorMode::Color && !caps.supports_color {
            warnings.push(PrintWarning::UnsupportedColorMode {
                requested: self.color_mode.clone(),
                fallback: ColorMode::Monochrome,
            });
        }

        // Check duplex
        if self.duplex != DuplexMode::Simplex && !caps.supports_duplex {
            warnings.push(PrintWarning::UnsupportedDuplex {
                requested: self.duplex.clone(),
                fallback: DuplexMode::Simplex,
            });
        }

        // Check resolution against supported list
        if let Some(dpi) = self.resolution {
            // Check if the exact resolution is in the supported list
            if !caps.supported_resolutions.is_empty() && !caps.supported_resolutions.contains(&dpi)
            {
                // Find the closest supported resolution (rounding down)
                let closest = caps
                    .supported_resolutions
                    .iter()
                    .filter(|&&r| r <= dpi)
                    .max()
                    .copied()
                    .or(caps.supported_resolutions.first().copied())
                    .unwrap_or(dpi);
                if closest != dpi {
                    warnings.push(PrintWarning::UnsupportedResolution {
                        requested: dpi,
                        fallback: closest,
                    });
                }
            } else if let Some(max) = caps.max_resolution {
                if dpi > max {
                    warnings.push(PrintWarning::UnsupportedResolution {
                        requested: dpi,
                        fallback: max,
                    });
                }
            }
        }

        // Check borderless
        if self.borderless && !caps.supports_borderless {
            warnings.push(PrintWarning::UnsupportedBorderless);
        }

        Ok(warnings)
    }

    /// Apply fallback values for unsupported settings.
    ///
    /// Returns a new PrintSettings with unsupported values replaced by fallbacks.
    pub fn apply_fallbacks(&self, caps: &PrinterCapabilities) -> Self {
        let mut settings = self.clone();

        if !caps.paper_sizes.contains(&self.paper_size) {
            settings.paper_size = caps
                .paper_sizes
                .first()
                .copied()
                .unwrap_or(PageSize::Letter);
        }

        if self.color_mode == ColorMode::Color && !caps.supports_color {
            settings.color_mode = ColorMode::Monochrome;
        }

        if self.duplex != DuplexMode::Simplex && !caps.supports_duplex {
            settings.duplex = DuplexMode::Simplex;
        }

        if let Some(dpi) = self.resolution {
            if let Some(max) = caps.max_resolution {
                if dpi > max {
                    settings.resolution = Some(max);
                }
            }
        }

        if self.borderless && !caps.supports_borderless {
            settings.borderless = false;
        }

        settings
    }
}

/// Page orientation.
#[derive(Debug, Clone, PartialEq)]
pub enum PageOrientation {
    Portrait,
    Landscape,
    ReversePortrait,
    ReverseLandscape,
}

/// Color mode for printing.
#[derive(Debug, Clone, PartialEq)]
pub enum ColorMode {
    Color,
    Monochrome,
    Grayscale,
}

/// Duplex (double-sided) printing mode.
#[derive(Debug, Clone, PartialEq)]
pub enum DuplexMode {
    /// Single-sided printing.
    Simplex,
    /// Double-sided, flip on long edge.
    LongEdge,
    /// Double-sided, flip on short edge.
    ShortEdge,
}

/// Page range to print.
#[derive(Debug, Clone, PartialEq)]
pub enum PageRange {
    /// Print all pages.
    All,
    /// Print a specific range (start, end) inclusive, 1-indexed.
    Range(u32, u32),
    /// Print specific pages, 1-indexed.
    Pages(Vec<u32>),
}

/// Print scaling options.
#[derive(Debug, Clone, PartialEq)]
pub enum PrintScaling {
    /// Scale to fit the printable area.
    FitToPage,
    /// Scale to fill (may crop).
    FillPage,
    /// No scaling (1:1).
    None,
    /// Custom scale factor (1.0 = 100%).
    Custom(f64),
}

/// Warnings generated when settings don't match printer capabilities.
#[derive(Debug, Clone, PartialEq)]
pub enum PrintWarning {
    UnsupportedPaperSize {
        requested: PageSize,
        fallback: PageSize,
    },
    UnsupportedColorMode {
        requested: ColorMode,
        fallback: ColorMode,
    },
    UnsupportedDuplex {
        requested: DuplexMode,
        fallback: DuplexMode,
    },
    UnsupportedResolution {
        requested: u32,
        fallback: u32,
    },
    UnsupportedBorderless,
}

impl std::fmt::Display for PrintWarning {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PrintWarning::UnsupportedPaperSize {
                requested,
                fallback,
            } => {
                write!(
                    f,
                    "Paper size {:?} not supported, using {:?}",
                    requested, fallback
                )
            }
            PrintWarning::UnsupportedColorMode {
                requested,
                fallback,
            } => {
                write!(
                    f,
                    "Color mode {:?} not supported, using {:?}",
                    requested, fallback
                )
            }
            PrintWarning::UnsupportedDuplex {
                requested,
                fallback,
            } => {
                write!(
                    f,
                    "Duplex mode {:?} not supported, using {:?}",
                    requested, fallback
                )
            }
            PrintWarning::UnsupportedResolution {
                requested,
                fallback,
            } => {
                write!(
                    f,
                    "Resolution {} DPI not supported, using {}",
                    requested, fallback
                )
            }
            PrintWarning::UnsupportedBorderless => {
                write!(f, "Borderless printing not supported, using bordered")
            }
        }
    }
}

/// Capabilities of a printer.
#[derive(Debug, Clone, PartialEq)]
pub struct PrinterCapabilities {
    /// Printer name.
    pub name: String,
    /// Available paper sizes.
    pub paper_sizes: Vec<PageSize>,
    /// Supports color printing.
    pub supports_color: bool,
    /// Supports duplex (double-sided) printing.
    pub supports_duplex: bool,
    /// Maximum resolution in DPI.
    pub max_resolution: Option<u32>,
    /// Supported resolutions.
    pub supported_resolutions: Vec<u32>,
    /// Supports borderless printing.
    pub supports_borderless: bool,
    /// Whether this is the default printer.
    pub is_default: bool,
    /// Printer state.
    pub state: PrinterState,
}

impl PrinterCapabilities {
    /// Create a minimal set of capabilities for a generic printer.
    pub fn generic(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            paper_sizes: vec![PageSize::Letter, PageSize::A4],
            supports_color: true,
            supports_duplex: false,
            max_resolution: Some(300),
            supported_resolutions: vec![150, 300],
            supports_borderless: false,
            is_default: false,
            state: PrinterState::Ready,
        }
    }

    /// Check if a specific paper size is supported.
    pub fn supports_paper_size(&self, size: &PageSize) -> bool {
        self.paper_sizes.contains(size)
    }

    /// Check if a specific resolution is supported.
    pub fn supports_resolution(&self, dpi: u32) -> bool {
        self.supported_resolutions.contains(&dpi)
            || self.max_resolution.map_or(false, |max| dpi <= max)
    }
}

/// Printer state.
#[derive(Debug, Clone, PartialEq)]
pub enum PrinterState {
    Ready,
    Busy,
    Offline,
    Error(String),
    PaperJam,
    OutOfPaper,
    OutOfInk,
    DoorOpen,
}

/// A printer that can be printed to.
#[derive(Debug, Clone)]
pub struct Printer {
    pub capabilities: PrinterCapabilities,
}

impl Printer {
    pub fn new(capabilities: PrinterCapabilities) -> Self {
        Self { capabilities }
    }

    /// Get the printer's capabilities.
    pub fn capabilities(&self) -> &PrinterCapabilities {
        &self.capabilities
    }

    /// Check if the printer is ready.
    pub fn is_ready(&self) -> bool {
        self.capabilities.state == PrinterState::Ready
    }
}

/// Result type for print dialog operations.
pub type PrintDialogResult<T> = Result<T, PrintError>;

/// Errors from print dialog operations.
#[derive(Debug, Error)]
pub enum PrintError {
    #[error("No printers available")]
    NoPrinters,

    #[error("Printer '{0}' not found")]
    PrinterNotFound(String),

    #[error("Print job cancelled by user")]
    Cancelled,

    #[error("Print error: {0}")]
    PrintFailed(String),

    #[error("Invalid settings: {0}")]
    InvalidSettings(String),

    #[error("Platform error: {0}")]
    Platform(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

/// Abstract interface for native print dialogs.
///
/// Platform backends implement this trait to show native print dialogs.
pub trait PrintDialog {
    /// Show a print dialog and return the user's settings.
    ///
    /// Returns `PrintError::Cancelled` if the user cancels.
    fn show_print_dialog(
        &self,
        settings: &PrintSettings,
        document_title: Option<&str>,
    ) -> PrintDialogResult<PrintSettings>;

    /// Show a page setup dialog and return the user's page settings.
    ///
    /// Returns `PrintError::Cancelled` if the user cancels.
    fn show_page_setup(&self, settings: &PrintSettings) -> PrintDialogResult<PrintSettings>;

    /// Get available printers on the system.
    fn available_printers(&self) -> PrintDialogResult<Vec<Printer>>;

    /// Get the default printer.
    fn default_printer(&self) -> PrintDialogResult<Printer>;
}

/// No-op print dialog implementation for headless/CI environments.
///
/// Always returns the provided settings unchanged and reports no printers.
pub struct NoOpDialog;

impl PrintDialog for NoOpDialog {
    fn show_print_dialog(
        &self,
        settings: &PrintSettings,
        _document_title: Option<&str>,
    ) -> PrintDialogResult<PrintSettings> {
        Ok(settings.clone())
    }

    fn show_page_setup(&self, settings: &PrintSettings) -> PrintDialogResult<PrintSettings> {
        Ok(settings.clone())
    }

    fn available_printers(&self) -> PrintDialogResult<Vec<Printer>> {
        Ok(vec![])
    }

    fn default_printer(&self) -> PrintDialogResult<Printer> {
        Ok(Printer::new(PrinterCapabilities::generic(
            "Default Printer",
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_settings() {
        let settings = PrintSettings::default();
        assert_eq!(settings.paper_size, PageSize::Letter);
        assert_eq!(settings.orientation, PageOrientation::Portrait);
        assert_eq!(settings.copies, 1);
        assert_eq!(settings.color_mode, ColorMode::Color);
        assert_eq!(settings.duplex, DuplexMode::Simplex);
    }

    #[test]
    fn test_settings_builder() {
        let settings = PrintSettings::default()
            .paper_size(PageSize::A4)
            .orientation(PageOrientation::Landscape)
            .copies(3)
            .color_mode(ColorMode::Monochrome)
            .duplex(DuplexMode::LongEdge)
            .resolution(600);

        assert_eq!(settings.paper_size, PageSize::A4);
        assert_eq!(settings.orientation, PageOrientation::Landscape);
        assert_eq!(settings.copies, 3);
        assert_eq!(settings.color_mode, ColorMode::Monochrome);
        assert_eq!(settings.duplex, DuplexMode::LongEdge);
        assert_eq!(settings.resolution, Some(600));
    }

    #[test]
    fn test_copies_clamps_to_1() {
        let settings = PrintSettings::default().copies(0);
        assert_eq!(settings.copies, 1);
    }

    #[test]
    fn test_validate_supported_settings() {
        let caps = PrinterCapabilities::generic("Test");
        let settings = PrintSettings::default();

        let warnings = settings.validate(&caps).unwrap();
        assert!(
            warnings.is_empty(),
            "Default settings should be valid for generic printer"
        );
    }

    #[test]
    fn test_validate_unsupported_paper_size() {
        let mut caps = PrinterCapabilities::generic("Test");
        caps.paper_sizes = vec![PageSize::Letter]; // Only Letter

        let settings = PrintSettings::default().paper_size(PageSize::Legal);

        let warnings = settings.validate(&caps).unwrap();
        assert!(!warnings.is_empty());
        assert!(warnings
            .iter()
            .any(|w| matches!(w, PrintWarning::UnsupportedPaperSize { .. })));
    }

    #[test]
    fn test_validate_unsupported_color() {
        let mut caps = PrinterCapabilities::generic("Test");
        caps.supports_color = false;

        let settings = PrintSettings::default().color_mode(ColorMode::Color);

        let warnings = settings.validate(&caps).unwrap();
        assert!(!warnings.is_empty());
        assert!(warnings
            .iter()
            .any(|w| matches!(w, PrintWarning::UnsupportedColorMode { .. })));
    }

    #[test]
    fn test_validate_unsupported_duplex() {
        let caps = PrinterCapabilities::generic("Test");
        // Generic printer doesn't support duplex

        let settings = PrintSettings::default().duplex(DuplexMode::LongEdge);

        let warnings = settings.validate(&caps).unwrap();
        assert!(!warnings.is_empty());
        assert!(warnings
            .iter()
            .any(|w| matches!(w, PrintWarning::UnsupportedDuplex { .. })));
    }

    #[test]
    fn test_validate_unsupported_resolution() {
        let caps = PrinterCapabilities::generic("Test");
        // Generic max is 300

        let settings = PrintSettings::default().resolution(1200);

        let warnings = settings.validate(&caps).unwrap();
        assert!(!warnings.is_empty());
        assert!(warnings
            .iter()
            .any(|w| matches!(w, PrintWarning::UnsupportedResolution { .. })));
    }

    #[test]
    fn test_apply_fallbacks() {
        let mut caps = PrinterCapabilities::generic("Test");
        caps.paper_sizes = vec![PageSize::Letter];
        caps.supports_color = false;
        caps.supports_duplex = false;
        caps.max_resolution = Some(300);

        let settings = PrintSettings::default()
            .paper_size(PageSize::Legal)
            .color_mode(ColorMode::Color)
            .duplex(DuplexMode::LongEdge)
            .resolution(600);

        let fixed = settings.apply_fallbacks(&caps);
        assert_eq!(fixed.paper_size, PageSize::Letter);
        assert_eq!(fixed.color_mode, ColorMode::Monochrome);
        assert_eq!(fixed.duplex, DuplexMode::Simplex);
        assert_eq!(fixed.resolution, Some(300));
    }

    #[test]
    fn test_no_op_dialog() {
        let dialog = NoOpDialog;
        let settings = PrintSettings::default();

        let result = dialog.show_print_dialog(&settings, Some("Test")).unwrap();
        assert_eq!(result.paper_size, PageSize::Letter);

        let result = dialog.show_page_setup(&settings).unwrap();
        assert_eq!(result.paper_size, PageSize::Letter);

        let printers = dialog.available_printers().unwrap();
        assert!(printers.is_empty());

        let default = dialog.default_printer().unwrap();
        assert!(default.is_ready());
    }

    #[test]
    fn test_printer_capabilities() {
        let caps = PrinterCapabilities::generic("Test");
        assert!(caps.supports_paper_size(&PageSize::Letter));
        assert!(caps.supports_paper_size(&PageSize::A4));
        assert!(!caps.supports_paper_size(&PageSize::Legal));
        assert!(caps.supports_resolution(300));
        assert!(caps.supports_resolution(150));
        assert!(!caps.supports_resolution(1200));
    }

    #[test]
    fn test_printer_state() {
        let mut caps = PrinterCapabilities::generic("Test");
        assert_eq!(caps.state, PrinterState::Ready);
        assert!(Printer::new(caps.clone()).is_ready());

        caps.state = PrinterState::Offline;
        assert!(!Printer::new(caps).is_ready());
    }

    #[test]
    fn test_print_warning_display() {
        let w = PrintWarning::UnsupportedPaperSize {
            requested: PageSize::Legal,
            fallback: PageSize::Letter,
        };
        let msg = format!("{}", w);
        assert!(msg.contains("Legal"));
        assert!(msg.contains("Letter"));
    }

    #[test]
    fn test_page_range() {
        assert_eq!(PageRange::All, PageRange::All);
        assert_eq!(PageRange::Range(1, 5), PageRange::Range(1, 5));
        assert_eq!(
            PageRange::Pages(vec![1, 3, 5]),
            PageRange::Pages(vec![1, 3, 5])
        );
    }

    #[test]
    fn test_print_scaling() {
        assert_eq!(PrintScaling::FitToPage, PrintScaling::FitToPage);
        assert_eq!(PrintScaling::Custom(0.75), PrintScaling::Custom(0.75));
    }
}
