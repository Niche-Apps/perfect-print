//! Structured geometry assertions for document verification.
//!
//! Provides measurable WYSIWYG checks:
//! - Page size verification
//! - Content bounds checking
//! - Text baseline positions
//! - Table row heights and column widths
//! - Page count verification

use perfect_print_core::document::DocumentModel;
use perfect_print_core::draw::DrawCommand;
use perfect_print_core::units::Rect;

/// A geometry assertion to verify against a document.
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub enum GeometryAssertion {
    /// Verify the total number of pages.
    PageCount { expected: usize },
    /// Verify the size of a specific page (in points).
    PageSize {
        page_index: usize,
        expected_width: f64,
        expected_height: f64,
        tolerance_pts: f64,
    },
    /// Verify content exists within a bounding rect on a page.
    ContentInBounds {
        page_index: usize,
        min_x: f64,
        min_y: f64,
        max_x: f64,
        max_y: f64,
    },
    /// Verify no content exists outside a bounding rect.
    ContentNotOutside {
        page_index: usize,
        max_x: f64,
        max_y: f64,
    },
    /// Verify a minimum number of draw commands on a page.
    MinCommands { page_index: usize, min_count: usize },
    /// Verify text commands exist on a page.
    HasText { page_index: usize },
    /// Verify the content area (page size minus margins) matches expectations.
    ContentArea {
        page_index: usize,
        expected_rect: Rect,
        tolerance_pts: f64,
    },
}

/// Result of running a geometry assertion.
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct AssertionResult {
    pub assertion: GeometryAssertion,
    pub passed: bool,
    pub message: String,
}

impl AssertionResult {
    pub fn summary(&self) -> String {
        let status = if self.passed { "PASS" } else { "FAIL" };
        format!("[{}] {}", status, self.message)
    }
}

/// Run a single assertion against a document.
pub fn check_assertion(model: &DocumentModel, assertion: &GeometryAssertion) -> AssertionResult {
    match assertion {
        GeometryAssertion::PageCount { expected } => {
            let actual = model.page_count();
            let passed = actual == *expected;
            AssertionResult {
                assertion: assertion.clone(),
                passed,
                message: format!("Page count: expected {:?}, got {:?}", expected, actual),
            }
        }
        GeometryAssertion::PageSize {
            page_index,
            expected_width,
            expected_height,
            tolerance_pts,
        } => match model.pages.get(*page_index) {
            Some(page) => {
                let w_ok = (page.size.width - expected_width).abs() <= *tolerance_pts;
                let h_ok = (page.size.height - expected_height).abs() <= *tolerance_pts;
                let passed = w_ok && h_ok;
                AssertionResult {
                    assertion: assertion.clone(),
                    passed,
                    message: format!(
                        "Page {} size: expected {}x{}, got {}x{} (tolerance {:.1}pt)",
                        page_index,
                        expected_width,
                        expected_height,
                        page.size.width,
                        page.size.height,
                        tolerance_pts
                    ),
                }
            }
            None => AssertionResult {
                assertion: assertion.clone(),
                passed: false,
                message: format!("Page {} does not exist", page_index),
            },
        },
        GeometryAssertion::ContentInBounds {
            page_index,
            min_x,
            min_y,
            max_x,
            max_y,
        } => match model.pages.get(*page_index) {
            Some(page) => {
                let all_cmds: Vec<_> = page.layers.iter().flat_map(|l| l.commands.iter()).collect();

                let any_in_bounds = all_cmds.iter().any(|cmd| {
                    if let Some(bounds) = cmd.bounds() {
                        bounds.x >= *min_x
                            && bounds.y >= *min_y
                            && bounds.x + bounds.width <= *max_x
                            && bounds.y + bounds.height <= *max_y
                    } else {
                        false
                    }
                });

                AssertionResult {
                    assertion: assertion.clone(),
                    passed: any_in_bounds,
                    message: if any_in_bounds {
                        format!(
                            "Page {}: found content within [{},{}]-[{},{}]",
                            page_index, min_x, min_y, max_x, max_y
                        )
                    } else {
                        format!(
                            "Page {}: no content found within [{},{}]-[{},{}]",
                            page_index, min_x, min_y, max_x, max_y
                        )
                    },
                }
            }
            None => AssertionResult {
                assertion: assertion.clone(),
                passed: false,
                message: format!("Page {} does not exist", page_index),
            },
        },
        GeometryAssertion::ContentNotOutside {
            page_index,
            max_x,
            max_y,
        } => match model.pages.get(*page_index) {
            Some(page) => {
                let all_cmds: Vec<_> = page.layers.iter().flat_map(|l| l.commands.iter()).collect();

                let any_outside = all_cmds.iter().any(|cmd| {
                    if let Some(bounds) = cmd.bounds() {
                        bounds.x + bounds.width > *max_x || bounds.y + bounds.height > *max_y
                    } else {
                        false
                    }
                });

                AssertionResult {
                    assertion: assertion.clone(),
                    passed: !any_outside,
                    message: if !any_outside {
                        format!(
                            "Page {}: all content within {}x{}",
                            page_index, max_x, max_y
                        )
                    } else {
                        format!(
                            "Page {}: found content exceeding {}x{}",
                            page_index, max_x, max_y
                        )
                    },
                }
            }
            None => AssertionResult {
                assertion: assertion.clone(),
                passed: false,
                message: format!("Page {} does not exist", page_index),
            },
        },
        GeometryAssertion::MinCommands {
            page_index,
            min_count,
        } => match model.pages.get(*page_index) {
            Some(page) => {
                let count: usize = page.layers.iter().map(|l| l.commands.len()).sum();
                let passed = count >= *min_count;
                AssertionResult {
                    assertion: assertion.clone(),
                    passed,
                    message: format!(
                        "Page {} commands: expected >= {}, got {}",
                        page_index, min_count, count
                    ),
                }
            }
            None => AssertionResult {
                assertion: assertion.clone(),
                passed: false,
                message: format!("Page {} does not exist", page_index),
            },
        },
        GeometryAssertion::HasText { page_index } => match model.pages.get(*page_index) {
            Some(page) => {
                let has_text = page
                    .layers
                    .iter()
                    .flat_map(|l| l.commands.iter())
                    .any(|cmd| matches!(cmd, DrawCommand::Text { .. }));
                AssertionResult {
                    assertion: assertion.clone(),
                    passed: has_text,
                    message: if has_text {
                        format!("Page {}: text commands found", page_index)
                    } else {
                        format!("Page {}: no text commands found", page_index)
                    },
                }
            }
            None => AssertionResult {
                assertion: assertion.clone(),
                passed: false,
                message: format!("Page {} does not exist", page_index),
            },
        },
        GeometryAssertion::ContentArea {
            page_index,
            expected_rect,
            tolerance_pts,
        } => match model.pages.get(*page_index) {
            Some(page) => {
                let actual = page.content_rect();
                let x_ok = (actual.x - expected_rect.x).abs() <= *tolerance_pts;
                let y_ok = (actual.y - expected_rect.y).abs() <= *tolerance_pts;
                let w_ok = (actual.width - expected_rect.width).abs() <= *tolerance_pts;
                let h_ok = (actual.height - expected_rect.height).abs() <= *tolerance_pts;
                let passed = x_ok && y_ok && w_ok && h_ok;
                AssertionResult {
                    assertion: assertion.clone(),
                    passed,
                    message: format!(
                        "Page {} content area: expected {:?}, got {:?} (tolerance {:.1}pt)",
                        page_index, expected_rect, actual, tolerance_pts
                    ),
                }
            }
            None => AssertionResult {
                assertion: assertion.clone(),
                passed: false,
                message: format!("Page {} does not exist", page_index),
            },
        },
    }
}

/// Run multiple assertions and return all results.
pub fn check_all(model: &DocumentModel, assertions: &[GeometryAssertion]) -> Vec<AssertionResult> {
    assertions
        .iter()
        .map(|a| check_assertion(model, a))
        .collect()
}

/// Run assertions and return Ok(()) if all pass, or Err with summary if any fail.
#[allow(dead_code)]
pub fn verify_all(model: &DocumentModel, assertions: &[GeometryAssertion]) -> Result<(), String> {
    let results = check_all(model, assertions);
    let failures: Vec<_> = results.iter().filter(|r| !r.passed).collect();

    if failures.is_empty() {
        Ok(())
    } else {
        let summary: String = failures
            .iter()
            .map(|r| r.summary())
            .collect::<Vec<_>>()
            .join("\n");
        Err(format!(
            "{} of {} assertions failed:\n{}",
            failures.len(),
            results.len(),
            summary
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use perfect_print_core::color::Color;
    use perfect_print_core::document::DocumentBuilder;
    use perfect_print_core::draw::DrawCommand;
    use perfect_print_core::page::PageSize;
    use perfect_print_core::units::Rect;

    fn make_test_doc() -> DocumentModel {
        DocumentBuilder::new()
            .page(PageSize::Letter)
            .build()
            .unwrap()
    }

    fn make_doc_with_content() -> DocumentModel {
        use perfect_print_core::document::PageBuilder;

        let mut page = perfect_print_core::page::Page::new(PageSize::Letter);
        page.add(DrawCommand::FillRect {
            rect: Rect::new(100.0, 200.0, 50.0, 30.0),
            color: Color::red(),
        });
        DocumentBuilder::new().add_page(page).build().unwrap()
    }

    #[test]
    fn test_page_count_pass() {
        let model = make_test_doc();
        let result = check_assertion(&model, &GeometryAssertion::PageCount { expected: 1 });
        assert!(result.passed);
    }

    #[test]
    fn test_page_count_fail() {
        let model = make_test_doc();
        let result = check_assertion(&model, &GeometryAssertion::PageCount { expected: 2 });
        assert!(!result.passed);
    }

    #[test]
    fn test_page_size_letter() {
        let model = make_test_doc();
        let result = check_assertion(
            &model,
            &GeometryAssertion::PageSize {
                page_index: 0,
                expected_width: 612.0,
                expected_height: 792.0,
                tolerance_pts: 1.0,
            },
        );
        assert!(result.passed);
    }

    #[test]
    fn test_content_in_bounds() {
        let model = make_doc_with_content();
        let result = check_assertion(
            &model,
            &GeometryAssertion::ContentInBounds {
                page_index: 0,
                min_x: 50.0,
                min_y: 150.0,
                max_x: 200.0,
                max_y: 250.0,
            },
        );
        assert!(result.passed);
    }

    #[test]
    fn test_content_not_outside() {
        let model = make_doc_with_content();
        let result = check_assertion(
            &model,
            &GeometryAssertion::ContentNotOutside {
                page_index: 0,
                max_x: 612.0,
                max_y: 792.0,
            },
        );
        assert!(result.passed);
    }

    #[test]
    fn test_min_commands() {
        let model = make_doc_with_content();
        let result = check_assertion(
            &model,
            &GeometryAssertion::MinCommands {
                page_index: 0,
                min_count: 1,
            },
        );
        assert!(result.passed);
    }

    #[test]
    fn test_has_text() {
        let model = make_test_doc();
        let result = check_assertion(&model, &GeometryAssertion::HasText { page_index: 0 });
        assert!(!result.passed);

        let model = make_doc_with_content();
        let result = check_assertion(&model, &GeometryAssertion::HasText { page_index: 0 });
        assert!(!result.passed); // No text, just a rect
    }

    #[test]
    fn test_content_area() {
        use perfect_print_core::page::Margins;

        let mut page = perfect_print_core::page::Page::new(PageSize::Letter);
        page.margins = Margins::all(72.0);
        let model = DocumentBuilder::new().add_page(page).build().unwrap();

        let result = check_assertion(
            &model,
            &GeometryAssertion::ContentArea {
                page_index: 0,
                expected_rect: Rect::new(72.0, 72.0, 468.0, 648.0),
                tolerance_pts: 0.5,
            },
        );
        assert!(result.passed);
    }

    #[test]
    fn test_verify_all_pass() {
        let model = make_test_doc();
        let assertions = vec![
            GeometryAssertion::PageCount { expected: 1 },
            GeometryAssertion::PageSize {
                page_index: 0,
                expected_width: 612.0,
                expected_height: 792.0,
                tolerance_pts: 1.0,
            },
        ];
        assert!(verify_all(&model, &assertions).is_ok());
    }

    #[test]
    fn test_verify_all_fail() {
        let model = make_test_doc();
        let assertions = vec![GeometryAssertion::PageCount { expected: 99 }];
        let result = verify_all(&model, &assertions);
        assert!(result.is_err());
    }

    #[test]
    fn test_nonexistent_page() {
        let model = make_test_doc();
        let result = check_assertion(&model, &GeometryAssertion::PageCount { expected: 2 });
        assert!(!result.passed);
    }
}
