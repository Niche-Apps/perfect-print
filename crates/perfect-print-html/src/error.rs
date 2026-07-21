use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HtmlRenderStage {
    Validate,
    Parse,
    Load,
    Fonts,
    Images,
    RenderPdf,
    ValidatePdf,
    Cleanup,
}

impl fmt::Display for HtmlRenderStage {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{:?}", self)
    }
}

#[derive(Debug, thiserror::Error)]
#[error("{code} during {stage}: {message}")]
pub struct HtmlRenderError {
    code: &'static str,
    stage: HtmlRenderStage,
    message: String,
}

impl HtmlRenderError {
    pub fn at_stage(
        code: &'static str,
        stage: HtmlRenderStage,
        message: impl Into<String>,
    ) -> Self {
        Self {
            code,
            stage,
            message: message.into(),
        }
    }

    pub fn code(&self) -> &'static str {
        self.code
    }

    pub fn stage(&self) -> HtmlRenderStage {
        self.stage
    }
}
