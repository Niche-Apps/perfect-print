use anyhow::Result;
use clap::{Parser, Subcommand};
use perfect_print_core::document::DocumentBuilder;
use perfect_print_core::document::PageBuilder;
use perfect_print_core::page::PageSize;
use perfect_print_core::units::Dpi;
use perfect_print_core::Strictness;
use perfect_print_dialog::PrintDialog;
use perfect_print_render::Render;
use std::path::PathBuf;

mod diff;
mod geometry;
#[cfg(test)]
mod golden;

#[derive(Parser)]
#[command(name = "perfect-print-cli", version, about = "perfect-print CLI")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Build an example document and output model JSON
    Model {
        /// Example name (hello, invoice, report, receipt, labels, worksheet)
        example: String,
        /// Output file path
        #[arg(long, default_value = "-")]
        output: String,
    },
    /// Render an example document to PDF and/or PNG
    Render {
        /// Example name
        example: String,
        /// PDF output path
        #[arg(long)]
        pdf: Option<PathBuf>,
        /// PNG output directory
        #[arg(long)]
        png_dir: Option<PathBuf>,
        /// DPI for raster output
        #[arg(long, default_value = "300")]
        dpi: f64,
    },
    /// Render an HTML/CSS document to PDF and/or PNG (pure-Rust pipeline)
    RenderHtml {
        /// Path to an .html input file
        input: PathBuf,
        /// PDF output path
        #[arg(long)]
        pdf: Option<PathBuf>,
        /// PNG output directory
        #[arg(long)]
        png_dir: Option<PathBuf>,
        /// DPI for raster output
        #[arg(long, default_value = "300")]
        dpi: u32,
        /// Local base directory to allow relative/local image references from
        #[arg(long)]
        base_dir: Option<PathBuf>,
        /// Treat any conversion warning as a failure (exit code 1)
        #[arg(long)]
        strict: bool,
    },
    /// List available printers
    Printers {
        #[command(subcommand)]
        action: Option<PrinterCommands>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Show printer capabilities
    Capabilities {
        /// Printer name
        #[arg(long)]
        printer: String,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// List pending print jobs
    Jobs,
    /// Check print job status
    JobStatus {
        /// Job ID
        job_id: String,
    },
    /// Cancel a print job
    CancelJob {
        /// Job ID
        job_id: String,
    },
    /// Print a document
    Print {
        /// Example name
        example: String,
        /// Printer name
        #[arg(long)]
        printer: String,
        /// Settings JSON file
        #[arg(long)]
        settings: Option<PathBuf>,
        /// Strictness mode: best-effort, warn, exact
        #[arg(long, default_value = "warn")]
        strictness: String,
    },
    /// Verify PDF/raster parity
    Verify {
        /// Example name
        example: String,
        /// Golden reference directory
        #[arg(long)]
        against: PathBuf,
        /// DPI for comparison
        #[arg(long, default_value = "300")]
        dpi: f64,
        /// Tolerance (0.0-1.0)
        #[arg(long, default_value = "0.01")]
        tolerance: f64,
    },
    /// Generate diagnostics bundle
    Diagnostics {
        /// Example name
        example: String,
        /// Output zip path
        #[arg(long)]
        out: PathBuf,
    },
}

#[derive(Subcommand)]
enum PrinterCommands {
    /// List printers
    List,
}

fn main() -> Result<()> {
    env_logger::init();
    let cli = Cli::parse();

    match cli.command {
        Commands::Model { example, output } => {
            let model = build_example(&example)?;
            let json = model.to_json()?;

            if output == "-" {
                println!("{}", json);
            } else {
                std::fs::write(&output, &json)?;
                eprintln!("Model written to {}", output);
            }
        }
        Commands::Render {
            example,
            pdf,
            png_dir,
            dpi,
        } => {
            let model = build_example(&example)?;

            if let Some(ref pdf_path) = pdf {
                let renderer = perfect_print_pdf::PdfRenderer::new();
                renderer.render_to_pdf(&model, pdf_path)?;
                eprintln!("PDF written to {}", pdf_path.display());
            }

            if let Some(ref png_path) = png_dir {
                let renderer = perfect_print_render::TinySkiaRenderer::new();
                let paths = renderer.render_to_raster(&model, Dpi(dpi), png_path)?;
                eprintln!("PNG pages written to {}:", png_path.display());
                for p in &paths {
                    eprintln!("  {}", p.display());
                }
            }

            if pdf.is_none() && png_dir.is_none() {
                eprintln!("Specify --pdf and/or --png-dir");
            }
        }
        Commands::RenderHtml {
            input,
            pdf,
            png_dir,
            dpi,
            base_dir,
            strict,
        } => {
            let html = std::fs::read_to_string(&input)
                .map_err(|e| anyhow::anyhow!("failed to read {}: {}", input.display(), e))?;

            let mut policy = perfect_print_html::ResourcePolicy::offline();
            if let Some(dir) = &base_dir {
                policy = policy.with_local_base_directory(dir).map_err(|e| {
                    anyhow::anyhow!("invalid --base-dir {}: {}", dir.display(), e)
                })?;
            }

            let doc = perfect_print_html::HtmlDocument::new(html).resource_policy(policy);
            let result = doc
                .render()
                .map_err(|e| anyhow::anyhow!("failed to render {}: {}", input.display(), e))?;

            if let Some(ref pdf_path) = pdf {
                let bytes = result
                    .to_pdf_bytes()
                    .map_err(|e| anyhow::anyhow!("failed to render PDF: {}", e))?;
                std::fs::write(pdf_path, &bytes)?;
                eprintln!("PDF written to {}", pdf_path.display());
            }

            if let Some(ref png_path) = png_dir {
                let paths = result
                    .render_png(png_path, dpi)
                    .map_err(|e| anyhow::anyhow!("failed to render PNG: {}", e))?;
                eprintln!("PNG pages written to {}:", png_path.display());
                for p in &paths {
                    eprintln!("  {}", p.display());
                }
            }

            if pdf.is_none() && png_dir.is_none() {
                eprintln!("Specify --pdf and/or --png-dir");
            }

            if !result.warnings.is_empty() {
                for w in &result.warnings {
                    eprintln!("Warning: {}", w);
                }
                if strict {
                    eprintln!(
                        "Error: {} warning(s) treated as failures in --strict mode",
                        result.warnings.len()
                    );
                    std::process::exit(1);
                }
            }
        }
        Commands::Printers { action: _, json } => {
            #[cfg(target_os = "macos")]
            {
                let dialog = perfect_print_backend_macos::MacosPrintDialog::new();
                match dialog.available_printers() {
                    Ok(printers) => {
                        if json {
                            let json_output: Vec<serde_json::Value> = printers
                                .iter()
                                .map(|p| {
                                    serde_json::json!({
                                        "name": p.capabilities.name,
                                        "is_default": p.capabilities.is_default,
                                        "supports_color": p.capabilities.supports_color,
                                        "supports_duplex": p.capabilities.supports_duplex,
                                        "paper_sizes": p.capabilities.paper_sizes.iter()
                                            .map(|s| format!("{:?}", s))
                                            .collect::<Vec<_>>(),
                                        "state": format!("{:?}", p.capabilities.state),
                                    })
                                })
                                .collect();
                            println!("{}", serde_json::to_string_pretty(&json_output).unwrap());
                        } else {
                            if printers.is_empty() {
                                eprintln!("No printers found.");
                            } else {
                                for p in &printers {
                                    let default_flag = if p.capabilities.is_default {
                                        " (default)"
                                    } else {
                                        ""
                                    };
                                    println!(
                                        "{}{} | color: {} | duplex: {}",
                                        p.capabilities.name,
                                        default_flag,
                                        p.capabilities.supports_color,
                                        p.capabilities.supports_duplex
                                    );
                                }
                            }
                        }
                    }
                    Err(e) => {
                        if json {
                            println!(r#"{{"error": "{}"}}"#, e);
                        } else {
                            eprintln!("Error listing printers: {}", e);
                        }
                    }
                }
            }
            #[cfg(not(target_os = "macos"))]
            {
                if json {
                    println!(
                        r#"{{"error": "Printer listing not yet implemented on this platform"}}"#
                    );
                } else {
                    eprintln!("Printer listing not yet implemented on this platform");
                }
            }
        }
        Commands::Capabilities { printer, json } => {
            #[cfg(target_os = "macos")]
            {
                let dialog = perfect_print_backend_macos::MacosPrintDialog::new();
                let printers = dialog.available_printers()?;
                let found = printers.iter().find(|p| p.capabilities.name == printer);
                match found {
                    Some(p) => {
                        if json {
                            let json_output = serde_json::json!({
                                "name": p.capabilities.name,
                                "is_default": p.capabilities.is_default,
                                "supports_color": p.capabilities.supports_color,
                                "supports_duplex": p.capabilities.supports_duplex,
                                "paper_sizes": p.capabilities.paper_sizes.iter()
                                    .map(|s| format!("{:?}", s))
                                    .collect::<Vec<_>>(),
                                "max_resolution": p.capabilities.max_resolution,
                                "state": format!("{:?}", p.capabilities.state),
                            });
                            println!("{}", serde_json::to_string_pretty(&json_output).unwrap());
                        } else {
                            println!("Printer: {}", p.capabilities.name);
                            println!("Default: {}", p.capabilities.is_default);
                            println!("Color: {}", p.capabilities.supports_color);
                            println!("Duplex: {}", p.capabilities.supports_duplex);
                            println!(
                                "Paper sizes: {}",
                                p.capabilities
                                    .paper_sizes
                                    .iter()
                                    .map(|s| format!("{:?}", s))
                                    .collect::<Vec<_>>()
                                    .join(", ")
                            );
                            println!("State: {:?}", p.capabilities.state);
                        }
                    }
                    None => {
                        if json {
                            println!(r#"{{"error": "Printer '{}' not found"}}"#, printer);
                        } else {
                            eprintln!("Printer '{}' not found", printer);
                            eprintln!("Available printers:");
                            for p in &printers {
                                eprintln!("  {}", p.capabilities.name);
                            }
                        }
                        std::process::exit(1);
                    }
                }
            }
            #[cfg(not(target_os = "macos"))]
            {
                if json {
                    println!(
                        r#"{{"error": "Printer capabilities not yet implemented on this platform"}}"#
                    );
                } else {
                    eprintln!("Printer capabilities not yet implemented on this platform");
                }
            }
        }
        Commands::Print {
            example,
            printer,
            settings: _,
            strictness,
        } => {
            #[cfg(target_os = "macos")]
            {
                // Parse strictness mode
                let strictness_mode = match strictness.as_str() {
                    "best-effort" => Strictness::BestEffort,
                    "warn" => Strictness::Warn,
                    "exact" => Strictness::Exact,
                    _ => {
                        eprintln!(
                            "Invalid strictness mode: {}. Use best-effort, warn, or exact.",
                            strictness
                        );
                        std::process::exit(1);
                    }
                };

                // 1. Build the document
                let model = build_example(&example)?;

                // 2. Validate document
                let validation = model.validate();
                match validation {
                    Ok(()) => {}
                    Err(e) => {
                        let err_msg = format!("{}", e);
                        match strictness_mode {
                            Strictness::BestEffort => {
                                eprintln!("Warning: Document validation failed: {}", err_msg);
                            }
                            Strictness::Warn => {
                                eprintln!("Warning: Document validation failed: {}", err_msg);
                            }
                            Strictness::Exact => {
                                eprintln!("Error: Document validation failed: {}", err_msg);
                                std::process::exit(1);
                            }
                        }
                    }
                }

                // 3. Render to a temporary PDF
                let tmp_pdf = std::env::temp_dir().join(format!("pp_print_{}.pdf", example));
                let pdf_renderer = perfect_print_pdf::PdfRenderer::new();
                pdf_renderer.render_to_pdf(&model, &tmp_pdf)?;
                println!("Rendered PDF: {}", tmp_pdf.display());

                // 4. Get printer capabilities and validate settings
                let dialog = perfect_print_backend_macos::MacosPrintDialog::new();
                let printers = dialog.available_printers()?;
                let printer_info = printers.iter().find(|p| p.capabilities.name == printer);

                match printer_info {
                    Some(p) => {
                        let print_settings = perfect_print_dialog::PrintSettings::default()
                            .paper_size(perfect_print_core::page::PageSize::Letter);

                        // Validate settings against capabilities
                        match print_settings.validate(&p.capabilities) {
                            Ok(warnings) => {
                                if !warnings.is_empty() {
                                    for w in &warnings {
                                        eprintln!("Warning: {}", w);
                                    }
                                    if strictness_mode == Strictness::Exact {
                                        eprintln!("Error: Unsupported settings in exact mode");
                                        std::process::exit(1);
                                    }
                                }
                            }
                            Err(e) => {
                                eprintln!("Validation error: {}", e);
                                if strictness_mode != Strictness::BestEffort {
                                    std::process::exit(1);
                                }
                            }
                        }

                        // 5. Submit to printer
                        let job_id = dialog.submit_print_job(&tmp_pdf, &print_settings)?;
                        match job_id {
                            Some(id) => {
                                println!("Print job submitted to '{}' (job id: {})", printer, id)
                            }
                            None => {
                                println!("Print job submitted to '{}' (no job id parsed)", printer)
                            }
                        }
                    }
                    None => {
                        eprintln!("Printer '{}' not found", printer);
                        eprintln!("Available printers:");
                        for p in &printers {
                            eprintln!("  {}", p.capabilities.name);
                        }
                        std::process::exit(1);
                    }
                }
            }
            #[cfg(not(target_os = "macos"))]
            {
                eprintln!("Print not yet implemented on this platform");
                eprintln!(
                    "Would print example '{}' to printer '{}' with strictness '{}'",
                    example, printer, strictness
                );
            }
        }
        Commands::Jobs => {
            #[cfg(target_os = "macos")]
            {
                let dialog = perfect_print_backend_macos::MacosPrintDialog::new();
                match dialog.list_jobs() {
                    Ok(jobs) => {
                        if jobs.is_empty() {
                            println!("No pending print jobs.");
                        } else {
                            println!("Pending print jobs:");
                            for (job_id, printer, status) in &jobs {
                                println!("  {}  printer={}  status={}", job_id, printer, status);
                            }
                        }
                    }
                    Err(e) => eprintln!("Error listing jobs: {}", e),
                }
            }
            #[cfg(not(target_os = "macos"))]
            {
                eprintln!("Job listing not yet implemented on this platform");
            }
        }
        Commands::JobStatus { job_id } => {
            #[cfg(target_os = "macos")]
            {
                let dialog = perfect_print_backend_macos::MacosPrintDialog::new();
                match dialog.poll_job_status(&job_id) {
                    Ok(completed) => {
                        if completed {
                            println!("Job '{}' has completed.", job_id);
                        } else {
                            println!("Job '{}' is still in the print queue.", job_id);
                        }
                    }
                    Err(e) => eprintln!("Error checking job status: {}", e),
                }
            }
            #[cfg(not(target_os = "macos"))]
            {
                eprintln!("Job status not yet implemented on this platform");
            }
        }
        Commands::CancelJob { job_id } => {
            #[cfg(target_os = "macos")]
            {
                let dialog = perfect_print_backend_macos::MacosPrintDialog::new();
                match dialog.cancel_job(&job_id) {
                    Ok(()) => println!("Job '{}' cancelled.", job_id),
                    Err(e) => eprintln!("Error cancelling job: {}", e),
                }
            }
            #[cfg(not(target_os = "macos"))]
            {
                eprintln!("Job cancellation not yet implemented on this platform");
            }
        }
        Commands::Verify {
            example,
            against,
            dpi,
            tolerance,
        } => {
            let model = build_example(&example)?;
            let _dpi_val = dpi as u32;

            // Create a temporary directory for raster output
            let tmp_dir = std::env::temp_dir().join("pp_verify");
            let _ = std::fs::create_dir_all(&tmp_dir);

            // Render the document to PNG
            let renderer = perfect_print_render::TinySkiaRenderer::new();
            let _png_paths =
                renderer.render_to_raster(&model, perfect_print_core::units::Dpi(dpi), &tmp_dir)?;

            // Run geometry assertions
            let assertions = vec![
                geometry::GeometryAssertion::PageCount {
                    expected: model.page_count(),
                },
                geometry::GeometryAssertion::HasText { page_index: 0 },
            ];

            let geo_results = geometry::check_all(&model, &assertions);
            let geo_passed = geo_results.iter().all(|r| r.passed);

            // Compare with reference if provided
            let diff_passed = if against.exists() {
                let ref_entries: Vec<_> = std::fs::read_dir(&against)
                    .map(|rd| {
                        rd.filter_map(|e| e.ok())
                            .filter(|e| e.path().extension().map_or(false, |ext| ext == "png"))
                            .collect()
                    })
                    .unwrap_or_default();

                if ref_entries.is_empty() {
                    eprintln!(
                        "  Warning: no PNG reference files found in {}",
                        against.display()
                    );
                    true
                } else {
                    let mut all_pass = true;
                    for entry in &ref_entries {
                        let name = entry.file_name();
                        let generated = tmp_dir.join(&name);
                        if generated.exists() {
                            match diff::compare_pngs(&generated, &entry.path(), tolerance, 5) {
                                Ok(result) => {
                                    eprintln!("  {}: {}", name.to_string_lossy(), result.summary());
                                    if !result.matches {
                                        all_pass = false;
                                    }
                                }
                                Err(e) => {
                                    eprintln!("  {}: ERROR - {}", name.to_string_lossy(), e);
                                    all_pass = false;
                                }
                            }
                        } else {
                            eprintln!("  {}: missing generated file", name.to_string_lossy());
                            all_pass = false;
                        }
                    }
                    all_pass
                }
            } else {
                true
            };

            // Print summary
            eprintln!("Geometry: {}", if geo_passed { "PASS" } else { "FAIL" });
            for r in &geo_results {
                eprintln!("  {}", r.summary());
            }

            if against.exists() {
                eprintln!("Visual diff: {}", if diff_passed { "PASS" } else { "FAIL" });
            }

            // Generate heatmap on failure
            if !diff_passed && against.exists() {
                let heatmap_dir = tmp_dir.join("diffs");
                let _ = std::fs::create_dir_all(&heatmap_dir);
                for entry in std::fs::read_dir(&against).unwrap().filter_map(|e| e.ok()) {
                    let name = entry.file_name();
                    let generated = tmp_dir.join(&name);
                    if generated.exists() {
                        let heatmap_path =
                            heatmap_dir.join(format!("diff_{}", name.to_string_lossy()));
                        if let Ok(result) =
                            diff::generate_heatmap(&generated, &entry.path(), &heatmap_path, 5)
                        {
                            eprintln!(
                                "  Heatmap: {} ({})",
                                heatmap_path.display(),
                                result.summary()
                            );
                        }
                    }
                }
                eprintln!("Diff heatmaps written to: {}", heatmap_dir.display());
            }

            // Cleanup temp dir on success
            if geo_passed && diff_passed {
                let _ = std::fs::remove_dir_all(&tmp_dir);
            }

            if !geo_passed || !diff_passed {
                std::process::exit(1);
            }
        }
        Commands::Diagnostics { example, out } => {
            let model = build_example(&example)?;
            let dpi_val = 300u32;

            // Create temp dir for outputs
            let tmp_dir = std::env::temp_dir().join("pp_diagnostics");
            let _ = std::fs::create_dir_all(&tmp_dir);

            // 1. Render to PDF
            let pdf_path = tmp_dir.join("output.pdf");
            let pdf_renderer = perfect_print_pdf::PdfRenderer::new();
            pdf_renderer.render_to_pdf(&model, &pdf_path)?;

            // 2. Render to PNG
            let png_dir = tmp_dir.join("pages");
            let _ = std::fs::create_dir_all(&png_dir);
            let raster_renderer = perfect_print_render::TinySkiaRenderer::new();
            let png_paths = raster_renderer.render_to_raster(
                &model,
                perfect_print_core::units::Dpi(dpi_val as f64),
                &png_dir,
            )?;

            // 3. Run geometry assertions
            let assertions = vec![
                geometry::GeometryAssertion::PageCount {
                    expected: model.page_count(),
                },
                geometry::GeometryAssertion::PageSize {
                    page_index: 0,
                    expected_width: model.pages.get(0).map(|p| p.size.width).unwrap_or(612.0),
                    expected_height: model.pages.get(0).map(|p| p.size.height).unwrap_or(792.0),
                    tolerance_pts: 0.5,
                },
                geometry::GeometryAssertion::HasText { page_index: 0 },
            ];
            let geo_results = geometry::check_all(&model, &assertions);

            // 4. Write model JSON
            let json_path = tmp_dir.join("model.json");
            let json = model.to_json().unwrap_or_default();
            std::fs::write(&json_path, &json)?;

            // 5. Write system info
            let sysinfo_path = tmp_dir.join("system-info.json");
            let sysinfo = serde_json::json!({
                "os": std::env::consts::OS,
                "arch": std::env::consts::ARCH,
                "family": std::env::consts::FAMILY,
                "rust_version": "unknown",
                "timestamp": chrono::Local::now().to_rfc3339(),
            });
            std::fs::write(
                &sysinfo_path,
                serde_json::to_string_pretty(&sysinfo).unwrap(),
            )?;

            // 6. Write font list
            let fontlist_path = tmp_dir.join("font-list.json");
            let font_db = perfect_print_layout::font_loader::SystemFontLoader::new();
            let font_list: Vec<_> = font_db.families();
            std::fs::write(
                &fontlist_path,
                serde_json::to_string_pretty(&font_list).unwrap(),
            )?;

            // 7. Write printer capabilities report
            let printers_path = tmp_dir.join("printers.json");
            #[cfg(target_os = "macos")]
            {
                let dialog = perfect_print_backend_macos::MacosPrintDialog::new();
                if let Ok(printers) = dialog.available_printers() {
                    let printer_json: Vec<_> = printers
                        .iter()
                        .map(|p| {
                            serde_json::json!({
                                "name": p.capabilities.name,
                                "is_default": p.capabilities.is_default,
                                "supports_color": p.capabilities.supports_color,
                                "supports_duplex": p.capabilities.supports_duplex,
                                "paper_sizes": p.capabilities.paper_sizes.iter()
                                    .map(|s| format!("{:?}", s))
                                    .collect::<Vec<_>>(),
                                "state": format!("{:?}", p.capabilities.state),
                            })
                        })
                        .collect();
                    std::fs::write(
                        &printers_path,
                        serde_json::to_string_pretty(&printer_json).unwrap(),
                    )?;
                }
            }
            #[cfg(not(target_os = "macos"))]
            {
                std::fs::write(&printers_path, "[]")?;
            }

            // Print report
            eprintln!("=== Diagnostics Report ===");
            eprintln!("Pages: {}", model.page_count());
            eprintln!("PDF: {}", pdf_path.display());
            eprintln!("PNGs: {} pages", png_paths.len());
            eprintln!("Model JSON: {}", json_path.display());
            eprintln!("System info: {}", sysinfo_path.display());
            eprintln!("Font list: {} fonts", font_list.len());
            eprintln!(
                "Geometry: {}",
                if geo_results.iter().all(|r| r.passed) {
                    "PASS"
                } else {
                    "FAIL"
                }
            );
            for r in &geo_results {
                eprintln!("  {}", r.summary());
            }

            // Create zip bundle
            let _ = std::fs::remove_file(&out);
            let zip_file = std::fs::File::create(&out)?;
            let mut zip = zip::ZipWriter::new(zip_file);
            let options = zip::write::FileOptions::default()
                .compression_method(zip::CompressionMethod::Deflated);

            // Add all files from temp dir
            for entry in std::fs::read_dir(&tmp_dir).unwrap().filter_map(|e| e.ok()) {
                let path = entry.path();
                if path.is_file() {
                    let name = path.file_name().unwrap().to_string_lossy().to_string();
                    zip.start_file(&name, options)?;
                    let data = std::fs::read(&path)?;
                    std::io::Write::write_all(&mut zip, &data)?;
                }
            }

            // Add PNG pages from the pages subdirectory
            for entry in std::fs::read_dir(&png_dir).unwrap().filter_map(|e| e.ok()) {
                let path = entry.path();
                if path.is_file() {
                    let name = format!("pages/{}", path.file_name().unwrap().to_string_lossy());
                    zip.start_file(&name, options)?;
                    let data = std::fs::read(&path)?;
                    std::io::Write::write_all(&mut zip, &data)?;
                }
            }

            zip.finish()?;
            eprintln!("Diagnostics bundle written to: {}", out.display());

            // Cleanup
            let _ = std::fs::remove_dir_all(&tmp_dir);
        }
    }

    Ok(())
}

fn build_example(name: &str) -> Result<perfect_print_core::document::DocumentModel> {
    match name {
        "hello" => Ok(build_hello()),
        "invoice" => Ok(build_invoice()),
        "report" => Ok(build_report()),
        "worksheet" => Ok(build_worksheet()),
        "labels" => Ok(build_labels()),
        "receipt" => Ok(build_receipt()),
        _ => anyhow::bail!("Unknown example: {}", name),
    }
}

fn build_hello() -> perfect_print_core::document::DocumentModel {
    use perfect_print_core::color::Color;
    use perfect_print_core::draw::{DrawCommand, ShapedGlyph, TextRun, TextStyle};
    use perfect_print_core::font::FontRef;
    use perfect_print_core::units::Point;

    let mut page = perfect_print_core::page::Page::new(PageSize::Letter);

    let text_run = TextRun {
        text: "Hello, World!".to_string(),
        glyphs: vec![ShapedGlyph {
            glyph_id: 0,
            x_offset: 0.0,
            y_offset: 0.0,
            x_advance: 10.0,
            y_advance: 0.0,
            font_index: 0,
            cluster: 0,
        }],
        style: TextStyle::new(FontRef::new("Helvetica"), 24.0),
    };

    page.add(DrawCommand::Text {
        run: text_run,
        position: Point::new(72.0, 72.0),
        max_width: None,
    });

    page.add(DrawCommand::FillRect {
        rect: perfect_print_core::units::Rect::new(72.0, 100.0, 200.0, 50.0),
        color: Color::blue(),
    });

    DocumentBuilder::new()
        .title("Hello Example")
        .add_page(page)
        .build()
        .unwrap()
}

fn build_invoice() -> perfect_print_core::document::DocumentModel {
    use perfect_print_core::color::Color;
    use perfect_print_core::draw::{DrawCommand, TextRun, TextStyle};
    use perfect_print_core::font::FontRef;
    use perfect_print_core::units::Point;

    let mut page = perfect_print_core::page::Page::new(PageSize::Letter);

    // Title
    page.add(DrawCommand::Text {
        run: TextRun {
            text: "INVOICE".to_string(),
            glyphs: vec![],
            style: TextStyle::new(FontRef::new("Helvetica"), 18.0),
        },
        position: Point::new(72.0, 72.0),
        max_width: None,
    });

    // Header line
    page.add(DrawCommand::FillRect {
        rect: perfect_print_core::units::Rect::new(72.0, 100.0, 468.0, 2.0),
        color: Color::black(),
    });

    // Table header
    page.add(DrawCommand::Text {
        run: TextRun {
            text: "Item                    Qty    Price    Total".to_string(),
            glyphs: vec![],
            style: TextStyle::new(FontRef::new("Helvetica"), 10.0),
        },
        position: Point::new(72.0, 120.0),
        max_width: None,
    });

    // Line items
    let items = vec![
        ("Beef Processing", 2, 80.00),
        ("Hog Slaughter", 1, 50.00),
        ("Smoking (per lb)", 10, 2.00),
    ];

    let mut y = 140.0;
    let mut total = 0.0;
    for (item, qty, price) in &items {
        let line_total = (*qty as f64) * price;
        total += line_total;
        page.add(DrawCommand::Text {
            run: TextRun {
                text: format!("{:<20} {:>5} {:>8.2} {:>8.2}", item, qty, price, line_total),
                glyphs: vec![],
                style: TextStyle::new(FontRef::new("Helvetica"), 10.0),
            },
            position: Point::new(72.0, y),
            max_width: None,
        });
        y += 14.0;
    }

    // Total
    page.add(DrawCommand::FillRect {
        rect: perfect_print_core::units::Rect::new(72.0, y + 5.0, 468.0, 1.0),
        color: Color::black(),
    });

    page.add(DrawCommand::Text {
        run: TextRun {
            text: format!("TOTAL: ${:.2}", total),
            glyphs: vec![],
            style: TextStyle::new(FontRef::new("Helvetica"), 12.0),
        },
        position: Point::new(400.0, y + 20.0),
        max_width: None,
    });

    DocumentBuilder::new()
        .title("Invoice Example")
        .add_page(page)
        .build()
        .unwrap()
}

fn build_report() -> perfect_print_core::document::DocumentModel {
    use perfect_print_core::color::Color;
    use perfect_print_core::draw::{DrawCommand, TextRun, TextStyle};
    use perfect_print_core::font::FontRef;
    use perfect_print_core::units::Point;

    let mut page = perfect_print_core::page::Page::new(PageSize::Letter);

    // Report title
    page.add(DrawCommand::Text {
        run: TextRun {
            text: "Quarterly Report".to_string(),
            glyphs: vec![],
            style: TextStyle::new(FontRef::new("Helvetica"), 18.0),
        },
        position: Point::new(72.0, 72.0),
        max_width: None,
    });

    // Subtitle
    page.add(DrawCommand::Text {
        run: TextRun {
            text: "Q2 2026".to_string(),
            glyphs: vec![],
            style: TextStyle::new(FontRef::new("Helvetica"), 12.0),
        },
        position: Point::new(72.0, 96.0),
        max_width: None,
    });

    // Divider
    page.add(DrawCommand::FillRect {
        rect: perfect_print_core::units::Rect::new(72.0, 110.0, 468.0, 1.0),
        color: Color::gray(0.5),
    });

    // Summary section
    page.add(DrawCommand::Text {
        run: TextRun {
            text: "Summary".to_string(),
            glyphs: vec![],
            style: TextStyle::new(FontRef::new("Helvetica"), 14.0),
        },
        position: Point::new(72.0, 130.0),
        max_width: None,
    });

    let summary = "This quarter showed strong growth across all product lines. \
                   Revenue increased 15% year-over-year, driven primarily by \
                   increased demand for our premium processing services.";

    page.add(DrawCommand::Text {
        run: TextRun {
            text: summary.to_string(),
            glyphs: vec![],
            style: TextStyle::new(FontRef::new("Helvetica"), 10.0),
        },
        position: Point::new(72.0, 150.0),
        max_width: Some(468.0),
    });

    DocumentBuilder::new()
        .title("Quarterly Report")
        .add_page(page)
        .build()
        .unwrap()
}

fn build_worksheet() -> perfect_print_core::document::DocumentModel {
    use perfect_print_core::color::Color;
    use perfect_print_core::draw::{DrawCommand, LineCap, LineJoin, PathOp, TextRun, TextStyle};
    use perfect_print_core::font::FontRef;
    use perfect_print_core::units::Point;

    let mut page = perfect_print_core::page::Page::new(PageSize::Letter);

    // Title
    page.add(DrawCommand::Text {
        run: TextRun {
            text: "Math Worksheet".to_string(),
            glyphs: vec![],
            style: TextStyle::new(FontRef::new("Helvetica"), 16.0),
        },
        position: Point::new(72.0, 72.0),
        max_width: None,
    });

    // Grid of math problems
    let mut y = 100.0;
    for row in 0..10 {
        let mut x = 72.0;
        for col in 0..4 {
            let a = (row * 4 + col + 1) * 3;
            let b = (row * 4 + col + 2) * 2;

            page.add(DrawCommand::Text {
                run: TextRun {
                    text: format!("{} + {} = ___", a, b),
                    glyphs: vec![],
                    style: TextStyle::new(FontRef::new("Courier"), 10.0),
                },
                position: Point::new(x, y),
                max_width: None,
            });

            x += 130.0;
        }
        y += 60.0;

        // Horizontal separator after each row
        if row < 9 {
            page.add(DrawCommand::StrokePath {
                ops: vec![
                    PathOp::MoveTo(Point::new(72.0, y - 15.0)),
                    PathOp::LineTo(Point::new(540.0, y - 15.0)),
                ],
                width: 0.5,
                line_cap: LineCap::Butt,
                line_join: LineJoin::Miter,
                miter_limit: 4.0,
                color: Color::gray(0.8),
            });
        }
    }

    DocumentBuilder::new()
        .title("Math Worksheet")
        .add_page(page)
        .build()
        .unwrap()
}

fn build_labels() -> perfect_print_core::document::DocumentModel {
    use perfect_print_core::color::Color;
    use perfect_print_core::draw::{DrawCommand, LineCap, LineJoin, TextRun, TextStyle};
    use perfect_print_core::font::FontRef;
    use perfect_print_core::units::Point;

    let mut page = perfect_print_core::page::Page::new(PageSize::Letter);

    // Standard 30-up address labels (3 columns x 10 rows, 2.625" x 1" each)
    let label_width = 189.0; // 2.625 inches
    let label_height = 72.0; // 1 inch
    let cols = 3;
    let rows = 10;
    let gap_x = 18.0;
    let gap_y = 0.0;
    let margin_left = 36.0;
    let margin_top = 36.0;

    for row in 0..rows {
        for col in 0..cols {
            let x = margin_left + col as f64 * (label_width + gap_x);
            let y = margin_top + row as f64 * (label_height + gap_y);

            // Label border (cut line)
            page.add(DrawCommand::StrokeRect {
                rect: perfect_print_core::units::Rect::new(x, y, label_width, label_height),
                color: Color::gray(0.7),
                width: 0.25,
                line_cap: LineCap::Butt,
                line_join: LineJoin::Miter,
            });

            // Sample address content
            let label_num = row * cols + col + 1;
            page.add(DrawCommand::Text {
                run: TextRun {
                    text: format!(
                        "Recipient #{:03}\n123 Main Street\nAnytown, ST 12345",
                        label_num
                    ),
                    glyphs: vec![],
                    style: TextStyle::new(FontRef::new("Helvetica"), 8.0),
                },
                position: Point::new(x + 6.0, y + 14.0),
                max_width: Some(label_width - 12.0),
            });
        }
    }

    DocumentBuilder::new()
        .title("Address Labels")
        .add_page(page)
        .build()
        .unwrap()
}

fn build_receipt() -> perfect_print_core::document::DocumentModel {
    use perfect_print_core::color::Color;
    use perfect_print_core::draw::{DrawCommand, TextRun, TextStyle};
    use perfect_print_core::font::FontRef;
    use perfect_print_core::units::Point;

    // Standard 3-inch thermal receipt paper
    let mut page = perfect_print_core::page::Page::new(PageSize::Custom {
        width: 216.0,  // 3 inches
        height: 720.0, // 10 inches
    });

    let x = 10.0;
    let mut y = 16.0;

    // Store header
    page.add(DrawCommand::Text {
        run: TextRun {
            text: "RECEIPT".to_string(),
            glyphs: vec![],
            style: TextStyle::new(FontRef::new("Helvetica"), 12.0),
        },
        position: Point::new(x, y),
        max_width: None,
    });
    y += 16.0;

    page.add(DrawCommand::Text {
        run: TextRun {
            text: "123 Commerce St\nAnytown, ST 12345\n(555) 123-4567".to_string(),
            glyphs: vec![],
            style: TextStyle::new(FontRef::new("Helvetica"), 7.0),
        },
        position: Point::new(x, y),
        max_width: None,
    });
    y += 32.0;

    // Date/time
    page.add(DrawCommand::Text {
        run: TextRun {
            text: "2026-06-09  14:32".to_string(),
            glyphs: vec![],
            style: TextStyle::new(FontRef::new("Helvetica"), 7.0),
        },
        position: Point::new(x, y),
        max_width: None,
    });
    y += 14.0;

    // Divider
    page.add(DrawCommand::FillRect {
        rect: perfect_print_core::units::Rect::new(x, y, 196.0, 0.5),
        color: Color::black(),
    });
    y += 6.0;

    // Line items
    let items = vec![
        ("Widget A", 2, 9.99),
        ("Gadget B", 1, 24.99),
        ("Thingamajig", 5, 3.50),
        ("Doohickey", 1, 49.99),
    ];

    let mut subtotal = 0.0;
    for (item, qty, price) in &items {
        let line_total = (*qty as f64) * price;
        subtotal += line_total;
        page.add(DrawCommand::Text {
            run: TextRun {
                text: format!("{} x{}  ${:.2}", item, qty, line_total),
                glyphs: vec![],
                style: TextStyle::new(FontRef::new("Helvetica"), 7.0),
            },
            position: Point::new(x, y),
            max_width: None,
        });
        y += 10.0;
    }

    // Subtotal, tax, total
    let tax_rate = 0.08;
    let tax = subtotal * tax_rate;
    let total = subtotal + tax;

    y += 4.0;
    page.add(DrawCommand::FillRect {
        rect: perfect_print_core::units::Rect::new(x, y, 196.0, 0.5),
        color: Color::black(),
    });
    y += 8.0;

    page.add(DrawCommand::Text {
        run: TextRun {
            text: format!("Subtotal:  ${:.2}", subtotal),
            glyphs: vec![],
            style: TextStyle::new(FontRef::new("Helvetica"), 7.0),
        },
        position: Point::new(x, y),
        max_width: None,
    });
    y += 10.0;

    page.add(DrawCommand::Text {
        run: TextRun {
            text: format!("Tax (8%):  ${:.2}", tax),
            glyphs: vec![],
            style: TextStyle::new(FontRef::new("Helvetica"), 7.0),
        },
        position: Point::new(x, y),
        max_width: None,
    });
    y += 12.0;

    page.add(DrawCommand::Text {
        run: TextRun {
            text: format!("TOTAL:     ${:.2}", total),
            glyphs: vec![],
            style: TextStyle::new(FontRef::new("Helvetica"), 9.0),
        },
        position: Point::new(x, y),
        max_width: None,
    });
    y += 16.0;

    // Payment method
    page.add(DrawCommand::Text {
        run: TextRun {
            text: "Paid: Visa ****1234".to_string(),
            glyphs: vec![],
            style: TextStyle::new(FontRef::new("Helvetica"), 7.0),
        },
        position: Point::new(x, y),
        max_width: None,
    });
    y += 14.0;

    // Footer
    page.add(DrawCommand::FillRect {
        rect: perfect_print_core::units::Rect::new(x, y, 196.0, 0.5),
        color: Color::black(),
    });
    y += 8.0;

    page.add(DrawCommand::Text {
        run: TextRun {
            text: "Thank you for your business!\nReturns accepted within 30 days\nwith receipt"
                .to_string(),
            glyphs: vec![],
            style: TextStyle::new(FontRef::new("Helvetica"), 6.0),
        },
        position: Point::new(x, y),
        max_width: None,
    });

    DocumentBuilder::new()
        .title("Receipt")
        .add_page(page)
        .build()
        .unwrap()
}
