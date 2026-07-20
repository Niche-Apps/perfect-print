use crate::{HtmlRenderError, HtmlRenderStage};
use std::path::{Path, PathBuf};
use url::Url;

const DEFAULT_MAX_HTML_BYTES: usize = 8 * 1024 * 1024;
const DEFAULT_MAX_RESOURCE_BYTES: usize = 32 * 1024 * 1024;
const DEFAULT_MAX_PDF_BYTES: usize = 128 * 1024 * 1024;

#[derive(Debug, Clone)]
pub struct ResourcePolicy {
    scripts_enabled: bool,
    network_enabled: bool,
    local_base_directory: Option<PathBuf>,
    max_html_bytes: usize,
    max_resource_bytes: usize,
    max_pdf_bytes: usize,
}

impl ResourcePolicy {
    pub fn offline() -> Self {
        Self {
            scripts_enabled: false,
            network_enabled: false,
            local_base_directory: None,
            max_html_bytes: DEFAULT_MAX_HTML_BYTES,
            max_resource_bytes: DEFAULT_MAX_RESOURCE_BYTES,
            max_pdf_bytes: DEFAULT_MAX_PDF_BYTES,
        }
    }

    pub fn scripts_enabled(&self) -> bool {
        self.scripts_enabled
    }

    pub fn with_max_html_bytes(mut self, bytes: usize) -> Self {
        self.max_html_bytes = bytes;
        self
    }

    pub fn with_max_resource_bytes(mut self, bytes: usize) -> Self {
        self.max_resource_bytes = bytes;
        self
    }

    pub fn with_max_pdf_bytes(mut self, bytes: usize) -> Self {
        self.max_pdf_bytes = bytes;
        self
    }

    pub fn with_local_base_directory(
        mut self,
        directory: impl AsRef<Path>,
    ) -> Result<Self, HtmlRenderError> {
        let canonical = directory.as_ref().canonicalize().map_err(|error| {
            HtmlRenderError::at_stage(
                "HTML_BASE_URL_INVALID",
                HtmlRenderStage::Validate,
                format!("local base directory is unavailable: {error}"),
            )
        })?;
        if !canonical.is_dir() {
            return Err(HtmlRenderError::at_stage(
                "HTML_BASE_URL_INVALID",
                HtmlRenderStage::Validate,
                "local base path is not a directory",
            ));
        }
        self.local_base_directory = Some(canonical);
        Ok(self)
    }

    pub fn allows_url(&self, value: &str) -> Result<bool, HtmlRenderError> {
        let url = Url::parse(value).map_err(|error| {
            HtmlRenderError::at_stage(
                "HTML_RESOURCE_URL_INVALID",
                HtmlRenderStage::Validate,
                format!("resource URL is invalid: {error}"),
            )
        })?;
        match url.scheme() {
            "data" => Ok(true),
            "about" => Ok(value.eq_ignore_ascii_case("about:blank")),
            "http" | "https" => Ok(self.network_enabled),
            "file" => url
                .to_file_path()
                .map_err(|_| {
                    HtmlRenderError::at_stage(
                        "HTML_RESOURCE_URL_INVALID",
                        HtmlRenderStage::Validate,
                        "file URL cannot be converted to a local path",
                    )
                })
                .and_then(|path| self.allows_local_path(path)),
            _ => Ok(false),
        }
    }

    pub fn allows_local_path(&self, path: impl AsRef<Path>) -> Result<bool, HtmlRenderError> {
        let Some(base) = &self.local_base_directory else {
            return Ok(false);
        };
        let canonical = match path.as_ref().canonicalize() {
            Ok(path) => path,
            Err(_) => return Ok(false),
        };
        Ok(canonical.starts_with(base))
    }

    pub fn validate_html_bytes(&self, bytes: usize) -> Result<(), HtmlRenderError> {
        if bytes > self.max_html_bytes {
            return Err(HtmlRenderError::at_stage(
                "HTML_INPUT_TOO_LARGE",
                HtmlRenderStage::Validate,
                format!("HTML input exceeds {} bytes", self.max_html_bytes),
            ));
        }
        Ok(())
    }

    pub fn validate_resource_bytes(&self, bytes: usize) -> Result<(), HtmlRenderError> {
        if bytes > self.max_resource_bytes {
            return Err(HtmlRenderError::at_stage(
                "HTML_RESOURCES_TOO_LARGE",
                HtmlRenderStage::Validate,
                format!("decoded resources exceed {} bytes", self.max_resource_bytes),
            ));
        }
        Ok(())
    }

    pub fn validate_pdf_bytes(&self, bytes: usize) -> Result<(), HtmlRenderError> {
        if bytes > self.max_pdf_bytes {
            return Err(HtmlRenderError::at_stage(
                "PDF_OUTPUT_TOO_LARGE",
                HtmlRenderStage::ValidatePdf,
                format!("rendered PDF exceeds {} bytes", self.max_pdf_bytes),
            ));
        }
        Ok(())
    }
}

impl Default for ResourcePolicy {
    fn default() -> Self {
        Self::offline()
    }
}
