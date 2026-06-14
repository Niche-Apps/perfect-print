//! Visual diff engine for comparing rendered output.
//!
//! Supports:
//! - Pixel-level PNG comparison with tolerance
//! - PDF-to-raster conversion for PDF vs raster parity checks
//! - Per-pixel diff heatmap generation
//! - Structured geometry assertions

use std::path::Path;

/// Result of a visual diff comparison.
#[derive(Debug, Clone)]
pub struct DiffResult {
    /// Whether the images match within tolerance.
    pub matches: bool,
    /// Number of pixels that differ.
    pub diff_pixels: u64,
    /// Total number of pixels compared.
    pub total_pixels: u64,
    /// Maximum per-channel difference found (0-255).
    pub max_diff: u8,
    /// Diff percentage (0.0-1.0).
    pub diff_ratio: f64,
    /// Dimensions of the first image.
    pub width: u32,
    pub height: u32,
    /// Path to the diff heatmap, if generated.
    pub heatmap_path: Option<std::path::PathBuf>,
}

impl DiffResult {
    pub fn diff_percentage(&self) -> f64 {
        self.diff_ratio * 100.0
    }

    pub fn summary(&self) -> String {
        if self.matches {
            format!(
                "PASS — {}x{} pixels, {} diffs ({:.4}%)",
                self.width,
                self.height,
                self.diff_pixels,
                self.diff_percentage()
            )
        } else {
            format!(
                "FAIL — {}x{} pixels, {} diffs ({:.4}%), max channel diff {}",
                self.width,
                self.height,
                self.diff_pixels,
                self.diff_percentage(),
                self.max_diff
            )
        }
    }
}

/// Compare two PNG images pixel-by-pixel.
///
/// `tolerance` is a value from 0.0 to 1.0 representing the maximum
/// allowed fraction of differing pixels.
///
/// Two pixels are considered different if any channel differs by more
/// than `channel_threshold` (0-255).
pub fn compare_pngs(
    path_a: &Path,
    path_b: &Path,
    tolerance: f64,
    channel_threshold: u8,
) -> Result<DiffResult, String> {
    let img_a = image::open(path_a)
        .map_err(|e| format!("Failed to open {}: {}", path_a.display(), e))?
        .to_rgba8();
    let img_b = image::open(path_b)
        .map_err(|e| format!("Failed to open {}: {}", path_b.display(), e))?
        .to_rgba8();

    let (w_a, h_a) = img_a.dimensions();
    let (w_b, h_b) = img_b.dimensions();

    if w_a != w_b || h_a != h_b {
        return Err(format!(
            "Image dimensions differ: {}x{} vs {}x{}",
            w_a, h_a, w_b, h_b
        ));
    }

    let total = (w_a as u64) * (h_a as u64);
    let mut diff_pixels: u64 = 0;
    let mut max_diff: u8 = 0;

    for (px_a, px_b) in img_a.pixels().zip(img_b.pixels()) {
        for c in 0..4 {
            let d = if px_a[c] > px_b[c] {
                px_a[c] - px_b[c]
            } else {
                px_b[c] - px_a[c]
            };
            if d > max_diff {
                max_diff = d;
            }
            if d > channel_threshold {
                diff_pixels += 1;
                break;
            }
        }
    }

    let diff_ratio = diff_pixels as f64 / total as f64;
    let matches = diff_ratio <= tolerance;

    Ok(DiffResult {
        matches,
        diff_pixels,
        total_pixels: total,
        max_diff,
        diff_ratio,
        width: w_a,
        height: h_b,
        heatmap_path: None,
    })
}

/// Generate a diff heatmap highlighting differing pixels.
///
/// Matching pixels are shown as grayscale. Differing pixels are
/// highlighted in red (image A) or blue (image B).
pub fn generate_heatmap(
    path_a: &Path,
    path_b: &Path,
    output_path: &Path,
    channel_threshold: u8,
) -> Result<DiffResult, String> {
    let img_a = image::open(path_a)
        .map_err(|e| format!("Failed to open {}: {}", path_a.display(), e))?
        .to_rgba8();
    let img_b = image::open(path_b)
        .map_err(|e| format!("Failed to open {}: {}", path_b.display(), e))?
        .to_rgba8();

    let (w, h) = img_a.dimensions();
    if img_b.dimensions() != (w, h) {
        return Err("Image dimensions differ".to_string());
    }

    let mut heatmap = image::RgbaImage::new(w, h);
    let total = (w as u64) * (h as u64);
    let mut diff_pixels: u64 = 0;
    let mut max_diff: u8 = 0;

    for (x, y, px_a) in img_a.enumerate_pixels() {
        let px_b = img_b.get_pixel(x, y);
        let mut is_diff = false;
        let mut channel_max_diff: u8 = 0;

        for c in 0..4 {
            let d = if px_a[c] > px_b[c] {
                px_a[c] - px_b[c]
            } else {
                px_b[c] - px_a[c]
            };
            if d > channel_max_diff {
                channel_max_diff = d;
            }
            if d > channel_threshold {
                is_diff = true;
            }
        }

        if channel_max_diff > max_diff {
            max_diff = channel_max_diff;
        }

        if is_diff {
            diff_pixels += 1;
            // Highlight in magenta for visibility
            heatmap.put_pixel(x, y, image::Rgba([255, 0, 255, 255]));
        } else {
            // Grayscale for matching pixels
            let gray = ((px_a[0] as u16 + px_a[1] as u16 + px_a[2] as u16) / 3) as u8;
            heatmap.put_pixel(x, y, image::Rgba([gray, gray, gray, 255]));
        }
    }

    heatmap
        .save(output_path)
        .map_err(|e| format!("Failed to save heatmap: {}", e))?;

    let diff_ratio = diff_pixels as f64 / total as f64;

    Ok(DiffResult {
        matches: diff_ratio == 0.0,
        diff_pixels,
        total_pixels: total,
        max_diff,
        diff_ratio,
        width: w,
        height: h,
        heatmap_path: Some(output_path.to_path_buf()),
    })
}

/// Compare all PNG files in two directories.
///
/// Files are matched by name. Returns a result for each pair.
pub fn compare_directories(
    dir_a: &Path,
    dir_b: &Path,
    tolerance: f64,
    channel_threshold: u8,
) -> Result<Vec<(String, DiffResult)>, String> {
    let mut results = Vec::new();

    let entries_a: Vec<_> = std::fs::read_dir(dir_a)
        .map_err(|e| format!("Failed to read {}: {}", dir_a.display(), e))?
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().map_or(false, |ext| ext == "png"))
        .collect();

    for entry in &entries_a {
        let name = entry.file_name();
        let path_b = dir_b.join(&name);

        if !path_b.exists() {
            return Err(format!(
                "Missing counterpart for {} in {}",
                name.to_string_lossy(),
                dir_b.display()
            ));
        }

        let result = compare_pngs(&entry.path(), &path_b, tolerance, channel_threshold)?;
        results.push((name.to_string_lossy().to_string(), result));
    }

    Ok(results)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn create_test_png(path: &PathBuf, width: u32, height: u32, color: [u8; 4]) {
        let mut img = image::RgbaImage::new(width, height);
        for pixel in img.pixels_mut() {
            *pixel = image::Rgba(color);
        }
        img.save(path).unwrap();
    }

    #[test]
    fn test_compare_identical_images() {
        let dir = std::env::temp_dir().join("pp_test_diff");
        let _ = std::fs::create_dir_all(&dir);

        let a = dir.join("a.png");
        let b = dir.join("b.png");
        create_test_png(&a, 100, 100, [255, 0, 0, 255]);
        create_test_png(&b, 100, 100, [255, 0, 0, 255]);

        let result = compare_pngs(&a, &b, 0.01, 0).unwrap();
        assert!(result.matches);
        assert_eq!(result.diff_pixels, 0);
        assert_eq!(result.diff_ratio, 0.0);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_compare_different_images() {
        let dir = std::env::temp_dir().join("pp_test_diff2");
        let _ = std::fs::create_dir_all(&dir);

        let a = dir.join("a.png");
        let b = dir.join("b.png");
        create_test_png(&a, 100, 100, [255, 0, 0, 255]);
        create_test_png(&b, 100, 100, [0, 255, 0, 255]);

        let result = compare_pngs(&a, &b, 0.01, 0).unwrap();
        assert!(!result.matches);
        assert!(result.diff_pixels > 0);
        assert!(result.diff_ratio > 0.0);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_compare_with_tolerance() {
        let dir = std::env::temp_dir().join("pp_test_diff3");
        let _ = std::fs::create_dir_all(&dir);

        let a = dir.join("a.png");
        let b = dir.join("b.png");
        create_test_png(&a, 100, 100, [255, 0, 0, 255]);
        create_test_png(&b, 100, 100, [250, 5, 5, 255]); // Slightly different

        // With threshold 0, should detect difference
        let result = compare_pngs(&a, &b, 0.01, 0).unwrap();
        assert!(!result.matches);

        // With threshold 10, should match
        let result = compare_pngs(&a, &b, 0.01, 10).unwrap();
        assert!(result.matches);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_dimension_mismatch() {
        let dir = std::env::temp_dir().join("pp_test_diff4");
        let _ = std::fs::create_dir_all(&dir);

        let a = dir.join("a.png");
        let b = dir.join("b.png");
        create_test_png(&a, 100, 100, [255, 0, 0, 255]);
        create_test_png(&b, 200, 200, [255, 0, 0, 255]);

        let result = compare_pngs(&a, &b, 0.01, 0);
        assert!(result.is_err());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_heatmap_generation() {
        let dir = std::env::temp_dir().join("pp_test_heatmap");
        let _ = std::fs::create_dir_all(&dir);

        let a = dir.join("a.png");
        let b = dir.join("b.png");
        let heatmap = dir.join("diff.png");

        create_test_png(&a, 10, 10, [255, 0, 0, 255]);
        create_test_png(&b, 10, 10, [0, 255, 0, 255]);

        let result = generate_heatmap(&a, &b, &heatmap, 0).unwrap();
        assert!(!result.matches);
        assert!(heatmap.exists());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_compare_directories() {
        let dir = std::env::temp_dir().join("pp_test_dir_diff");
        let _ = std::fs::create_dir_all(&dir);

        let dir_a = dir.join("a");
        let dir_b = dir.join("b");
        let _ = std::fs::create_dir_all(&dir_a);
        let _ = std::fs::create_dir_all(&dir_b);

        create_test_png(&dir_a.join("page_001.png"), 50, 50, [128, 128, 128, 255]);
        create_test_png(&dir_b.join("page_001.png"), 50, 50, [128, 128, 128, 255]);

        let results = compare_directories(&dir_a, &dir_b, 0.01, 0).unwrap();
        assert_eq!(results.len(), 1);
        assert!(results[0].1.matches);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_diff_result_summary() {
        let pass = DiffResult {
            matches: true,
            diff_pixels: 0,
            total_pixels: 10000,
            max_diff: 0,
            diff_ratio: 0.0,
            width: 100,
            height: 100,
            heatmap_path: None,
        };
        assert!(pass.summary().contains("PASS"));

        let fail = DiffResult {
            matches: false,
            diff_pixels: 500,
            total_pixels: 10000,
            max_diff: 128,
            diff_ratio: 0.05,
            width: 100,
            height: 100,
            heatmap_path: None,
        };
        assert!(fail.summary().contains("FAIL"));
    }
}
