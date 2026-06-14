# ADR-0003: Print Job Status Tracking

## Status: Accepted

## Context

The initial `submit_print_job()` returned `()` — fire and forget. Users need to:
- Know if a print job was assigned an ID
- Check if a job has completed
- Cancel a queued job
- List all pending jobs

## Decision

Change `submit_print_job()` to return `Option<String>` (the job ID), and add three new methods to `MacosPrintDialog`:

```rust
/// Submit a print job, returning the job ID if parseable
pub fn submit_print_job(&self, pdf_path: &Path, settings: &PrintSettings)
    -> PrintDialogResult<Option<String>>;

/// Check if a job has completed (true = done, false = still in queue)
pub fn poll_job_status(&self, job_id: &str) -> PrintDialogResult<bool>;

/// List all pending jobs as (job_id, printer, status)
pub fn list_jobs(&self) -> PrintDialogResult<Vec<(String, String, String)>>;

/// Cancel a queued job
pub fn cancel_job(&self, job_id: &str) -> PrintDialogResult<()>;
```

### Job ID Parsing

The `lp` command outputs: `request id is PrinterName-42 (1 file(s))`

The `parse_job_id()` method extracts `PrinterName-42` using string parsing.

### Job Status via `lpstat -o`

`lpstat -o` lists all pending jobs. If a job ID appears in the output, it's still in queue. If not found, it's assumed completed (or never existed).

### Job Cancellation via `cancel`

The `cancel` command removes a job from the queue. Returns an error if the job doesn't exist or can't be cancelled.

### CLI Commands

Three new CLI commands were added:

```bash
# List pending jobs
cargo run -p perfect-print-cli -- jobs

# Check job status
cargo run -p perfect-print-cli -- job-status PrinterName-42

# Cancel a job
cargo run -p perfect-print-cli -- cancel-job PrinterName-42
```

## Consequences

- **Positive**: Full job lifecycle management (submit → track → cancel)
- **Positive**: Job ID is optional — if parsing fails, printing still works
- **Positive**: CLI provides complete job management
- **Negative**: `lpstat -o` format may vary across macOS versions (currently assumes standard format)
- **Negative**: "Not found" in `lpstat -o` could mean completed OR cancelled — we assume completed

## Alternatives Considered

1. **IPP protocol for job tracking**: More robust but requires additional dependencies. CLI bridge is simpler.
2. **`cups-sys` for cross-platform**: Would only help Linux. macOS already has CLI tools.
3. **Polling loop in CLI**: Rejected — CLI should be one-shot. Polling loops belong in applications.
