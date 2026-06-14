# ADR-0002: Print Backend Architecture

## Status: Accepted

## Context

perfect-print needs to support native printing across macOS, Windows, and Linux. Each platform has different printing APIs:

- **macOS**: `NSPrintPanel` + `NSPrintOperation` (Objective-C), or `lp`/`lpstat` CLI
- **Windows**: `winspool` API, `PrintDocument` API, or XPS
- **Linux**: CUPS (IPP protocol)

The core document model and rendering pipeline are platform-independent. Only the final "send to printer" step is platform-specific.

## Decision

Use a trait-based backend architecture:

1. **`PrintDialog` trait** in `perfect-print-dialog` defines the interface
2. **Platform backend crates** implement the trait
3. **CLI and applications** depend on the trait, not concrete backends

```rust
pub trait PrintDialog {
    fn show_print_dialog(&self, settings: &PrintSettings, title: Option<&str>)
        -> Result<PrintSettings, PrintError>;
    fn show_page_setup(&self, settings: &PrintSettings)
        -> Result<PrintSettings, PrintError>;
    fn available_printers(&self) -> Result<Vec<Printer>, PrintError>;
    fn default_printer(&self) -> Result<Printer, PrintError>;
}
```

### Backend Crates

| Crate | Platform | Implementation |
|-------|----------|----------------|
| `perfect-print-backend-macos` | macOS | `lpstat`/`lp`/`cancel` CLI bridge |
| `perfect-print-backend-windows` | Windows | Stub (planned: `winspool`) |
| `perfect-print-backend-linux` | Linux | Stub (planned: CUPS/IPP) |

### macOS Implementation Details

The macOS backend uses command-line tools instead of Objective-C FFI:

- **Pros**: No `objc2` type system issues, works in CI, simple to debug
- **Cons**: No native print dialog UI, limited settings exposure

The `lp` command accepts all standard print settings via flags. Job tracking uses `lpstat -o` and `cancel`.

## Consequences

- **Positive**: Core crate is fully platform-independent and testable
- **Positive**: Backends can be developed independently
- **Positive**: Applications can swap backends or provide custom implementations
- **Negative**: macOS backend lacks native print dialog (acceptable for CLI/server use)
- **Negative**: Windows/Linux backends are stubs (native printing not yet available)

## Alternatives Considered

1. **Direct FFI for each platform**: Rejected due to complexity (especially `objc2` type incompatibilities discovered during development)
2. **Single crate with `cfg` flags**: Rejected — separates concerns poorly, harder to test
3. **External print command only**: Rejected — no job tracking, limited error handling
