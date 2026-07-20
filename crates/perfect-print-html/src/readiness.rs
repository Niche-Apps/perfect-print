use crate::{HtmlRenderError, HtmlRenderStage};
use std::time::{Duration, Instant};

#[derive(Debug, Clone)]
pub struct ReadinessTracker {
    started_at: Instant,
    timeout: Duration,
    dom_ready: bool,
    fonts_ready: bool,
    images_ready: bool,
}

impl ReadinessTracker {
    pub fn new(started_at: Instant, timeout: Duration) -> Self {
        Self {
            started_at,
            timeout,
            dom_ready: false,
            fonts_ready: false,
            images_ready: false,
        }
    }

    pub fn mark_dom_ready(&mut self) {
        self.dom_ready = true;
    }

    pub fn mark_fonts_ready(&mut self) {
        self.fonts_ready = true;
    }

    pub fn mark_images_ready(&mut self) {
        self.images_ready = true;
    }

    pub fn is_ready(&self) -> bool {
        self.dom_ready && self.fonts_ready && self.images_ready
    }

    pub fn stage(&self) -> HtmlRenderStage {
        if !self.dom_ready {
            HtmlRenderStage::Load
        } else if !self.fonts_ready {
            HtmlRenderStage::Fonts
        } else if !self.images_ready {
            HtmlRenderStage::Images
        } else {
            HtmlRenderStage::RenderPdf
        }
    }

    pub fn check_timeout(&self, now: Instant) -> Result<(), HtmlRenderError> {
        if now.saturating_duration_since(self.started_at) > self.timeout {
            return Err(HtmlRenderError::at_stage(
                "HTML_LOAD_TIMEOUT",
                self.stage(),
                format!("renderer exceeded {} ms", self.timeout.as_millis()),
            ));
        }
        Ok(())
    }
}
