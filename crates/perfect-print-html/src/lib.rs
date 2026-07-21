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
    page_settings: HtmlPageSettings,
}

impl HtmlDocument {
    pub fn new(html: impl Into<String>) -> Self {
        Self {
            html: html.into(),
            title: None,
            resource_policy: ResourcePolicy::offline(),
            page_settings: HtmlPageSettings::letter(),
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
        self.page_settings = settings;
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
        self.page_settings.validate()
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

    pub fn page(&self) -> HtmlPageSettings {
        self.page_settings
    }
}
