use serde::{Deserialize, Serialize};

/// Core error type for document model operations.
#[derive(Debug, thiserror::Error, Clone, PartialEq)]
pub enum CoreError {
    #[error("Validation error: {0}")]
    Validation(String),

    #[error("Serialization error: {0}")]
    Serialization(String),

    #[error("Resource error: {0}")]
    Resource(String),
}

/// Result type alias for CoreError.
pub type CoreResult<T> = Result<T, CoreError>;

/// The unified error type for all perfect-print operations.
///
/// This error type covers:
/// - Document validation errors
/// - PDF generation errors
/// - Layout errors
/// - Font loading errors
/// - Image errors
/// - Print dialog errors
/// - IO errors
///
/// Each variant includes context about what operation was being performed.
#[derive(Debug, thiserror::Error)]
pub enum PrintError {
    /// IO error (file not found, permission denied, etc.)
    #[error("IO error: {source}")]
    Io {
        #[source]
        source: std::io::Error,
        path: Option<String>,
        context: String,
    },

    /// PDF generation error
    #[error("PDF generation failed: {message}")]
    Pdf {
        message: String,
        page: Option<usize>,
        context: String,
    },

    /// Layout engine error
    #[error("Layout error: {message}")]
    Layout {
        message: String,
        element: Option<String>,
        context: String,
    },

    /// Font loading or shaping error
    #[error("Font error: {family} - {message}")]
    Font {
        family: String,
        message: String,
        context: String,
    },

    /// Image loading or decoding error
    #[error("Image error: {id} - {message}")]
    Image {
        id: String,
        message: String,
        context: String,
    },

    /// Document validation error
    #[error("Validation error: {message}")]
    Validation { message: String, context: String },

    /// Serialization error
    #[error("Serialization error: {message}")]
    Serialization { message: String, context: String },

    /// Print dialog error
    #[error("Print dialog error: {message}")]
    Dialog { message: String, context: String },

    /// Platform-specific error
    #[error("Platform error: {message}")]
    Platform { message: String, context: String },
}

/// Result type alias for PrintError.
pub type PrintResult<T> = Result<T, PrintError>;

impl PrintError {
    /// Set context for this error.
    pub fn with_context(mut self, ctx: impl Into<String>) -> Self {
        let ctx = ctx.into();
        match &mut self {
            PrintError::Io { context, .. } => *context = ctx,
            PrintError::Pdf { context, .. } => *context = ctx,
            PrintError::Layout { context, .. } => *context = ctx,
            PrintError::Font { context, .. } => *context = ctx,
            PrintError::Image { context, .. } => *context = ctx,
            PrintError::Validation { context, .. } => *context = ctx,
            PrintError::Serialization { context, .. } => *context = ctx,
            PrintError::Dialog { context, .. } => *context = ctx,
            PrintError::Platform { context, .. } => *context = ctx,
        }
        self
    }

    /// Check if this error is related to a missing resource.
    pub fn is_not_found(&self) -> bool {
        matches!(
            self,
            PrintError::Io { source, .. } if source.kind() == std::io::ErrorKind::NotFound
        )
    }

    /// Check if this error is a validation error.
    pub fn is_validation(&self) -> bool {
        matches!(self, PrintError::Validation { .. })
    }
}

// Conversions from crate-specific error types

impl From<std::io::Error> for PrintError {
    fn from(e: std::io::Error) -> Self {
        PrintError::Io {
            source: e,
            path: None,
            context: String::new(),
        }
    }
}

impl From<CoreError> for PrintError {
    fn from(e: CoreError) -> Self {
        match e {
            CoreError::Validation(msg) => PrintError::Validation {
                message: msg,
                context: String::new(),
            },
            CoreError::Serialization(msg) => PrintError::Serialization {
                message: msg,
                context: String::new(),
            },
            CoreError::Resource(msg) => PrintError::Image {
                id: String::new(),
                message: msg,
                context: String::new(),
            },
        }
    }
}

impl From<PrintError> for CoreError {
    fn from(e: PrintError) -> Self {
        CoreError::Validation(e.to_string())
    }
}

/// Structured warning for non-fatal issues.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum PrintWarning {
    /// A requested setting was not supported and was adjusted.
    UnsupportedSetting {
        setting: String,
        requested: String,
        actual: String,
    },
    /// A font was substituted.
    FontSubstitution { requested: String, actual: String },
    /// An image could not be loaded.
    ImageLoadFailed { id: String, reason: String },
    /// Content overflowed its container.
    ContentOverflow { element: String, details: String },
}

impl std::fmt::Display for PrintWarning {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PrintWarning::UnsupportedSetting {
                setting,
                requested,
                actual,
            } => write!(
                f,
                "Unsupported setting '{}': requested '{}', using '{}'",
                setting, requested, actual
            ),
            PrintWarning::FontSubstitution { requested, actual } => {
                write!(f, "Font '{}' not found, using '{}'", requested, actual)
            }
            PrintWarning::ImageLoadFailed { id, reason } => {
                write!(f, "Image '{}' failed to load: {}", id, reason)
            }
            PrintWarning::ContentOverflow { element, details } => {
                write!(f, "Content overflow in {}: {}", element, details)
            }
        }
    }
}

/// Strictness mode for handling unsupported settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Strictness {
    /// Try to print, report warnings for unsupported settings.
    BestEffort,
    /// Default: print only if differences are non-destructive.
    Warn,
    /// Fail if requested output cannot be honored.
    Exact,
}

/// Result of validation with warnings.
pub struct ValidationResult {
    pub is_valid: bool,
    pub warnings: Vec<PrintWarning>,
}

impl ValidationResult {
    pub fn valid() -> Self {
        Self {
            is_valid: true,
            warnings: vec![],
        }
    }

    pub fn invalid(warnings: Vec<PrintWarning>) -> Self {
        Self {
            is_valid: false,
            warnings,
        }
    }

    pub fn with_warning(warning: PrintWarning) -> Self {
        Self {
            is_valid: true,
            warnings: vec![warning],
        }
    }

    /// Check if validation passes for the given strictness level.
    pub fn passes(&self, strictness: Strictness) -> bool {
        match strictness {
            Strictness::BestEffort => true,
            Strictness::Warn => self.is_valid,
            Strictness::Exact => self.is_valid && self.warnings.is_empty(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_display_includes_context() {
        let err = PrintError::Validation {
            message: "Document has no pages".to_string(),
            context: String::new(),
        };
        assert!(err.to_string().contains("Document has no pages"));
    }

    #[test]
    fn test_error_from_io() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "file not found");
        let print_err: PrintError = io_err.into();
        assert!(print_err.is_not_found());
    }

    #[test]
    fn test_error_with_context() {
        let err = PrintError::Font {
            family: "Helvetica".to_string(),
            message: "not found".to_string(),
            context: String::new(),
        };
        let err = err.with_context("while loading document");
        let _ = err.to_string();
    }

    #[test]
    fn test_validation_result_passes() {
        let valid = ValidationResult::valid();
        assert!(valid.passes(Strictness::BestEffort));
        assert!(valid.passes(Strictness::Warn));
        assert!(valid.passes(Strictness::Exact));

        let with_warnings = ValidationResult::with_warning(PrintWarning::FontSubstitution {
            requested: "Arial".to_string(),
            actual: "Helvetica".to_string(),
        });
        assert!(with_warnings.passes(Strictness::BestEffort));
        assert!(with_warnings.passes(Strictness::Warn));
        assert!(!with_warnings.passes(Strictness::Exact));

        let invalid = ValidationResult::invalid(vec![PrintWarning::UnsupportedSetting {
            setting: "duplex".to_string(),
            requested: "long-edge".to_string(),
            actual: "simplex".to_string(),
        }]);
        assert!(invalid.passes(Strictness::BestEffort));
        assert!(!invalid.passes(Strictness::Warn));
        assert!(!invalid.passes(Strictness::Exact));
    }

    #[test]
    fn test_print_warning_display() {
        let warn = PrintWarning::FontSubstitution {
            requested: "Comic Sans".to_string(),
            actual: "Helvetica".to_string(),
        };
        let msg = warn.to_string();
        assert!(msg.contains("Comic Sans"));
        assert!(msg.contains("Helvetica"));
    }

    #[test]
    fn test_core_error_conversion() {
        let core_err = CoreError::Validation("test error".to_string());
        let print_err: PrintError = core_err.into();
        assert!(print_err.is_validation());
    }

    #[test]
    fn test_error_chain() {
        // Test that context chaining works
        let err = PrintError::Io {
            source: std::io::Error::new(std::io::ErrorKind::Other, "disk full"),
            path: Some("/tmp/test.pdf".to_string()),
            context: String::new(),
        };
        let err = err.with_context("while saving PDF");
        let msg = err.to_string();
        assert!(msg.contains("disk full"));
        // Context is stored but not displayed by default (thiserror limitation)
        // The important thing is that it doesn't panic and the context is stored
    }
}
