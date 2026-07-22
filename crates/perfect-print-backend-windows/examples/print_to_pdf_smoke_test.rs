// crates/perfect-print-backend-windows/examples/print_to_pdf_smoke_test.rs
//! Windows-only smoke test: prints a real document through GDI to the
//! "Microsoft Print to PDF" virtual printer and writes a real PDF file,
//! for genuine end-to-end verification (not just typecheck) on a
//! machine that actually has that printer installed.
#[cfg(target_os = "windows")]
fn main() {
    use perfect_print::{Document, Paragraph};
    use perfect_print_dialog::PrintSettings;

    let doc = Document::new()
        .title("Perfect Print Windows Smoke Test")
        .add(
            Paragraph::new("Windows GDI print backend smoke test")
                .font_size(24.0)
                .bold(),
        )
        .add(Paragraph::new(
            "If you can read this in the output PDF, StretchDIBits worked.",
        ))
        .build();

    let settings = PrintSettings::default();

    match perfect_print_backend_windows::print_to_named_printer_for_test(
        "Microsoft Print to PDF",
        &doc,
        &settings,
        r"C:\PerfectPrint-Build\smoke_test_output.pdf",
    ) {
        Ok(job) => println!("OK: job = {:?}", job),
        Err(e) => {
            eprintln!("FAILED: {e}");
            std::process::exit(1);
        }
    }
}

#[cfg(not(target_os = "windows"))]
fn main() {
    eprintln!("This example only runs on Windows.");
}
