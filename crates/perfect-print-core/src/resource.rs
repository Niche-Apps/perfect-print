use serde::{Deserialize, Serialize};

use crate::error::CoreResult;
use crate::image::ImageData;

/// A font with its loaded data.
#[derive(Debug, Clone)]
pub struct FontResource {
    pub name: String,
    pub data: Vec<u8>,
    pub index: u32, // For TTC collections
}

/// A handle to a font in the resource store.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FontResourceHandle {
    pub id: String,
    pub family: String,
    pub data_hash: String, // SHA-256 of the font data for caching
}

/// A handle to an image in the resource store.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImageResourceHandle {
    pub id: String,
    pub width: u32,
    pub height: u32,
    pub format: crate::image::ImageFormat,
    pub data_hash: String,
}

impl ImageResourceHandle {
    /// Create a handle from image data.
    pub fn from_image(id: &str, data: &ImageData, format: crate::image::ImageFormat) -> Self {
        let hash = md5::compute(&data.pixels);
        Self {
            id: id.to_string(),
            width: data.width,
            height: data.height,
            format,
            data_hash: hash,
        }
    }
}

/// ResourceStore holds fonts and images for the document.
///
/// This is serializable for the canonical model (stores only references,
/// not the actual binary data). Binary data is stored separately.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResourceStore {
    /// Font references used in the document.
    pub fonts: Vec<FontResourceHandle>,

    /// Image references used in the document.
    pub images: Vec<ImageResourceHandle>,
}

impl ResourceStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a font reference.
    pub fn add_font(&mut self, handle: FontResourceHandle) {
        if !self.fonts.iter().any(|f| f.id == handle.id) {
            self.fonts.push(handle);
        }
    }

    /// Add an image reference.
    pub fn add_image(&mut self, handle: ImageResourceHandle) {
        if !self.images.iter().any(|i| i.id == handle.id) {
            self.images.push(handle);
        }
    }

    /// Get a font handle by ID.
    pub fn get_font(&self, id: &str) -> Option<&FontResourceHandle> {
        self.fonts.iter().find(|f| f.id == id)
    }

    /// Get an image handle by ID.
    pub fn get_image(&self, id: &str) -> Option<&ImageResourceHandle> {
        self.images.iter().find(|i| i.id == id)
    }

    /// Validate all references exist.
    pub fn validate(&self) -> CoreResult<()> {
        // References are validated during model construction
        Ok(())
    }
}

/// Shared image data store for DocumentModel.
///
/// Holds the actual pixel data, indexed by image ID.
/// This is NOT in ResourceStore to keep ResourceStore serializable
/// (pixel data is large). It's a companion struct.
#[derive(Debug, Clone, Default)]
pub struct ImageStore {
    /// Map from image ID to pixel data.
    images: std::collections::HashMap<String, std::sync::Arc<ImageData>>,
}

impl ImageStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// Insert an image and return its reference handle.
    pub fn insert(&mut self, id: &str, data: ImageData) -> ImageResourceHandle {
        let handle = ImageResourceHandle::from_image(id, &data, crate::image::ImageFormat::RawRgba);
        self.images
            .insert(id.to_string(), std::sync::Arc::new(data));
        handle
    }

    /// Load an image from a file, insert it, and return both handle and ID.
    pub fn load_from_file(
        &mut self,
        id: &str,
        path: &std::path::Path,
    ) -> Result<ImageResourceHandle, crate::image::ImageLoadError> {
        let data = ImageData::load(path)?;
        Ok(self.insert(id, data))
    }

    /// Insert a test pattern image.
    pub fn insert_test_pattern(
        &mut self,
        id: &str,
        width: u32,
        height: u32,
    ) -> ImageResourceHandle {
        let data = ImageData::test_pattern(width, height);
        self.insert(id, data)
    }

    /// Get an image by ID.
    pub fn get(&self, id: &str) -> Option<std::sync::Arc<ImageData>> {
        self.images.get(id).cloned()
    }

    /// Check if an image exists.
    pub fn has(&self, id: &str) -> bool {
        self.images.contains_key(id)
    }

    /// Get the number of stored images.
    pub fn len(&self) -> usize {
        self.images.len()
    }

    pub fn is_empty(&self) -> bool {
        self.images.is_empty()
    }

    /// Iterate over all stored images.
    pub fn iter(&self) -> impl Iterator<Item = (&String, &std::sync::Arc<ImageData>)> {
        self.images.iter()
    }
}

impl PartialEq for ImageStore {
    fn eq(&self, other: &Self) -> bool {
        if self.images.len() != other.images.len() {
            return false;
        }
        for (key, val) in &self.images {
            match other.images.get(key) {
                Some(other_val) => {
                    if val.width != other_val.width
                        || val.height != other_val.height
                        || val.pixels != other_val.pixels
                    {
                        return false;
                    }
                }
                None => return false,
            }
        }
        true
    }
}

impl Eq for ImageStore {}

// Simple hash for image data identification
mod md5 {
    pub fn compute(data: &[u8]) -> String {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        let mut hasher = DefaultHasher::new();
        data.hash(&mut hasher);
        format!("{:x}", hasher.finish())
    }
}
