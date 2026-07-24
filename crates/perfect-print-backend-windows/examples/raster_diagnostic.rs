// Diagnostic: rasterize the exact same DocumentModel the GDI smoke test
// uses, straight to PNG via perfect-print-render, bypassing GDI entirely.
// Used to bisect whether a blank GDI print output is a font/rasterization
// problem or a GDI blit problem. Windows-only, same as the smoke test this
// pairs with — `perfect-print` is only a dev-dependency under
// `cfg(target_os = "windows")`.
#[cfg(target_os = "windows")]
fn main() {
    use perfect_print::{Document, Paragraph};

    let paths = Document::new()
        .title("Perfect Print Windows Smoke Test")
        .add(
            Paragraph::new("Windows GDI print backend smoke test")
                .font_size(24.0)
                .bold(),
        )
        .add(Paragraph::new(
            "If you can read this in the output PDF, StretchDIBits worked.",
        ))
        .render_png("C:/PerfectPrint-Build/raster_diagnostic_pages", 150)
        .expect("render_png failed");
    println!("Rendered {} page(s): {:?}", paths.len(), paths);
}

#[cfg(not(target_os = "windows"))]
fn main() {
    eprintln!("This example only runs on Windows.");
}
