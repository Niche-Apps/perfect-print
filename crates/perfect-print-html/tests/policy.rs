use perfect_print_html::{
    HtmlDocument, HtmlPageSettings, HtmlRenderError, HtmlRenderStage, ReadinessTracker,
    ResourcePolicy,
};
use std::time::{Duration, Instant};

#[test]
fn html_document_rejects_empty_and_oversize_input() {
    let empty = HtmlDocument::new("").validate().unwrap_err();
    assert_eq!(empty.code(), "HTML_INPUT_INVALID");

    let policy = ResourcePolicy::offline().with_max_html_bytes(8);
    let oversize = HtmlDocument::new("123456789")
        .resource_policy(policy)
        .validate()
        .unwrap_err();
    assert_eq!(oversize.code(), "HTML_INPUT_TOO_LARGE");
}

#[test]
fn offline_policy_blocks_network_and_allows_embedded_data() {
    let policy = ResourcePolicy::offline();
    assert!(!policy.scripts_enabled());
    assert!(!policy
        .allows_url("https://example.com/private.png")
        .unwrap());
    assert!(!policy.allows_url("http://example.com/font.woff2").unwrap());
    assert!(policy
        .allows_url("data:image/png;base64,iVBORw0KGgo=")
        .unwrap());
    assert!(policy.allows_url("about:blank").unwrap());
}

#[test]
fn local_resources_require_a_canonical_allowlisted_root() {
    let root = tempfile::tempdir().unwrap();
    let allowed = root.path().join("logo.svg");
    std::fs::write(&allowed, "<svg/>").unwrap();
    let outside = tempfile::NamedTempFile::new().unwrap();

    let policy = ResourcePolicy::offline()
        .with_local_base_directory(root.path())
        .unwrap();
    assert!(policy.allows_local_path(&allowed).unwrap());
    assert!(!policy.allows_local_path(outside.path()).unwrap());
    assert!(!policy
        .allows_local_path(
            &root
                .path()
                .join("..")
                .join(outside.path().file_name().unwrap())
        )
        .unwrap());
}

#[test]
fn page_and_resource_limits_are_validated() {
    let invalid_page = HtmlDocument::new("<p>x</p>")
        .page_settings(HtmlPageSettings::custom(0.0, 72.0))
        .validate()
        .unwrap_err();
    assert_eq!(invalid_page.code(), "HTML_PAGE_SETTINGS_INVALID");

    let policy = ResourcePolicy::offline()
        .with_max_resource_bytes(10)
        .with_max_pdf_bytes(20);
    assert!(policy.validate_resource_bytes(10).is_ok());
    assert_eq!(
        policy.validate_resource_bytes(11).unwrap_err().code(),
        "HTML_RESOURCES_TOO_LARGE"
    );
    assert!(policy.validate_pdf_bytes(20).is_ok());
    assert_eq!(
        policy.validate_pdf_bytes(21).unwrap_err().code(),
        "PDF_OUTPUT_TOO_LARGE"
    );
}

#[test]
fn readiness_requires_dom_fonts_and_images() {
    let start = Instant::now();
    let mut tracker = ReadinessTracker::new(start, Duration::from_secs(5));
    assert_eq!(tracker.stage(), HtmlRenderStage::Load);
    assert!(!tracker.is_ready());

    tracker.mark_dom_ready();
    assert_eq!(tracker.stage(), HtmlRenderStage::Fonts);
    tracker.mark_fonts_ready();
    assert_eq!(tracker.stage(), HtmlRenderStage::Images);
    tracker.mark_images_ready();
    assert!(tracker.is_ready());
    assert_eq!(tracker.stage(), HtmlRenderStage::RenderPdf);
}

#[test]
fn readiness_timeout_has_a_stable_stage_error() {
    let start = Instant::now();
    let tracker = ReadinessTracker::new(start, Duration::from_millis(50));
    let error = tracker
        .check_timeout(start + Duration::from_millis(51))
        .unwrap_err();
    assert_eq!(error.code(), "HTML_LOAD_TIMEOUT");
    assert_eq!(error.stage(), HtmlRenderStage::Load);
}

#[test]
fn error_codes_are_stable_and_do_not_include_document_content() {
    let error = HtmlRenderError::at_stage(
        "PDF_RENDER_FAILED",
        HtmlRenderStage::RenderPdf,
        "platform renderer returned no bytes",
    );
    assert_eq!(error.code(), "PDF_RENDER_FAILED");
    assert_eq!(error.stage(), HtmlRenderStage::RenderPdf);
    assert!(!error.to_string().contains("<html"));
}
