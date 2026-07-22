# Windows & Linux Print Backend Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make `perfect-print-backend-linux` and `perfect-print-backend-windows` real, correct, verified print backends — not just code that compiles on the author's machine and has never been checked against the actual OS.

**Architecture:**
- **Linux** currently binds directly to `libcups` via the `cups-sys` FFI crate. That crate is unmaintained (0.1.4), and its build script (bindgen) crashes on this dev machine trying to parse macOS SDK headers when cross-checking for `x86_64-unknown-linux-gnu` — it cannot even be typechecked here, let alone verified. `perfect-print-backend-macos` already solved the equivalent problem by shelling out to the CUPS command-line tools (`lp`/`lpstat`/`lpoptions`/`cancel`) instead of FFI — and since Linux CUPS installs ship the *identical* command-line tools, and macOS also ships them (it's CUPS-based too), the fix is to port macOS's already-tested `Command`-based implementation to Linux verbatim. This has a major side benefit: the "Linux" backend's logic becomes directly testable *on this Mac*, because the same `lp`/`lpstat` binaries exist here.
- **Windows** currently opens a raw spooler handle (`OpenPrinter`/`StartDocPrinter`/`WritePrinter`) and writes the finished PDF's bytes straight into it with the default ("RAW") datatype. That only produces correct output if the printer's driver itself understands raw PDF bytes as its native page-description language, which is not true for the vast majority of Windows printer drivers (RAW means "already in the printer's native language" — PCL/PostScript/etc, not PDF). The fix is to rasterize each page with the *existing* `perfect-print-render` crate (already produces `tiny_skia::Pixmap`s from a `DocumentModel`, already a workspace dependency) and blit each page bitmap onto a GDI printer device context (`CreateDC` → `StartDoc`/`StartPage` → `StretchDIBits` → `EndPage`/`EndDoc`) — the same WYSIWYG-bitmap strategy used by essentially every cross-platform app that prints on Windows without depending on driver-level PDF support. This requires passing the `DocumentModel` (not just finished PDF bytes) down to the Windows backend, so the top-level dispatch in `perfect-print/src/lib.rs` gains one new parameter.

**Tech Stack:** `windows` crate 0.58 (`Win32_Graphics_Gdi`, already an enabled feature), `tiny-skia` (already a workspace dep, pulled in via `perfect-print-render`), `std::process::Command` for the Linux CUPS-CLI rewrite. No new external dependencies.

**Verification ceiling — read this before starting:**
- **Linux track is fully testable on this Mac.** The new implementation is literally the same `Command::new("lp"/"lpstat"/"lpoptions"/"cancel")` calls macOS already uses and already has passing tests for (macOS ships CUPS too). `cargo test -p perfect-print-backend-linux` will exercise the real code path, not a stub, and will print real results about whatever printers/CUPS state exists on this Mac.
- **Windows track has a real remote build+run environment:** SSH host alias `windows` (already used elsewhere in this workspace for other projects' Windows builds) has a working Rust toolchain (`cargo 1.92`, confirmed reachable) and MSVC Build Tools reachable via `VsDevCmd.bat` at `C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\Common7\Tools\VsDevCmd.bat`. It also has a **working "Microsoft Print to PDF" virtual printer already installed** (confirmed via `Get-Printer`), which is a real Windows printer entry that accepts a real GDI print job and writes a real PDF file to a path you control via `DOCINFOA.lpszOutput` — this means Task 3's verification is a genuine, real, end-to-end print job through the actual Win32 GDI printing API, not a typecheck-only guess. Use it. Do not settle for `cargo check --target x86_64-pc-windows-msvc` alone when a real build+run is available.
- **What's still not verifiable from here:** output on a *physical* paper printer, and printer-driver quirks across the huge variety of real-world Windows drivers. The plan does not attempt to close that gap — it closes the "does this even work at all" gap, which today's code fails.

---

### Task 1: Rewrite the Linux backend to use CUPS command-line tools

**Files:**
- Modify: `crates/perfect-print-backend-linux/src/lib.rs` (full rewrite of the body, same public API)
- Modify: `crates/perfect-print-backend-linux/Cargo.toml` (drop `cups-sys`)
- Reference (read, do not modify): `crates/perfect-print-backend-macos/src/lib.rs` lines 147–469 — this is the pattern being ported

- [ ] **Step 1: Read the reference implementation**

Read `crates/perfect-print-backend-macos/src/lib.rs`'s `MacosPrintDialog` impl block (`enumerate_printers`, `get_default_printer_name`, `get_printer_caps`, `submit_print_job`, `parse_job_id`, `poll_job_status`, `list_jobs`, `cancel_job`, and the `PrintDialog` trait impl below it) end to end. This is the exact behavior and command set to port — same `lp`/`lpstat`/`lpoptions`/`cancel` invocations, same output-parsing logic, because Linux CUPS and macOS CUPS expose the identical CLI surface for these operations.

- [ ] **Step 2: Replace `crates/perfect-print-backend-linux/src/lib.rs` in full**

```rust
//! Linux CUPS print backend.
//!
//! Uses the CUPS command-line tools (`lp`, `lpstat`, `lpoptions`, `cancel`) —
//! the same tools `perfect-print-backend-macos` uses, since macOS's print
//! system is also CUPS-based. Shelling out avoids depending on `libcups`'s
//! C ABI/header surface (which drifts across CUPS versions and distros) and
//! means this backend's logic is exercised by the exact same code path on
//! any machine with CUPS command-line tools installed — including macOS,
//! which is how this crate's tests validate real behavior in CI/dev even
//! when the target OS is not Linux.

use perfect_print_core::page::PageSize;
use perfect_print_dialog::{
    ColorMode as _, DuplexMode, PageOrientation, PageRange, PrintDialog, PrintDialogResult,
    PrintError, PrintScaling, PrintSettings, Printer, PrinterCapabilities, PrinterState,
};
use std::path::Path;
use std::process::Command;

/// Linux CUPS print backend (command-line, not `libcups` FFI).
pub struct LinuxPrintDialog;

impl LinuxPrintDialog {
    pub fn new() -> Self {
        Self
    }

    /// Enumerate printers via `lpstat -a`.
    fn enumerate_printers(&self) -> Vec<Printer> {
        let mut printers = Vec::new();

        let output = Command::new("lpstat").args(["-a", "--"]).output();
        let output = match output {
            Ok(o) if o.status.success() => o,
            _ => return printers,
        };

        let stdout = String::from_utf8_lossy(&output.stdout);
        let default_printer = Self::get_default_printer_name();

        for line in stdout.lines() {
            // Format: "printer_name accepting requests since .."
            let name = line.split_whitespace().next().unwrap_or("");
            if name.is_empty() {
                continue;
            }

            let is_default = default_printer.as_ref().is_some_and(|d| d == name);
            let mut caps = self.get_printer_caps(name);
            caps.is_default = is_default;

            printers.push(Printer::new(caps));
        }

        printers.sort_by(|a, b| {
            b.capabilities
                .is_default
                .cmp(&a.capabilities.is_default)
                .then_with(|| a.capabilities.name.cmp(&b.capabilities.name))
        });

        printers
    }

    fn get_default_printer_name() -> Option<String> {
        let output = Command::new("lpstat").args(["-d"]).output().ok()?;
        if !output.status.success() {
            return None;
        }
        let stdout = String::from_utf8_lossy(&output.stdout);
        // Format: "system default destination: printer_name"
        stdout.split(": ").nth(1).map(|s| s.trim().to_string())
    }

    fn get_printer_caps(&self, name: &str) -> PrinterCapabilities {
        let mut paper_sizes = vec![PageSize::Letter, PageSize::A4, PageSize::Legal];
        let mut supports_color = false;
        let mut supports_duplex = false;

        if let Ok(output) = Command::new("lpoptions").args(["-p", name, "-l"]).output() {
            if output.status.success() {
                let stdout = String::from_utf8_lossy(&output.stdout);
                for line in stdout.lines() {
                    let lower = line.to_lowercase();
                    if lower.contains("color") || lower.contains("cmyk") || lower.contains("rgb") {
                        supports_color = true;
                    }
                    if lower.contains("duplex")
                        || lower.contains("double-sided")
                        || lower.contains("two-sided")
                    {
                        supports_duplex = true;
                    }
                    if lower.starts_with("pagesize=") || lower.contains("pagesize=") {
                        for size in &[
                            ("letter", PageSize::Letter),
                            ("a4", PageSize::A4),
                            ("legal", PageSize::Legal),
                            ("tabloid", PageSize::Tabloid),
                            ("a3", PageSize::A3),
                            ("a5", PageSize::A5),
                        ] {
                            if lower.contains(size.0) && !paper_sizes.contains(&size.1) {
                                paper_sizes.push(size.1);
                            }
                        }
                    }
                }
            }
        }

        let state = if let Ok(output) = Command::new("lpstat").args(["-p", name]).output() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            if stdout.contains("disabled") {
                PrinterState::Error("Printer disabled".to_string())
            } else {
                PrinterState::Ready
            }
        } else {
            PrinterState::Offline
        };

        PrinterCapabilities {
            name: name.to_string(),
            paper_sizes,
            supports_color,
            supports_duplex,
            max_resolution: None,
            supported_resolutions: vec![],
            supports_borderless: false,
            is_default: false,
            state,
        }
    }

    /// Submit a print job via `lp`.
    pub fn submit_print_job(
        &self,
        pdf_path: &Path,
        settings: &PrintSettings,
    ) -> PrintDialogResult<Option<String>> {
        let mut cmd = Command::new("lp");

        let printers = self.enumerate_printers();
        if let Some(default) = printers.iter().find(|p| p.capabilities.is_default) {
            cmd.arg("-d").arg(&default.capabilities.name);
        }

        if settings.copies > 1 {
            cmd.arg("-n").arg(settings.copies.to_string());
        }

        match &settings.page_range {
            PageRange::All => {}
            PageRange::Range(start, end) => {
                cmd.arg("-P").arg(format!("{}-{}", start, end));
            }
            PageRange::Pages(pages) => {
                let range: Vec<String> = pages.iter().map(|p| p.to_string()).collect();
                cmd.arg("-P").arg(range.join(","));
            }
        }

        if settings.duplex != DuplexMode::Simplex {
            cmd.arg("-o").arg("sides=two-sided-long-edge");
        }

        if settings.orientation == PageOrientation::Landscape {
            cmd.arg("-o").arg("orientation-requested=4");
        }

        match &settings.scaling {
            PrintScaling::FitToPage => {
                cmd.arg("-o").arg("fit-to-page");
            }
            PrintScaling::FillPage => {
                cmd.arg("-o").arg("fill");
            }
            PrintScaling::None => {}
            PrintScaling::Custom(s) => {
                cmd.arg("-o").arg(format!("scaling={}", (s * 100.0) as u32));
            }
        }

        if settings.collate && settings.copies > 1 {
            cmd.arg("-o").arg("Collate=True");
        }

        let paper_arg = match &settings.paper_size {
            PageSize::Letter => "Letter",
            PageSize::A4 => "A4",
            PageSize::Legal => "Legal",
            PageSize::Tabloid => "Tabloid",
            PageSize::A3 => "A3",
            PageSize::A5 => "A5",
            _ => "Letter",
        };
        cmd.arg("-o").arg(format!("media={}", paper_arg));

        cmd.arg("-t").arg("perfect-print job");
        cmd.arg(pdf_path);

        let output = cmd
            .output()
            .map_err(|e| PrintError::Platform(format!("Failed to run lp: {}", e)))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(PrintError::PrintFailed(format!(
                "lp failed ({}): {}",
                output.status, stderr
            )));
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let job_id = Self::parse_job_id(&stdout);
        log::info!("Print job submitted: {} (id: {:?})", stdout.trim(), job_id);
        Ok(job_id)
    }

    /// Parse the job ID from `lp` output.
    /// Typical output: "request id is PrinterName-42 (1 file(s))"
    fn parse_job_id(stdout: &str) -> Option<String> {
        let start = stdout.find("request id is ")? + "request id is ".len();
        let end = stdout[start..].find(' ').map(|i| start + i)?;
        Some(stdout[start..end].to_string())
    }

    /// Poll job status via `lpstat -o`. Returns `Ok(true)` if completed
    /// (no longer in the queue), `Ok(false)` if still queued/printing.
    pub fn poll_job_status(&self, job_id: &str) -> PrintDialogResult<bool> {
        let output = Command::new("lpstat")
            .args(["-o"])
            .output()
            .map_err(|e| PrintError::Platform(format!("Failed to run lpstat: {}", e)))?;

        if !output.status.success() {
            return Err(PrintError::Platform("lpstat -o failed".to_string()));
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let in_queue = stdout.lines().any(|line| line.starts_with(job_id));
        Ok(!in_queue)
    }

    /// List all pending print jobs as (job_id, printer, status) tuples.
    pub fn list_jobs(&self) -> PrintDialogResult<Vec<(String, String, String)>> {
        let output = Command::new("lpstat")
            .args(["-o"])
            .output()
            .map_err(|e| PrintError::Platform(format!("Failed to run lpstat: {}", e)))?;

        if !output.status.success() {
            return Err(PrintError::Platform("lpstat -o failed".to_string()));
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let mut jobs = Vec::new();
        for line in stdout.lines() {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 3 {
                let job_id = parts[0].to_string();
                if let Some(dash_pos) = job_id.rfind('-') {
                    let printer = job_id[..dash_pos].to_string();
                    let status = if line.contains("ready") {
                        "ready".to_string()
                    } else {
                        "printing".to_string()
                    };
                    jobs.push((job_id, printer, status));
                }
            }
        }
        Ok(jobs)
    }

    /// Cancel a print job by ID via `cancel`.
    pub fn cancel_job(&self, job_id: &str) -> PrintDialogResult<()> {
        let output = Command::new("cancel")
            .arg(job_id)
            .output()
            .map_err(|e| PrintError::Platform(format!("Failed to run cancel: {}", e)))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(PrintError::PrintFailed(format!(
                "cancel failed: {}",
                stderr.trim()
            )));
        }
        log::info!("Print job {} cancelled", job_id);
        Ok(())
    }
}

impl PrintDialog for LinuxPrintDialog {
    fn show_print_dialog(
        &self,
        settings: &PrintSettings,
        _document_title: Option<&str>,
    ) -> PrintDialogResult<PrintSettings> {
        log::warn!("Native print dialog not implemented on Linux. Use submit_print_job for printing.");
        Ok(settings.clone())
    }

    fn show_page_setup(&self, settings: &PrintSettings) -> PrintDialogResult<PrintSettings> {
        Ok(settings.clone())
    }

    fn available_printers(&self) -> PrintDialogResult<Vec<Printer>> {
        Ok(self.enumerate_printers())
    }

    fn default_printer(&self) -> PrintDialogResult<Printer> {
        let printers = self.enumerate_printers();
        printers
            .into_iter()
            .find(|p| p.capabilities.is_default)
            .ok_or(PrintError::NoPrinters)
    }
}

impl Default for LinuxPrintDialog {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dialog_trait_impl() {
        let dialog = LinuxPrintDialog::new();
        let settings = PrintSettings::default();
        let result = dialog.show_print_dialog(&settings, Some("Test"));
        assert!(result.is_ok());
    }

    #[test]
    fn test_page_setup_dialog() {
        let dialog = LinuxPrintDialog::new();
        let settings = PrintSettings::default();
        let result = dialog.show_page_setup(&settings);
        assert!(result.is_ok());
    }

    #[test]
    fn test_available_printers_does_not_panic() {
        let dialog = LinuxPrintDialog::new();
        let result = dialog.available_printers();
        assert!(result.is_ok());
        eprintln!("Found {} printer(s) via lpstat", result.unwrap().len());
    }

    #[test]
    fn test_default_printer_reports_no_printers_or_a_real_one() {
        let dialog = LinuxPrintDialog::new();
        match dialog.default_printer() {
            Ok(p) => eprintln!("Default printer: {}", p.capabilities.name),
            Err(PrintError::NoPrinters) => eprintln!("No printers configured (expected in CI)"),
            Err(e) => panic!("Unexpected error: {e}"),
        }
    }

    #[test]
    fn test_submit_job_nonexistent_file() {
        let dialog = LinuxPrintDialog::new();
        let path = Path::new("/tmp/nonexistent_perfect_print_test_12345.pdf");
        let result = dialog.submit_print_job(path, &PrintSettings::default());
        assert!(result.is_err(), "Should fail for nonexistent file");
    }

    #[test]
    fn test_parse_job_id_extracts_id() {
        let stdout = "request id is HP_LaserJet-42 (1 file(s))\n";
        assert_eq!(
            LinuxPrintDialog::parse_job_id(stdout),
            Some("HP_LaserJet-42".to_string())
        );
    }

    #[test]
    fn test_parse_job_id_handles_missing_marker() {
        assert_eq!(LinuxPrintDialog::parse_job_id("lp: no destinations added"), None);
    }
}
```

Note the removed `ColorMode` import warning: `ColorMode as _` is imported only if actually used — if `cargo check` reports it unused, delete that import entirely rather than leaving an unused import warning. (This mirrors the reference file, which also doesn't reference `ColorMode` directly in the CLI-arg-building logic — check before assuming.)

- [ ] **Step 3: Drop the `cups-sys` dependency**

Edit `crates/perfect-print-backend-linux/Cargo.toml`, remove:
```toml
[target.'cfg(target_os = "linux")'.dependencies]
cups-sys = { workspace = true }
```
so the file's `[dependencies]` section is just `perfect-print-core`, `perfect-print-dialog`, `log` (drop `perfect-print-pdf` too if nothing in the new file references it — check with `cargo check` after the rewrite; the old file didn't use it either as far as the read shows, so it was likely already dead weight).

Also remove the now-unused `cups-sys = "0.1"` entry from the root `Cargo.toml`'s `[workspace.dependencies]` if nothing else in the workspace references it (`grep -rn cups-sys crates/ Cargo.toml` after this change should show nothing).

- [ ] **Step 4: Run the real tests**

```bash
cargo test -p perfect-print-backend-linux -- --nocapture
```
Expected: all pass. The `eprintln!`s will show real output from this Mac's CUPS state (e.g. "Found 1 printer(s) via lpstat") — that's the point: this is genuine behavior, not a mocked stub.

- [ ] **Step 5: Full workspace check**

```bash
cargo check --workspace
cargo test --workspace
```
Expected: no new failures.

- [ ] **Step 6: Commit**

```bash
git add crates/perfect-print-backend-linux/src/lib.rs crates/perfect-print-backend-linux/Cargo.toml Cargo.toml Cargo.lock
git commit -m "$(cat <<'EOF'
fix(linux): replace cups-sys FFI with CUPS command-line tools

cups-sys is unmaintained and its bindgen-based build script cannot
even be typechecked on this workspace's dev machine (it crashes
parsing macOS SDK headers when cross-checking for Linux). Port
perfect-print-backend-macos's already-tested lp/lpstat/lpoptions/cancel
Command-based implementation instead — Linux and macOS CUPS expose
the identical CLI surface for these operations, so this is not a
degradation, and it has the added benefit of being directly testable
on any CUPS-based dev machine (including this one).

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>
EOF
)"
```

---

### Task 2: Windows backend — GDI bitmap printing via the existing raster renderer

**Files:**
- Modify: `crates/perfect-print/src/lib.rs` (`platform_print_bytes`/`platform_print_file` gain a `model: &DocumentModel` parameter)
- Modify: `crates/perfect-print-backend-windows/src/lib.rs` (rewrite `submit_print_job`; keep `poll_job_status`/`list_jobs`/`cancel_job`/`enumerate_printers` as-is — they operate on the spooler job-id namespace, which `StartDoc` also enqueues into, so they still work unchanged)
- Modify: `crates/perfect-print-backend-windows/Cargo.toml` (add `perfect-print-render`, and the `tiny-skia` workspace dep, both windows-only since that's the only place this crate uses rasterization)

- [ ] **Step 1: Thread `DocumentModel` through the top-level dispatch**

Read `crates/perfect-print/src/lib.rs` around `print_document_with`/`platform_print_bytes`/`platform_print_file` (currently ~lines 316–384). Change all three `platform_print_bytes` variants (macOS, linux+windows) to take an additional `model: &DocumentModel` parameter — macOS's implementation ignores it (prefix `_model`), linux+windows's implementation threads it into `platform_print_file`, whose linux variant also ignores it (prefix `_model`) and whose windows variant uses it.

```rust
pub fn print_document_with(
    model: &DocumentModel,
    settings: &PrintSettings,
) -> Result<Option<String>, PrintError> {
    let pdf_bytes = PdfRenderer::new()
        .render_to_bytes(model)
        .map_err(|e| PrintError::PrintFailed(format!("PDF render failed: {}", e)))?;
    platform_print_bytes(
        model,
        &pdf_bytes,
        model.metadata.title.as_deref().unwrap_or("Perfect Print"),
        settings,
    )
}

#[cfg(target_os = "macos")]
fn platform_print_bytes(
    _model: &DocumentModel,
    pdf_bytes: &[u8],
    title: &str,
    settings: &PrintSettings,
) -> Result<Option<String>, PrintError> {
    perfect_print_backend_macos::print_pdf_bytes_with_dialog(pdf_bytes, Some(title), settings)
        .map(|submitted| submitted.then(|| "native-print-dialog".to_string()))
}

#[cfg(any(target_os = "linux", target_os = "windows"))]
fn platform_print_bytes(
    model: &DocumentModel,
    pdf_bytes: &[u8],
    _title: &str,
    settings: &PrintSettings,
) -> Result<Option<String>, PrintError> {
    use std::io::Write;

    let mut pdf_file = tempfile::Builder::new()
        .prefix("perfect-print-")
        .suffix(".pdf")
        .tempfile()
        .map_err(|e| PrintError::PrintFailed(format!("Temporary PDF create failed: {}", e)))?;
    pdf_file
        .write_all(pdf_bytes)
        .and_then(|_| pdf_file.flush())
        .map_err(|e| PrintError::PrintFailed(format!("Temporary PDF write failed: {}", e)))?;
    platform_print_file(model, pdf_file.path(), settings)
}

#[cfg(target_os = "linux")]
fn platform_print_file(
    _model: &DocumentModel,
    pdf_path: &std::path::Path,
    settings: &PrintSettings,
) -> Result<Option<String>, PrintError> {
    let dialog = perfect_print_backend_linux::LinuxPrintDialog::new();
    dialog.submit_print_job(pdf_path, settings)
}

#[cfg(target_os = "windows")]
fn platform_print_file(
    model: &DocumentModel,
    _pdf_path: &std::path::Path,
    settings: &PrintSettings,
) -> Result<Option<String>, PrintError> {
    let dialog = perfect_print_backend_windows::WindowsPrintDialog::new();
    dialog.submit_print_job(model, settings)
}
```

Note `_pdf_path` on the Windows `platform_print_file`: the temp PDF file is still written (by the caller, `platform_print_bytes`) for parity/possible future use, but the Windows path no longer needs it since it rasterizes straight from `model`. Leave the temp-file plumbing in `platform_print_bytes` alone — don't skip writing it, since that would require another cfg-split; the wasted temp-file write is cheap and harmless.

`WindowsPrintDialog::submit_print_job`'s signature changes from `(&self, pdf_path: &Path, settings: &PrintSettings)` to `(&self, model: &DocumentModel, settings: &PrintSettings)` — update accordingly in Step 3 below.

- [ ] **Step 2: Add dependencies**

`crates/perfect-print-backend-windows/Cargo.toml`:
```toml
[dependencies]
perfect-print-core = { path = "../perfect-print-core" }
perfect-print-pdf = { path = "../perfect-print-pdf" }
perfect-print-dialog = { path = "../perfect-print-dialog" }
log = { workspace = true }

[target.'cfg(target_os = "windows")'.dependencies]
windows = { workspace = true }
perfect-print-render = { path = "../perfect-print-render" }
tiny-skia = { workspace = true }
```
(`perfect-print-pdf` may now be unused if nothing else in the file references it — check with `cargo check --target x86_64-pc-windows-msvc` after Step 3 and remove it if so, matching the same cleanup done for Linux in Task 1.)

- [ ] **Step 3: Rewrite `submit_print_job` to rasterize + GDI-blit**

In `crates/perfect-print-backend-windows/src/lib.rs`, replace the `#[cfg(target_os = "windows")] pub fn submit_print_job` body (and its signature) with:

```rust
#[cfg(target_os = "windows")]
use perfect_print_core::document::DocumentModel;
#[cfg(target_os = "windows")]
use perfect_print_render::TinySkiaRenderer;
#[cfg(target_os = "windows")]
use perfect_print_render::Render as _;
#[cfg(target_os = "windows")]
use perfect_print_core::units::Dpi;

/// Submit a print job by rasterizing every page and blitting each page's
/// bitmap onto a GDI printer device context.
///
/// This does not depend on the printer driver understanding PDF (most
/// don't, when fed raw bytes) — it produces a bitmap of exactly what
/// perfect-print's own raster/PNG output would show, at the printer's
/// own reported resolution, so output is WYSIWYG on any GDI-capable
/// Windows printer driver.
#[cfg(target_os = "windows")]
pub fn submit_print_job_impl(
    model: &DocumentModel,
    settings: &PrintSettings,
) -> PrintDialogResult<Option<String>> {
    let printer_name = Self::get_default_printer_name().ok_or(PrintError::NoPrinters)?;
    let printer_name_cstr = std::ffi::CString::new(printer_name.clone())
        .map_err(|e| PrintError::Platform(format!("Invalid printer name: {}", e)))?;

    let hdc = unsafe {
        CreateDCA(
            PCSTR::null(),
            PCSTR(printer_name_cstr.as_ptr() as *const u8),
            PCSTR::null(),
            None,
        )
    };
    if hdc.is_invalid() {
        return Err(PrintError::Platform(format!(
            "CreateDC failed for printer '{}'",
            printer_name
        )));
    }

    // Ensure the DC is always released, even on an early error return.
    struct DcGuard(HDC);
    impl Drop for DcGuard {
        fn drop(&mut self) {
            unsafe {
                let _ = DeleteDC(self.0);
            }
        }
    }
    let _dc_guard = DcGuard(hdc);

    let dpi_x = unsafe { GetDeviceCaps(hdc, LOGPIXELSX) };
    let dpi_y = unsafe { GetDeviceCaps(hdc, LOGPIXELSY) };
    let dpi = if dpi_x > 0 { dpi_x as f64 } else { 300.0 };
    let _ = dpi_y; // assumed equal to dpi_x for print DCs; not used separately below

    let phys_width = unsafe { GetDeviceCaps(hdc, PHYSICALWIDTH) };
    let phys_height = unsafe { GetDeviceCaps(hdc, PHYSICALHEIGHT) };
    let offset_x = unsafe { GetDeviceCaps(hdc, PHYSICALOFFSETX) };
    let offset_y = unsafe { GetDeviceCaps(hdc, PHYSICALOFFSETY) };

    let doc_name = "perfect-print job";
    let doc_name_cstr = std::ffi::CString::new(doc_name).unwrap();
    let doc_info = DOCINFOA {
        cbSize: std::mem::size_of::<DOCINFOA>() as i32,
        lpszDocName: PCSTR(doc_name_cstr.as_ptr() as *const u8),
        lpszOutput: PCSTR::null(),
        lpszDatatype: PCSTR::null(),
        fwType: 0,
    };

    let job_id = unsafe { StartDocA(hdc, &doc_info) };
    if job_id <= 0 {
        return Err(PrintError::PrintFailed("StartDoc failed".to_string()));
    }

    let renderer = TinySkiaRenderer::new();
    let page_indices = resolve_page_indices(&settings.page_range, model.page_count());

    for _copy in 0..settings.copies.max(1) {
        for &page_index in &page_indices {
            if unsafe { StartPage(hdc) } <= 0 {
                unsafe { let _ = AbortDoc(hdc); };
                return Err(PrintError::PrintFailed("StartPage failed".to_string()));
            }

            let pixmap = renderer
                .render_page_to_pixmap(model, page_index, Dpi(dpi))
                .map_err(|e| {
                    PrintError::PrintFailed(format!(
                        "Failed to rasterize page {}: {}",
                        page_index + 1,
                        e
                    ))
                })?;

            blit_pixmap(hdc, &pixmap, phys_width, phys_height, offset_x, offset_y)?;

            if unsafe { EndPage(hdc) } <= 0 {
                unsafe { let _ = AbortDoc(hdc); };
                return Err(PrintError::PrintFailed("EndPage failed".to_string()));
            }
        }
    }

    if unsafe { EndDoc(hdc) } <= 0 {
        return Err(PrintError::PrintFailed("EndDoc failed".to_string()));
    }

    log::info!(
        "Print job submitted: {} page(s) to printer '{}' (job {})",
        page_indices.len(),
        printer_name,
        job_id
    );

    Ok(Some(format!("{}-{}", printer_name, job_id)))
}

/// Resolve a `PageRange` against an actual page count into a concrete,
/// 0-indexed, in-order list of page indices to print.
#[cfg(target_os = "windows")]
fn resolve_page_indices(range: &PageRange, page_count: usize) -> Vec<usize> {
    match range {
        PageRange::All => (0..page_count).collect(),
        PageRange::Range(start, end) => {
            let start = (*start).max(1) as usize;
            let end = (*end as usize).min(page_count);
            if start > end {
                vec![]
            } else {
                (start - 1..end).collect()
            }
        }
        PageRange::Pages(pages) => pages
            .iter()
            .filter(|&&p| p >= 1 && (p as usize) <= page_count)
            .map(|&p| (p - 1) as usize)
            .collect(),
    }
}

/// Blit a rendered page bitmap onto the printer DC, filling the full
/// physical page (offset by the driver-reported unprintable margin) —
/// matching the page's own point-size 1:1 at the DC's reported DPI, the
/// same "just draw the page, let the driver map it to paper" contract
/// perfect-print-backend-macos relies on via PDFKit.
#[cfg(target_os = "windows")]
fn blit_pixmap(
    hdc: HDC,
    pixmap: &tiny_skia::Pixmap,
    phys_width: i32,
    phys_height: i32,
    offset_x: i32,
    offset_y: i32,
) -> PrintDialogResult<()> {
    let width = pixmap.width() as i32;
    let height = pixmap.height() as i32;

    // tiny-skia's Pixmap is premultiplied RGBA, top-to-bottom rows.
    // GDI DIBs expect BGRA (or a negative height for top-down) with a
    // positive-height bottom-up row order by convention; use a negative
    // biHeight to keep the pixmap's natural top-down row order and swap
    // R/B per pixel into a BGRA scratch buffer GDI expects.
    let mut bgra = vec![0u8; pixmap.data().len()];
    for (src, dst) in pixmap.data().chunks_exact(4).zip(bgra.chunks_exact_mut(4)) {
        dst[0] = src[2]; // B
        dst[1] = src[1]; // G
        dst[2] = src[0]; // R
        dst[3] = src[3]; // A
    }

    let bmi = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: width,
            biHeight: -height, // negative = top-down
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0 as u32,
            biSizeImage: 0,
            biXPelsPerMeter: 0,
            biYPelsPerMeter: 0,
            biClrUsed: 0,
            biClrImportant: 0,
        },
        bmiColors: [Default::default(); 1],
    };

    let dest_width = phys_width.max(1);
    let dest_height = phys_height.max(1);

    let result = unsafe {
        StretchDIBits(
            hdc,
            -offset_x,
            -offset_y,
            dest_width,
            dest_height,
            0,
            0,
            width,
            height,
            Some(bgra.as_ptr() as *const std::ffi::c_void),
            &bmi,
            DIB_RGB_COLORS,
            SRCCOPY,
        )
    };

    if result == 0 || result == GDI_ERROR as i32 {
        return Err(PrintError::PrintFailed(
            "StretchDIBits failed while printing a page".to_string(),
        ));
    }

    Ok(())
}
```

Update the `pub struct WindowsPrintDialog` inherent `impl` block's public entry point to dispatch to this — since the crate's public `submit_print_job` must keep working on non-Windows (stub) too, restructure as:

```rust
impl WindowsPrintDialog {
    // ... enumerate_printers, get_default_printer_name, poll_job_status,
    //     list_jobs, cancel_job, paper_size_from_win32 all stay exactly
    //     as they are today ...

    #[cfg(target_os = "windows")]
    pub fn submit_print_job(
        &self,
        model: &perfect_print_core::document::DocumentModel,
        settings: &PrintSettings,
    ) -> PrintDialogResult<Option<String>> {
        Self::submit_print_job_impl(model, settings)
    }
}

#[cfg(not(target_os = "windows"))]
impl WindowsPrintDialog {
    pub fn submit_print_job(
        &self,
        _model: &perfect_print_core::document::DocumentModel,
        _settings: &PrintSettings,
    ) -> PrintDialogResult<Option<String>> {
        Err(PrintError::Platform(
            "Windows backend not available on this platform".to_string(),
        ))
    }
    // enumerate_printers/poll_job_status/list_jobs/cancel_job stubs stay as-is
}
```

Add the necessary `use` items at the top of the file inside the existing `#[cfg(target_os = "windows")] use windows::{...}` block: `CreateDCA`, `DeleteDC`, `GetDeviceCaps`, `LOGPIXELSX`, `LOGPIXELSY`, `PHYSICALWIDTH`, `PHYSICALHEIGHT`, `PHYSICALOFFSETX`, `PHYSICALOFFSETY`, `StartDocA`, `StartPage`, `EndPage`, `EndDoc`, `AbortDoc`, `DOCINFOA`, `StretchDIBits`, `BITMAPINFO`, `BITMAPINFOHEADER`, `BI_RGB`, `DIB_RGB_COLORS`, `SRCCOPY`, `GDI_ERROR`. These are exported by the `windows` crate's `Win32::Graphics::Gdi` module, already covered by the `Win32_Graphics_Gdi` feature already enabled in the workspace `windows` dependency — no `Cargo.toml` feature changes needed.

**This is the one step in this plan where exact windows-rs 0.58 signatures cannot be guaranteed correct purely from memory** (parameter types like `Option<*const DEVMODEA>` vs `Option<&DEVMODEA>`, or whether some out-params are `&mut i32` vs return values, shift slightly between windows-rs versions). Treat `cargo check --target x86_64-pc-windows-msvc -p perfect-print-backend-windows` as the TDD loop for this step: run it, fix whatever signature mismatches the compiler reports (the Win32 *semantics* above are correct and stable — only the Rust binding's exact type spelling might need adjusting), re-run, repeat until clean. This is legitimate compiler-driven FFI development, not a placeholder — the logic and control flow are fully specified above; only mechanical type-signature details may need correction.

- [ ] **Step 4: Cross-typecheck**

```bash
rustup target add x86_64-pc-windows-msvc   # already installed on this machine; no-op if so
cargo check --target x86_64-pc-windows-msvc -p perfect-print-backend-windows
```
Iterate per Step 3's note until this is clean with zero errors (warnings OK).

- [ ] **Step 5: Real link + run verification on the remote Windows build host**

There is an SSH host alias `windows` with a working Rust toolchain and MSVC Build Tools (via `VsDevCmd.bat`), and a real, already-installed "Microsoft Print to PDF" virtual printer (confirmed via `Get-Printer`) that accepts genuine GDI print jobs and writes real PDF output to a caller-specified path. Use it for actual link-level and runtime verification — do not stop at cross-typechecking when a real build+run is available.

5a. Sync the workspace (or just the crates needed to build `perfect-print-backend-windows` standalone as a binary) to the Windows host:
```bash
ssh windows "cmd /c if not exist C:\PerfectPrint-Build mkdir C:\PerfectPrint-Build"
scp -r crates windows:C:/PerfectPrint-Build/
scp Cargo.toml Cargo.lock windows:C:/PerfectPrint-Build/
```

5b. Add a small verification binary at `crates/perfect-print-backend-windows/examples/print_to_pdf_smoke_test.rs` (only compiled/run on Windows; not part of the library's public surface) that builds a simple multi-element `DocumentModel` via the `perfect_print` crate's `Document` builder (add `perfect-print` as a `[dev-dependencies]` / `target.'cfg(windows)'.dev-dependencies` entry in this crate's `Cargo.toml`, path `../perfect-print`), sets `PrintSettings::paper_size = PageSize::Letter`, and calls a Windows-only helper that opens `"Microsoft Print to PDF"` directly by name (not the system default) with `DOCINFOA.lpszOutput` set to `C:\PerfectPrint-Build\smoke_test_output.pdf`, so the driver writes there without needing an interactive save dialog:

```rust
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
        .add(Paragraph::new("Windows GDI print backend smoke test").font_size(24.0).bold())
        .add(Paragraph::new("If you can read this in the output PDF, StretchDIBits worked."))
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
```

Add the corresponding `print_to_named_printer_for_test` helper to `crates/perfect-print-backend-windows/src/lib.rs` (windows-only, `#[doc(hidden)]`, public only so the example binary can call it — same GDI sequence as `submit_print_job_impl` but taking an explicit printer name and output path instead of the default printer / no-output-file, by setting `DOCINFOA.lpszOutput` to a `PCSTR` over the given path's `CString` instead of `PCSTR::null()`). Factor the shared per-page rasterize+blit loop out of `submit_print_job_impl` into a private helper both call, rather than duplicating it.

5c. Build and run on the remote host through the MSVC dev environment:
```bash
ssh windows "cmd /c \"\"C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\Common7\Tools\VsDevCmd.bat\" -arch=amd64 -host_arch=amd64 && cd /d C:\PerfectPrint-Build && cargo build -p perfect-print-backend-windows --example print_to_pdf_smoke_test 2>&1\""
ssh windows "cmd /c \"\"C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\Common7\Tools\VsDevCmd.bat\" -arch=amd64 -host_arch=amd64 && cd /d C:\PerfectPrint-Build && cargo run -p perfect-print-backend-windows --example print_to_pdf_smoke_test 2>&1\""
```
Expected: build succeeds (real link against `windows-rs`'s generated import libs — this is the step that catches any remaining signature mismatches cross-typechecking might have missed), and the run prints `OK: job = ...`.

5d. Pull the resulting PDF back and visually verify it:
```bash
scp windows:C:/PerfectPrint-Build/smoke_test_output.pdf /tmp/pp-windows-smoke.pdf
```
Then rasterize and view it (Read tool) to confirm the text actually rendered — e.g. `cargo run -q -p perfect-print-cli -- verify` isn't applicable here since it's not comparing against a reference, so instead just rasterize the pulled-back PDF with any available PDF-to-PNG tool on this Mac (`sips -s format png`, as used elsewhere in this workspace's print-verification work) and view it. This is real proof the whole chain — rasterize with `perfect-print-render` → GDI `StretchDIBits` → driver → real PDF bytes — worked, not a guess.

5e. Clean up the remote scratch build dir when done (do not leave it) — but only after 5d has succeeded and been reviewed:
```bash
ssh windows "cmd /c rmdir /s /q C:\PerfectPrint-Build"
```

- [ ] **Step 6: Update tests that referenced the old `submit_print_job(&Path, ...)` signature**

`crates/perfect-print-backend-windows/src/lib.rs`'s existing `#[cfg(test)] mod tests` has `test_submit_job_nonexistent_file`, which passed a bogus `Path`. Since the signature is now `(model: &DocumentModel, settings: &PrintSettings)`, replace that test with one appropriate to the new signature on non-Windows (still exercises the stub error path):

```rust
#[test]
fn test_submit_job_stub_on_non_windows() {
    #[cfg(not(target_os = "windows"))]
    {
        let dialog = WindowsPrintDialog::new();
        let model = perfect_print_core::document::DocumentBuilder::new()
            .page(perfect_print_core::page::PageSize::Letter)
            .build()
            .unwrap();
        let result = dialog.submit_print_job(&model, &PrintSettings::default());
        assert!(result.is_err(), "Should fail on non-Windows (stub backend)");
    }
}
```
Remove the old `test_submit_job_nonexistent_file`.

- [ ] **Step 7: Full workspace check on this Mac (macOS build unaffected)**

```bash
cargo check --workspace
cargo test --workspace
```
Expected: no new failures — the macOS build never touches the Windows-only code paths, and the top-level dispatch signature change is additive/mechanical.

- [ ] **Step 8: Commit**

```bash
git add crates/perfect-print/src/lib.rs crates/perfect-print-backend-windows/src/lib.rs crates/perfect-print-backend-windows/Cargo.toml crates/perfect-print-backend-windows/examples Cargo.lock
git commit -m "$(cat <<'EOF'
fix(windows): print via GDI bitmap blit instead of raw PDF bytes to spooler

The old implementation wrote finished PDF bytes directly into the
spooler via WritePrinter with the default RAW datatype, which only
produces correct output if the printer driver's native language is
literally PDF -- true for almost no real Windows printer drivers.
Rasterize each page with the existing perfect-print-render crate and
blit the bitmap onto a GDI printer DC (CreateDC/StartDoc/StartPage/
StretchDIBits/EndPage/EndDoc) instead, which works on any GDI-capable
driver regardless of PDF support.

Verified with a real build + run on a remote Windows host against the
"Microsoft Print to PDF" virtual printer: pulled the resulting PDF
back and confirmed the rendered text is present and correct, not just
cross-typechecked.

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>
EOF
)"
```

---

### Task 3: Docs

**Files:**
- Modify: `README.md`
- Modify: `docs/IMPROVEMENT-PLAN.md`

- [ ] **Step 1: README**

Update the "Print backend" feature bullet:
```markdown
- **Print backend** — macOS via CUPS (`lp`/`lpstat`) and a native `NSPrintOperation` dialog with page-accurate placement; Linux via CUPS command-line tools (`lp`/`lpstat`/`lpoptions`/`cancel`); Windows via GDI (rasterized pages blitted through `StretchDIBits`, verified against the "Microsoft Print to PDF" printer end to end) — no browser or driver-level PDF support required on any platform
```
Remove the trailing "other backends are still maturing" clause it's replacing — say what's true now, not what was aspirational before.

- [ ] **Step 2: `docs/IMPROVEMENT-PLAN.md`**

Update gap #9 in the table near the top (`Windows/Linux backends empty | Medium | 4-5`) — change status to note it's resolved, and add a dated section (mirror the style of the existing 2026-07-21 entries) summarizing: the Linux CUPS-CLI rewrite and why (cups-sys unmaintained + unbuildable in this dev environment), the Windows GDI-bitmap rewrite and why (RAW PDF bytes don't print on real drivers), and the verification methods used for each (real local test execution for Linux, real remote build+link+run against a live virtual printer for Windows) — be as specific as the existing entries in that file about what was actually verified vs. what remains unverified (physical printer hardware, the long tail of real-world driver quirks).

- [ ] **Step 3: Commit**

```bash
git add README.md docs/IMPROVEMENT-PLAN.md
git commit -m "docs: Windows/Linux print backend rewrite

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

## Self-review notes (already applied)

- Task 1's Linux rewrite deliberately removes ALL `#[cfg(target_os = "linux")]` gating from the implementation body (matching how `perfect-print-backend-macos` is itself unguarded internally) — the top-level `perfect-print` crate's own `#[cfg(target_os = "linux")]` on `platform_print_file` is what actually restricts when this code is reachable in a real build; the internal gate was pure incidental caution that also happened to make the crate untestable anywhere but real Linux. Removing it is what unlocks Step 4's real test coverage.
- Task 2's verification plan explicitly escalates past typecheck-only to a real build+link+run+visual-check, because a real Windows build host with a real virtual printer is available — settling for `cargo check` alone here would repeat this session's earlier mistake of shipping FFI code that was never actually exercised.
- Type/signature names used across tasks are consistent: `LinuxPrintDialog`, `WindowsPrintDialog::submit_print_job(&self, model: &DocumentModel, settings: &PrintSettings)`, `platform_print_bytes(model, pdf_bytes, title, settings)`, `platform_print_file(model, pdf_path, settings)`.
- Known non-goal, stated explicitly so it isn't mistaken for a gap: neither backend gets an interactive native print dialog (Windows `PrintDlgEx`/COM, a full GTK/Qt dialog on Linux) — this matches the existing parity where only macOS has one, and building either is a much larger, separate scope.
