use crate::HtmlRenderStage;

/// Tracks progress of the synchronous, pure-Rust HTML → `DocumentModel`
/// pipeline (parse+cascade → images → layout).
///
/// The original design (inherited from a WebView-based architecture)
/// modeled readiness as an asynchronous wait with a wall-clock timeout —
/// `dom_ready`/`fonts_ready`/`images_ready` flags flipped by callbacks from
/// a browser engine, with `check_timeout` guarding against a page that
/// never finishes loading. The pure-Rust pipeline has no such async
/// loading step: parsing, image decoding, and layout all run synchronously
/// and either complete or return an error immediately, so a timeout has
/// nothing to measure. This tracker is kept — simplified to match the
/// synchronous pipeline — purely to attribute the correct `HtmlRenderStage`
/// to `render()`'s progress and to assert (via `is_ready`) that every
/// stage actually ran before a `DocumentModel` is handed back.
#[derive(Debug, Clone, Default)]
pub struct ReadinessTracker {
    parsed: bool,
    images_loaded: bool,
    laid_out: bool,
}

impl ReadinessTracker {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn mark_parsed(&mut self) {
        self.parsed = true;
    }

    pub fn mark_images_loaded(&mut self) {
        self.images_loaded = true;
    }

    pub fn mark_laid_out(&mut self) {
        self.laid_out = true;
    }

    pub fn is_ready(&self) -> bool {
        self.parsed && self.images_loaded && self.laid_out
    }

    /// The stage the pipeline is currently blocked on (or `RenderPdf` once
    /// every prior stage has completed and PDF/PNG generation is next).
    pub fn stage(&self) -> HtmlRenderStage {
        if !self.parsed {
            HtmlRenderStage::Parse
        } else if !self.images_loaded {
            HtmlRenderStage::Images
        } else {
            HtmlRenderStage::RenderPdf
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stages_progress_in_order() {
        let mut tracker = ReadinessTracker::new();
        assert_eq!(tracker.stage(), HtmlRenderStage::Parse);
        assert!(!tracker.is_ready());

        tracker.mark_parsed();
        assert_eq!(tracker.stage(), HtmlRenderStage::Images);
        assert!(!tracker.is_ready());

        tracker.mark_images_loaded();
        assert_eq!(tracker.stage(), HtmlRenderStage::RenderPdf);
        assert!(!tracker.is_ready());

        tracker.mark_laid_out();
        assert!(tracker.is_ready());
    }
}
