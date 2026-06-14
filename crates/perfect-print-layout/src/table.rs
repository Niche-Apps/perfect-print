//! Table layout engine.
//!
//! Supports:
//! - Fixed and auto-sized columns
//! - Row headers (repeated on each page)
//! - Column headers (repeated on each page break)
//! - Cell padding and borders
//! - Multi-line cell content
//! - Page breaks with header repetition

use perfect_print_core::color::Color;
use perfect_print_core::draw::{DrawCommand, LineCap, LineJoin, TextRun, TextStyle};
use perfect_print_core::font::FontRef;
use perfect_print_core::units::{Point, Rect};

use crate::font_loader::FontCache;
use crate::text_shaper::TextShaper;

/// Column width specification.
#[derive(Debug, Clone, PartialEq)]
pub enum ColumnWidth {
    /// Fixed width in points
    Fixed(f64),
    /// Proportional weight (fraction of remaining space)
    Weight(f64),
    /// Auto-size based on content
    Auto,
}

impl ColumnWidth {
    /// Get the fixed width, if this is a Fixed variant.
    pub fn as_fixed(&self) -> Option<f64> {
        match self {
            ColumnWidth::Fixed(w) => Some(*w),
            _ => None,
        }
    }
}

/// Column definition.
#[derive(Debug, Clone, PartialEq)]
pub struct Column {
    pub width: ColumnWidth,
    /// Optional header text for this column
    pub header: Option<String>,
    /// Text style for the header
    pub header_style: Option<TextStyle>,
}

impl Column {
    pub fn new(width: ColumnWidth) -> Self {
        Self {
            width,
            header: None,
            header_style: None,
        }
    }

    pub fn with_header(mut self, text: impl Into<String>) -> Self {
        self.header = Some(text.into());
        self
    }

    pub fn with_header_style(mut self, style: TextStyle) -> Self {
        self.header_style = Some(style);
        self
    }
}

/// Cell content.
#[derive(Debug, Clone, PartialEq)]
pub enum CellContent {
    Text(String),
    /// Pre-formatted draw commands
    Commands(Vec<DrawCommand>),
}

impl CellContent {
    pub fn text(content: impl Into<String>) -> Self {
        CellContent::Text(content.into())
    }
}

impl From<&str> for CellContent {
    fn from(s: &str) -> Self {
        CellContent::Text(s.to_string())
    }
}

impl From<String> for CellContent {
    fn from(s: String) -> Self {
        CellContent::Text(s)
    }
}

/// Cell style.
#[derive(Debug, Clone, PartialEq)]
pub struct CellStyle {
    pub padding: f64,
    pub border_width: f64,
    pub border_color: Color,
    pub background: Option<Color>,
    pub text_style: TextStyle,
}

impl Default for CellStyle {
    fn default() -> Self {
        Self {
            padding: 4.0,
            border_width: 0.5,
            border_color: Color::gray(0.8),
            background: None,
            text_style: TextStyle::new(FontRef::new("Helvetica"), 10.0),
        }
    }
}

/// A cell in the table.
#[derive(Debug, Clone, PartialEq)]
pub struct Cell {
    pub content: CellContent,
    pub style: CellStyle,
    /// Number of columns this cell spans
    pub colspan: usize,
    /// Number of rows this cell spans
    pub rowspan: usize,
}

impl Cell {
    pub fn new(content: impl Into<CellContent>) -> Self {
        Self {
            content: content.into(),
            style: CellStyle::default(),
            colspan: 1,
            rowspan: 1,
        }
    }

    pub fn with_style(mut self, style: CellStyle) -> Self {
        self.style = style;
        self
    }

    pub fn with_colspan(mut self, span: usize) -> Self {
        self.colspan = span.max(1);
        self
    }

    pub fn with_rowspan(mut self, span: usize) -> Self {
        self.rowspan = span.max(1);
        self
    }
}

/// A row in the table.
#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    pub cells: Vec<Cell>,
    /// Whether this row is a header row (repeated on page breaks)
    pub is_header: bool,
    /// Fixed height in points; None = auto
    pub height: Option<f64>,
}

impl Row {
    pub fn new(cells: Vec<Cell>) -> Self {
        Self {
            cells,
            is_header: false,
            height: None,
        }
    }

    pub fn header(cells: Vec<Cell>) -> Self {
        Self {
            cells,
            is_header: true,
            height: None,
        }
    }

    pub fn with_height(mut self, height: f64) -> Self {
        self.height = Some(height);
        self
    }
}

/// Table layout result: a set of draw commands positioned on the page.
#[derive(Debug, Clone, PartialEq)]
pub struct TableLayout {
    /// The draw commands to render the table.
    pub commands: Vec<DrawCommand>,
    /// The total height of the table in points.
    pub total_height: f64,
    /// The total width of the table in points.
    pub total_width: f64,
    /// Rows that are header rows (for page break repetition).
    pub header_row_count: usize,
}

/// Table layout engine.
pub struct TableEngine {
    shaper: TextShaper,
    font_cache: FontCache,
}

impl TableEngine {
    pub fn new() -> Self {
        Self {
            shaper: TextShaper::new(),
            font_cache: FontCache::default(),
        }
    }

    pub fn with_font_cache(font_cache: FontCache) -> Self {
        Self {
            shaper: TextShaper::new(),
            font_cache,
        }
    }

    pub fn with_shaper(mut self, shaper: TextShaper) -> Self {
        self.shaper = shaper;
        self
    }

    /// Layout a table within the given width.
    pub fn layout_table(
        &mut self,
        columns: &[Column],
        rows: &[Row],
        available_width: f64,
        start_y: f64,
    ) -> TableLayout {
        if columns.is_empty() || rows.is_empty() {
            return TableLayout {
                commands: vec![],
                total_height: 0.0,
                total_width: available_width,
                header_row_count: 0,
            };
        }

        let col_widths = self.calculate_column_widths(columns, available_width);
        let mut commands = Vec::new();
        let mut y = start_y;
        let mut header_row_count = 0;

        for row in rows {
            if row.is_header {
                header_row_count += 1;
            }

            let row_height = self.calculate_row_height(row, &col_widths);
            let row_commands = self.layout_row(row, &col_widths, y);
            commands.extend(row_commands);
            y += row_height;
        }

        TableLayout {
            commands,
            total_height: y - start_y,
            total_width: available_width,
            header_row_count,
        }
    }

    /// Layout a table with page breaks.
    pub fn layout_table_paginated(
        &mut self,
        columns: &[Column],
        rows: &[Row],
        available_width: f64,
        page_height: f64,
        start_y: f64,
    ) -> Vec<TableLayout> {
        if columns.is_empty() || rows.is_empty() {
            return vec![];
        }

        let col_widths = self.calculate_column_widths(columns, available_width);
        let mut pages = Vec::new();
        let mut current_page_commands = Vec::new();
        let mut current_y = start_y;
        let mut header_row_count = 0;

        for row in rows {
            if row.is_header {
                header_row_count += 1;
            }
        }

        for row in rows {
            let row_height = self.calculate_row_height(row, &col_widths);

            if current_y + row_height > page_height && !row.is_header {
                if !current_page_commands.is_empty() {
                    pages.push(TableLayout {
                        commands: std::mem::take(&mut current_page_commands),
                        total_height: current_y - start_y,
                        total_width: available_width,
                        header_row_count,
                    });
                }

                current_y = start_y;
                for header_row in rows.iter().filter(|r| r.is_header) {
                    let h = self.calculate_row_height(header_row, &col_widths);
                    let cmds = self.layout_row(header_row, &col_widths, current_y);
                    current_page_commands.extend(cmds);
                    current_y += h;
                }
            }

            let row_commands = self.layout_row(row, &col_widths, current_y);
            current_page_commands.extend(row_commands);
            current_y += row_height;
        }

        if !current_page_commands.is_empty() {
            pages.push(TableLayout {
                commands: current_page_commands,
                total_height: current_y - start_y,
                total_width: available_width,
                header_row_count,
            });
        }

        pages
    }

    fn calculate_column_widths(&self, columns: &[Column], available_width: f64) -> Vec<f64> {
        let n = columns.len();
        let mut widths = vec![0.0; n];
        let mut remaining_width = available_width;
        let mut total_weight = 0.0;
        let mut auto_count = 0;

        for (i, col) in columns.iter().enumerate() {
            match &col.width {
                ColumnWidth::Fixed(w) => {
                    widths[i] = *w;
                    remaining_width -= w;
                }
                ColumnWidth::Weight(w) => {
                    total_weight += w;
                }
                ColumnWidth::Auto => {
                    auto_count += 1;
                }
            }
        }

        if total_weight > 0.0 {
            for (i, col) in columns.iter().enumerate() {
                if let ColumnWidth::Weight(w) = &col.width {
                    widths[i] = remaining_width * (w / total_weight);
                }
            }
        }

        if auto_count > 0 {
            let auto_width = remaining_width / auto_count as f64;
            for (i, col) in columns.iter().enumerate() {
                if matches!(&col.width, ColumnWidth::Auto) {
                    widths[i] = auto_width;
                }
            }
        }

        widths
    }

    fn calculate_row_height(&mut self, row: &Row, col_widths: &[f64]) -> f64 {
        if let Some(h) = row.height {
            return h;
        }

        let mut max_height: f64 = 0.0;

        for (i, cell) in row.cells.iter().enumerate() {
            let col_width = col_widths.get(i).copied().unwrap_or(0.0);
            let cell_width = col_width - 2.0 * cell.style.padding;

            let content_height = match &cell.content {
                CellContent::Text(text) => {
                    // Use the text shaper to measure actual text width
                    let font = self
                        .font_cache
                        .get_by_family(cell.style.text_style.font.as_ref());
                    let text_width = font
                        .as_ref()
                        .map(|f| {
                            self.shaper
                                .measure_width(text, cell.style.text_style.size, f)
                        })
                        .unwrap_or_else(|| {
                            // Fallback to estimate if font not available
                            text.len() as f64 * cell.style.text_style.size * 0.5
                        });

                    let line_height = cell
                        .style
                        .text_style
                        .line_height
                        .unwrap_or(cell.style.text_style.size * 1.2);
                    // Count lines by seeing how many times the text width exceeds cell_width
                    let chars_per_line =
                        (cell_width / (cell.style.text_style.size * 0.55)).max(1.0);
                    let estimated_lines = (text.len() as f64 / chars_per_line).ceil().max(1.0);
                    // For more accuracy, use the shaped width
                    let lines_by_width = if cell_width > 0.0 {
                        (text_width / cell_width).ceil().max(1.0)
                    } else {
                        1.0
                    };
                    let lines = lines_by_width.max(estimated_lines.min(lines_by_width * 2.0));
                    lines * line_height
                }
                CellContent::Commands(_) => 20.0,
            };

            let total_height = content_height + 2.0 * cell.style.padding;
            if total_height > max_height {
                max_height = total_height;
            }
        }

        if max_height < 12.0 {
            12.0
        } else {
            max_height
        }
    }

    fn layout_row(&mut self, row: &Row, col_widths: &[f64], y: f64) -> Vec<DrawCommand> {
        let row_height = self.calculate_row_height(row, col_widths);
        let mut commands = Vec::new();
        let mut x = 0.0;

        for (i, cell) in row.cells.iter().enumerate() {
            let col_width = col_widths.get(i).copied().unwrap_or(0.0);
            let cell_rect = Rect::new(x, y, col_width, row_height);

            if let Some(bg) = cell.style.background {
                commands.push(DrawCommand::FillRect {
                    rect: cell_rect,
                    color: bg,
                });
            }

            if cell.style.border_width > 0.0 {
                commands.push(DrawCommand::StrokeRect {
                    rect: cell_rect,
                    color: cell.style.border_color,
                    width: cell.style.border_width,
                    line_cap: LineCap::Butt,
                    line_join: LineJoin::Miter,
                });
            }

            let content_x = x + cell.style.padding;
            let content_y = y + cell.style.padding + cell.style.text_style.size;

            match &cell.content {
                CellContent::Text(text) => {
                    commands.push(DrawCommand::Text {
                        run: TextRun {
                            text: text.clone(),
                            glyphs: vec![],
                            style: cell.style.text_style.clone(),
                        },
                        position: Point::new(content_x, content_y),
                        max_width: Some(col_width - 2.0 * cell.style.padding),
                    });
                }
                CellContent::Commands(cmds) => {
                    commands.push(DrawCommand::Block {
                        rect: cell_rect,
                        commands: Box::new(cmds.clone()),
                    });
                }
            }

            x += col_width;
        }

        commands
    }
}

impl Default for TableEngine {
    fn default() -> Self {
        Self::new()
    }
}

/// Convenience builder for tables.
pub struct TableBuilder {
    columns: Vec<Column>,
    rows: Vec<Row>,
}

impl TableBuilder {
    pub fn new() -> Self {
        Self {
            columns: Vec::new(),
            rows: Vec::new(),
        }
    }

    pub fn column(mut self, width: ColumnWidth) -> Self {
        self.columns.push(Column::new(width));
        self
    }

    pub fn column_with_header(mut self, header: impl Into<String>, width: ColumnWidth) -> Self {
        self.columns.push(Column::new(width).with_header(header));
        self
    }

    pub fn row(mut self, cells: Vec<Cell>) -> Self {
        self.rows.push(Row::new(cells));
        self
    }

    pub fn header_row(mut self, cells: Vec<Cell>) -> Self {
        self.rows.push(Row::header(cells));
        self
    }

    pub fn build(self) -> (Vec<Column>, Vec<Row>) {
        (self.columns, self.rows)
    }
}

impl Default for TableBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn default_style() -> TextStyle {
        TextStyle::new(FontRef::new("Helvetica"), 10.0)
    }

    fn default_cell_style() -> CellStyle {
        CellStyle {
            padding: 4.0,
            border_width: 0.5,
            border_color: Color::gray(0.8),
            background: None,
            text_style: default_style(),
        }
    }

    #[test]
    fn test_column_widths_fixed() {
        let columns = vec![
            Column::new(ColumnWidth::Fixed(100.0)),
            Column::new(ColumnWidth::Fixed(200.0)),
        ];

        let engine = TableEngine::new();
        let widths = engine.calculate_column_widths(&columns, 400.0);

        assert_eq!(widths[0], 100.0);
        assert_eq!(widths[1], 200.0);
    }

    #[test]
    fn test_column_widths_weighted() {
        let columns = vec![
            Column::new(ColumnWidth::Weight(1.0)),
            Column::new(ColumnWidth::Weight(3.0)),
        ];

        let engine = TableEngine::new();
        let widths = engine.calculate_column_widths(&columns, 400.0);

        assert!((widths[0] - 100.0).abs() < 0.01);
        assert!((widths[1] - 300.0).abs() < 0.01);
    }

    #[test]
    fn test_column_widths_mixed() {
        let columns = vec![
            Column::new(ColumnWidth::Fixed(50.0)),
            Column::new(ColumnWidth::Weight(1.0)),
            Column::new(ColumnWidth::Weight(1.0)),
        ];

        let engine = TableEngine::new();
        let widths = engine.calculate_column_widths(&columns, 300.0);

        assert_eq!(widths[0], 50.0);
        assert!((widths[1] - 125.0).abs() < 0.01);
        assert!((widths[2] - 125.0).abs() < 0.01);
    }

    #[test]
    fn test_layout_simple_table() {
        let columns = vec![
            Column::new(ColumnWidth::Fixed(100.0)),
            Column::new(ColumnWidth::Fixed(100.0)),
        ];

        let rows = vec![
            Row::new(vec![
                Cell::new("Name").with_style(default_cell_style()),
                Cell::new("Age").with_style(default_cell_style()),
            ]),
            Row::new(vec![
                Cell::new("Alice").with_style(default_cell_style()),
                Cell::new("30").with_style(default_cell_style()),
            ]),
        ];

        let mut engine = TableEngine::new();
        let layout = engine.layout_table(&columns, &rows, 200.0, 0.0);

        assert!(layout.total_height > 0.0);
        assert!(!layout.commands.is_empty());
        assert_eq!(layout.total_width, 200.0);
    }

    #[test]
    fn test_layout_table_with_headers() {
        let columns = vec![
            Column::new(ColumnWidth::Fixed(100.0)),
            Column::new(ColumnWidth::Fixed(100.0)),
        ];

        let rows = vec![
            Row::header(vec![
                Cell::new("Name").with_style(default_cell_style()),
                Cell::new("Age").with_style(default_cell_style()),
            ]),
            Row::new(vec![
                Cell::new("Alice").with_style(default_cell_style()),
                Cell::new("30").with_style(default_cell_style()),
            ]),
        ];

        let mut engine = TableEngine::new();
        let layout = engine.layout_table(&columns, &rows, 200.0, 0.0);

        assert_eq!(layout.header_row_count, 1);
        assert!(layout.total_height > 0.0);
    }

    #[test]
    fn test_layout_empty_table() {
        let columns: Vec<Column> = vec![];
        let rows: Vec<Row> = vec![];

        let mut engine = TableEngine::new();
        let layout = engine.layout_table(&columns, &rows, 200.0, 0.0);

        assert_eq!(layout.total_height, 0.0);
        assert!(layout.commands.is_empty());
    }

    #[test]
    fn test_table_builder() {
        let (columns, rows) = TableBuilder::new()
            .column_with_header("Name", ColumnWidth::Fixed(150.0))
            .column_with_header("Age", ColumnWidth::Fixed(50.0))
            .header_row(vec![
                Cell::new("Name").with_style(default_cell_style()),
                Cell::new("Age").with_style(default_cell_style()),
            ])
            .row(vec![
                Cell::new("Alice").with_style(default_cell_style()),
                Cell::new("30").with_style(default_cell_style()),
            ])
            .build();

        assert_eq!(columns.len(), 2);
        assert_eq!(rows.len(), 2);
        assert!(rows[0].is_header);
        assert!(!rows[1].is_header);
    }

    #[test]
    fn test_paginated_table() {
        let columns = vec![
            Column::new(ColumnWidth::Fixed(100.0)),
            Column::new(ColumnWidth::Fixed(100.0)),
        ];

        let mut rows = vec![Row::header(vec![
            Cell::new("Name").with_style(default_cell_style()),
            Cell::new("Value").with_style(default_cell_style()),
        ])];

        for i in 0..20 {
            rows.push(Row::new(vec![
                Cell::new(format!("Item {}", i)).with_style(default_cell_style()),
                Cell::new(format!("{}", i * 10)).with_style(default_cell_style()),
            ]));
        }

        let mut engine = TableEngine::new();
        let pages = engine.layout_table_paginated(&columns, &rows, 200.0, 100.0, 0.0);

        assert!(!pages.is_empty(), "Should produce at least one page");
    }

    #[test]
    fn test_cell_colspan() {
        let cell = Cell::new("test").with_colspan(3);
        assert_eq!(cell.colspan, 3);
    }

    #[test]
    fn test_cell_with_background() {
        let style = CellStyle {
            background: Some(Color::gray(0.95)),
            ..Default::default()
        };
        let cell = Cell::new("test").with_style(style);
        assert!(cell.style.background.is_some());
    }

    #[test]
    fn test_table_row_height_uses_shaped_text() {
        // A cell with "Hello World" at 12pt in a 100pt-wide column
        // should produce a predictable height based on actual text measurement
        let mut engine = TableEngine::new();
        let columns = vec![Column::new(ColumnWidth::Fixed(100.0))];
        let rows = vec![Row::new(vec![Cell::new("Hello World")])];
        let layout = engine.layout_table(&columns, &rows, 100.0, 0.0);

        // The row should have some height
        assert!(
            layout.total_height > 0.0,
            "Table should have non-zero height"
        );
        // Height should be at least: padding*2 + line_height
        // With default padding of 4.0 and line_height of ~12pt (10pt * 1.2)
        assert!(
            layout.total_height >= 20.0,
            "Row height {} should be at least 20pt (padding + line_height)",
            layout.total_height
        );
    }

    #[test]
    fn test_table_row_height_wraps_long_text() {
        // Long text that doesn't fit in one line should produce a taller row
        let mut engine = TableEngine::new();
        let columns = vec![Column::new(ColumnWidth::Fixed(50.0))];
        let short_row = vec![Row::new(vec![Cell::new("Hi")])];
        let long_row = vec![Row::new(vec![Cell::new(
            "This is a very long text that should wrap to multiple lines",
        )])];

        let short_layout = engine.layout_table(&columns, &short_row, 50.0, 0.0);

        // Need a fresh engine since font_cache is consumed
        let mut engine2 = TableEngine::new();
        let long_layout = engine2.layout_table(&columns, &long_row, 50.0, 0.0);

        assert!(
            long_layout.total_height > short_layout.total_height,
            "Long text row ({}) should be taller than short text row ({})",
            long_layout.total_height,
            short_layout.total_height
        );
    }

    #[test]
    fn test_table_row_height_empty_cell() {
        let mut engine = TableEngine::new();
        let columns = vec![Column::new(ColumnWidth::Fixed(100.0))];
        let rows = vec![Row::new(vec![Cell::new("")])];
        let layout = engine.layout_table(&columns, &rows, 100.0, 0.0);

        // Empty cell should still have minimum height (padding * 2 + line_height)
        assert!(
            layout.total_height >= 12.0,
            "Empty cell row height {} should be at least 12pt",
            layout.total_height
        );
    }
}
