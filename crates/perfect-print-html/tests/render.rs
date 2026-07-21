use perfect_print_html::HtmlDocument;

#[test]
fn html_renders_to_pdf_bytes() {
    let doc = HtmlDocument::new("<h1>Report</h1><p>Hello <b>world</b></p>");
    let result = doc.render().unwrap();
    assert!(result.model.page_count() >= 1);
    let pdf = result.to_pdf_bytes().unwrap();
    assert!(pdf.starts_with(b"%PDF"));
    assert!(result.warnings.is_empty());
}

#[test]
fn deterministic_output() {
    let html = "<h1>Same</h1><p>Every time</p>";
    let a = HtmlDocument::new(html).render().unwrap().to_pdf_bytes().unwrap();
    let b = HtmlDocument::new(html).render().unwrap().to_pdf_bytes().unwrap();
    assert_eq!(a, b);
}

#[test]
fn multi_page_html_paginates() {
    let body: String = (0..200).map(|i| format!("<p>Paragraph {i}</p>")).collect();
    let result = HtmlDocument::new(body).render().unwrap();
    assert!(result.model.page_count() > 1);
}

#[test]
fn save_pdf_writes_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("out.pdf");
    let doc = HtmlDocument::new("<p>Hello</p>");
    doc.save_pdf(&path).unwrap();
    let bytes = std::fs::read(&path).unwrap();
    assert!(bytes.starts_with(b"%PDF"));
}

#[test]
fn render_png_writes_pages() {
    let dir = tempfile::tempdir().unwrap();
    let doc = HtmlDocument::new("<h1>Title</h1><p>Body text.</p>");
    let result = doc.render().unwrap();
    let paths = result.render_png(dir.path(), 150).unwrap();
    assert!(!paths.is_empty());
    for p in &paths {
        assert!(p.exists());
    }
}

#[test]
fn explicit_page_settings_win_over_at_page() {
    use perfect_print_html::HtmlPageSettings;
    let doc = HtmlDocument::new("<style>@page { size: a4 }</style><p>x</p>")
        .page_settings(HtmlPageSettings::custom(300.0, 400.0));
    let result = doc.render().unwrap();
    let page = &result.model.pages[0];
    assert_eq!(page.size.width, 300.0);
    assert_eq!(page.size.height, 400.0);
}

#[test]
fn caller_title_wins_over_html_title() {
    let doc = HtmlDocument::new("<html><head><title>From HTML</title></head><body><p>x</p></body></html>")
        .title("From Caller");
    let result = doc.render().unwrap();
    assert_eq!(result.model.metadata.title.as_deref(), Some("From Caller"));
}

#[test]
fn html_title_used_when_caller_title_unset() {
    let doc = HtmlDocument::new(
        "<html><head><title>From HTML</title></head><body><p>x</p></body></html>",
    );
    let result = doc.render().unwrap();
    assert_eq!(result.model.metadata.title.as_deref(), Some("From HTML"));
}
