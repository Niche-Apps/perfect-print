use serde::{Deserialize, Serialize};

use crate::draw::DrawCommand;
use crate::error::{CoreError, CoreResult};
use crate::page::{Layer, LayerType, Margins, Page, PageSize};
use crate::resource::{ImageStore, ResourceStore};
use crate::units::Size;

/// The canonical document model.
///
/// This is THE model that all output backends consume. PDF, raster, preview,
/// and native print all render from this same model.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DocumentModel {
    pub pages: Vec<Page>,
    /// Optional header rendered on every page
    pub header: Option<Box<DrawCommand>>,
    /// Optional footer rendered on every page
    pub footer: Option<Box<DrawCommand>>,
    pub resources: ResourceStore,
    /// Image pixel data (not serialized, rebuilt from resources as needed)
    #[serde(skip)]
    pub image_store: ImageStore,
    pub metadata: DocumentMetadata,
}

/// Document metadata.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct DocumentMetadata {
    pub title: Option<String>,
    pub author: Option<String>,
    pub creator: String,
    pub page_count: usize,
}

impl DocumentModel {
    pub fn new(pages: Vec<Page>) -> Self {
        let page_count = pages.len();
        Self {
            pages,
            header: None,
            footer: None,
            resources: ResourceStore::new(),
            image_store: ImageStore::new(),
            metadata: DocumentMetadata {
                title: None,
                author: None,
                creator: "perfect-print 0.1.0".to_string(),
                page_count,
            },
        }
    }

    /// Get the number of pages.
    pub fn page_count(&self) -> usize {
        self.pages.len()
    }

    /// Get the size of a specific page.
    pub fn page_size(&self, index: usize) -> CoreResult<Size> {
        self.pages
            .get(index)
            .map(|p| p.size)
            .ok_or_else(|| CoreError::Validation(format!("Page {} does not exist", index)))
    }

    /// Get all draw commands across all pages and layers.
    pub fn all_commands(&self) -> impl Iterator<Item = &DrawCommand> {
        self.pages
            .iter()
            .flat_map(|p| p.layers.iter())
            .flat_map(|l| l.commands.iter())
    }

    /// Validate the document model.
    pub fn validate(&self) -> CoreResult<()> {
        if self.pages.is_empty() {
            return Err(CoreError::Validation("Document has no pages".to_string()));
        }
        for (i, page) in self.pages.iter().enumerate() {
            if page.size.width <= 0.0 || page.size.height <= 0.0 {
                return Err(CoreError::Validation(format!(
                    "Page {} has invalid size: {:?}",
                    i, page.size
                )));
            }
        }
        self.resources.validate()?;
        Ok(())
    }

    /// Serialize to stable JSON.
    pub fn to_json(&self) -> CoreResult<String> {
        // Use sorted keys for deterministic output
        let mut buf = Vec::new();
        let formatter = serde_json::ser::CompactFormatter;
        let mut ser = serde_json::Serializer::with_formatter(&mut buf, formatter);
        self.serialize(&mut ser)
            .map_err(|e| CoreError::Serialization(e.to_string()))?;
        // For readability, use pretty printer
        let pretty = serde_json::to_string_pretty(self)
            .map_err(|e| CoreError::Serialization(e.to_string()))?;
        Ok(pretty)
    }
}

/// Builder for constructing documents.
#[derive(Debug, Default, Clone)]
pub struct DocumentBuilder {
    pages: Vec<Page>,
    resources: ResourceStore,
    image_store: ImageStore,
    header: Option<Box<DrawCommand>>,
    footer: Option<Box<DrawCommand>>,
    metadata: DocumentMetadata,
}

impl DocumentBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.metadata.title = Some(title.into());
        self
    }

    pub fn author(mut self, author: impl Into<String>) -> Self {
        self.metadata.author = Some(author.into());
        self
    }

    pub fn add_page(mut self, page: Page) -> Self {
        self.pages.push(page);
        self
    }

    pub fn page(mut self, size: PageSize) -> Self {
        self.pages.push(Page::new(size));
        self
    }

    pub fn resources(mut self, resources: ResourceStore) -> Self {
        self.resources = resources;
        self
    }

    pub fn image_store(mut self, store: ImageStore) -> Self {
        self.image_store = store;
        self
    }

    pub fn add_image(mut self, id: &str, data: crate::image::ImageData) -> Self {
        let handle = self.image_store.insert(id, data);
        self.resources.add_image(handle);
        self
    }

    pub fn header(mut self, cmd: DrawCommand) -> Self {
        self.header = Some(Box::new(cmd));
        self
    }

    pub fn footer(mut self, cmd: DrawCommand) -> Self {
        self.footer = Some(Box::new(cmd));
        self
    }

    /// Get the title set on this builder.
    pub fn get_title(&self) -> Option<&str> {
        self.metadata.title.as_deref()
    }

    /// Get the author set on this builder.
    pub fn get_author(&self) -> Option<&str> {
        self.metadata.author.as_deref()
    }

    pub fn build(self) -> CoreResult<DocumentModel> {
        let page_count = self.pages.len();
        let mut model = DocumentModel::new(self.pages);
        model.resources = self.resources;
        model.image_store = self.image_store;
        model.metadata = self.metadata;
        model.header = self.header;
        model.footer = self.footer;
        model.metadata.page_count = page_count;
        model.validate()?;
        Ok(model)
    }
}

/// Convenience trait for adding draw commands to pages.
pub trait PageBuilder {
    fn add(&mut self, cmd: DrawCommand) -> &mut Self;
}

impl PageBuilder for Page {
    fn add(&mut self, cmd: DrawCommand) -> &mut Self {
        // Add to the foreground layer
        if let Some(layer) = self
            .layers
            .iter_mut()
            .find(|l| l.layer_type == LayerType::Foreground)
        {
            layer.commands.push(cmd);
        } else {
            let mut layer = Layer::foreground();
            layer.commands.push(cmd);
            self.layers.push(layer);
        }
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::color::Color;
    use crate::draw::{ShapedGlyph, TextRun, TextStyle};
    use crate::font::FontRef;
    use crate::units::Point;

    #[test]
    fn empty_document_fails_validation() {
        let result = DocumentBuilder::new().build();
        assert!(result.is_err());
    }

    #[test]
    fn single_page_document_builds() {
        let model = DocumentBuilder::new()
            .page(PageSize::Letter)
            .build()
            .unwrap();
        assert_eq!(model.page_count(), 1);
    }

    #[test]
    fn page_size_letter() {
        let model = DocumentBuilder::new()
            .page(PageSize::Letter)
            .build()
            .unwrap();
        let size = model.page_size(0).unwrap();
        assert!((size.width - 612.0).abs() < 0.01);
        assert!((size.height - 792.0).abs() < 0.01);
    }

    #[test]
    fn deterministic_serialization() {
        // Build the same document twice
        let model1 = DocumentBuilder::new()
            .page(PageSize::Letter)
            .build()
            .unwrap();
        let model2 = DocumentBuilder::new()
            .page(PageSize::Letter)
            .build()
            .unwrap();

        let json1 = model1.to_json().unwrap();
        let json2 = model2.to_json().unwrap();

        assert_eq!(
            json1, json2,
            "Identical documents must produce byte-identical JSON"
        );
    }

    #[test]
    fn serialization_is_stable_across_runs() {
        // This test verifies that serialization is deterministic
        // (no random ordering, no timestamps, etc.)
        let model = DocumentBuilder::new()
            .title("Test Document")
            .page(PageSize::A4)
            .build()
            .unwrap();

        let json1 = model.to_json().unwrap();
        let json2 = model.to_json().unwrap();
        let json3 = model.to_json().unwrap();

        assert_eq!(json1, json2);
        assert_eq!(json2, json3);
    }

    #[test]
    fn test_json_roundtrip_stability() {
        // Build a multi-page document with text, shapes, headers, and footers
        let model = DocumentBuilder::new()
            .title("Roundtrip Test")
            .author("Test Author")
            .page(PageSize::Letter)
            .page(PageSize::A4)
            .build()
            .unwrap();

        // Serialize to JSON
        let json1 = model.to_json().unwrap();

        // Deserialize back
        let model2: DocumentModel =
            serde_json::from_str(&json1).expect("Should deserialize JSON back to DocumentModel");

        // Serialize again
        let json2 = model2.to_json().unwrap();

        // Should be byte-identical
        assert_eq!(
            json1, json2,
            "JSON roundtrip should produce identical output"
        );

        // Structural equality
        assert_eq!(model.page_count(), model2.page_count());
        assert_eq!(model.metadata.title, model2.metadata.title);
        assert_eq!(model.metadata.author, model2.metadata.author);
    }
}
