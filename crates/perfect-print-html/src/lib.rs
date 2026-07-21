mod convert;
mod css;
mod error;
mod policy;
mod readiness;
mod stylesheet;

pub use convert::{convert, ConvertedDocument, LoadedImage, PageSetup};
pub use error::{HtmlRenderError, HtmlRenderStage};
pub use policy::ResourcePolicy;
pub use readiness::ReadinessTracker;
pub use stylesheet::{resolve_color, PageRule, PageSizeSpec, SimpleSelector, Stylesheet};

use std::path::{Path, PathBuf};

use perfect_print::{
    DocumentModel, Dpi, FlowConfig, FlowLayoutEngine, ImageStore, PdfRenderer, Render,
    TinySkiaRenderer,
};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HtmlPageSettings {
    pub width_points: f64,
    pub height_points: f64,
}

impl HtmlPageSettings {
    pub fn letter() -> Self {
        Self::custom(612.0, 792.0)
    }

    pub fn custom(width_points: f64, height_points: f64) -> Self {
        Self {
            width_points,
            height_points,
        }
    }

    fn validate(self) -> Result<(), HtmlRenderError> {
        const MAX_PAGE_POINTS: f64 = 14_400.0;
        if !self.width_points.is_finite()
            || !self.height_points.is_finite()
            || self.width_points <= 0.0
            || self.height_points <= 0.0
            || self.width_points > MAX_PAGE_POINTS
            || self.height_points > MAX_PAGE_POINTS
        {
            return Err(HtmlRenderError::at_stage(
                "HTML_PAGE_SETTINGS_INVALID",
                HtmlRenderStage::Validate,
                "page dimensions must be finite and between 0 and 14,400 points",
            ));
        }
        Ok(())
    }
}

impl Default for HtmlPageSettings {
    fn default() -> Self {
        Self::letter()
    }
}

#[derive(Debug, Clone)]
pub struct HtmlDocument {
    html: String,
    title: Option<String>,
    resource_policy: ResourcePolicy,
    /// `None` means the caller never explicitly called `page_settings()` —
    /// `@page` (and, failing that, the letter default) is free to apply.
    /// `Some(_)` means the caller's explicit setting always wins.
    page_settings: Option<HtmlPageSettings>,
}

impl HtmlDocument {
    pub fn new(html: impl Into<String>) -> Self {
        Self {
            html: html.into(),
            title: None,
            resource_policy: ResourcePolicy::offline(),
            page_settings: None,
        }
    }

    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    pub fn resource_policy(mut self, policy: ResourcePolicy) -> Self {
        self.resource_policy = policy;
        self
    }

    pub fn page_settings(mut self, settings: HtmlPageSettings) -> Self {
        self.page_settings = Some(settings);
        self
    }

    pub fn validate(&self) -> Result<(), HtmlRenderError> {
        if self.html.trim().is_empty() {
            return Err(HtmlRenderError::at_stage(
                "HTML_INPUT_INVALID",
                HtmlRenderStage::Validate,
                "HTML input is empty",
            ));
        }
        self.resource_policy.validate_html_bytes(self.html.len())?;
        self.page().validate()
    }

    pub fn html(&self) -> &str {
        &self.html
    }

    pub fn title_value(&self) -> Option<&str> {
        self.title.as_deref()
    }

    pub fn policy(&self) -> &ResourcePolicy {
        &self.resource_policy
    }

    /// Effective page settings: the caller's explicit setting if any,
    /// otherwise the letter default. `@page` resolution happens later in
    /// `convert()`, which consults `explicit_page_settings()` directly to
    /// implement the full precedence (explicit > `@page` > letter default).
    pub fn page(&self) -> HtmlPageSettings {
        self.page_settings.unwrap_or_default()
    }

    /// `Some(_)` only if the caller explicitly called `page_settings()`.
    pub fn explicit_page_settings(&self) -> Option<HtmlPageSettings> {
        self.page_settings
    }

    /// Validate → parse → cascade → convert → flow layout → `DocumentModel`.
    ///
    /// Page setup precedence: an explicit `page_settings()` call always
    /// wins; otherwise a document `@page` rule wins; otherwise the letter
    /// default. Title precedence: an explicit `.title()` call always wins;
    /// otherwise the HTML `<title>` element is used, if present.
    pub fn render(&self) -> Result<HtmlRenderResult, HtmlRenderError> {
        self.validate()?;

        let mut tracker = ReadinessTracker::new();

        let converted = convert(self)?;
        tracker.mark_parsed();

        let mut image_store = ImageStore::new();
        for image in &converted.images {
            image_store.insert(&image.id, image.data.clone());
        }
        tracker.mark_images_loaded();

        let config = FlowConfig {
            page_size: converted.page.size,
            margins: converted.page.margins,
            default_style: None,
            ..Default::default()
        };
        let mut engine = FlowLayoutEngine::new(config);
        let mut model = engine.layout(&converted.blocks);
        model.image_store = image_store;
        tracker.mark_laid_out();

        debug_assert!(
            tracker.is_ready(),
            "render() must complete every pipeline stage before returning"
        );

        let title = self
            .title_value()
            .map(str::to_string)
            .or(converted.title);
        if let Some(title) = title {
            model.metadata.title = Some(title);
        }
        model.metadata.page_count = model.pages.len();

        Ok(HtmlRenderResult {
            model,
            warnings: converted.warnings,
            resource_policy: self.resource_policy.clone(),
        })
    }

    /// Convenience: render straight to a PDF file.
    pub fn save_pdf(&self, path: impl AsRef<Path>) -> Result<(), HtmlRenderError> {
        self.render()?.save_pdf(path)
    }
}

/// The result of `HtmlDocument::render()`: a laid-out `DocumentModel` ready
/// for PDF/PNG/print, plus any graceful-degradation warnings collected
/// during conversion (unsupported tags/CSS, blocked resources, etc.).
#[derive(Debug, Clone)]
pub struct HtmlRenderResult {
    pub model: DocumentModel,
    pub warnings: Vec<String>,
    resource_policy: ResourcePolicy,
}

impl HtmlRenderResult {
    /// Render to PDF bytes, enforcing the document's `ResourcePolicy` output
    /// size limit.
    pub fn to_pdf_bytes(&self) -> Result<Vec<u8>, HtmlRenderError> {
        let bytes = PdfRenderer::new()
            .render_to_bytes(&self.model)
            .map_err(|error| {
                HtmlRenderError::at_stage(
                    "HTML_RENDER_PDF_FAILED",
                    HtmlRenderStage::RenderPdf,
                    error.to_string(),
                )
            })?;
        self.resource_policy.validate_pdf_bytes(bytes.len())?;
        Ok(bytes)
    }

    /// Render to PDF and write it to `path`.
    pub fn save_pdf(&self, path: impl AsRef<Path>) -> Result<(), HtmlRenderError> {
        let bytes = self.to_pdf_bytes()?;
        std::fs::write(path.as_ref(), &bytes).map_err(|error| {
            HtmlRenderError::at_stage(
                "HTML_RENDER_PDF_FAILED",
                HtmlRenderStage::RenderPdf,
                error.to_string(),
            )
        })
    }

    /// Render one PNG per page into `dir` at the given DPI.
    pub fn render_png(
        &self,
        dir: impl AsRef<Path>,
        dpi: u32,
    ) -> Result<Vec<PathBuf>, HtmlRenderError> {
        TinySkiaRenderer::new()
            .render_to_raster(&self.model, Dpi(dpi as f64), dir.as_ref())
            .map_err(|error| {
                HtmlRenderError::at_stage(
                    "HTML_RENDER_PNG_FAILED",
                    HtmlRenderStage::RenderPdf,
                    error.to_string(),
                )
            })
    }
}
