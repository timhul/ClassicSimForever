//! Plain text tables with a left-aligned first column and right-aligned numbers.

use std::fmt::Write;

pub struct Table {
    headers: Vec<String>,
    rows: Vec<Vec<String>>,
    /// Summary rows under the rows, set apart by a rule.
    totals: Vec<Vec<String>>,
    /// Columns (by index) aligned left; the rest are aligned right.
    left: Vec<usize>,
}

impl Table {
    /// A table whose first column is aligned left.
    pub fn new<const N: usize>(headers: [&str; N]) -> Self {
        Table {
            headers: headers.iter().map(|h| h.to_string()).collect(),
            rows: Vec::new(),
            totals: Vec::new(),
            left: vec![0],
        }
    }

    /// Also aligns `column` left.
    pub fn left(mut self, column: usize) -> Self {
        self.left.push(column);
        self
    }

    pub fn row(&mut self, cells: Vec<String>) {
        debug_assert_eq!(cells.len(), self.headers.len(), "row width");
        self.rows.push(cells);
    }

    /// Adds a summary row under the rows.
    pub fn total(&mut self, cells: Vec<String>) {
        debug_assert_eq!(cells.len(), self.headers.len(), "total width");
        self.totals.push(cells);
    }

    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    pub fn headers(&self) -> &[String] {
        &self.headers
    }

    pub fn rows(&self) -> &[Vec<String>] {
        &self.rows
    }

    pub fn totals(&self) -> &[Vec<String>] {
        &self.totals
    }

    /// Whether `column` is aligned left.
    pub fn is_left(&self, column: usize) -> bool {
        self.left.contains(&column)
    }

    pub fn render(&self) -> String {
        let mut widths: Vec<usize> = self.headers.iter().map(|h| h.chars().count()).collect();
        for row in self.rows.iter().chain(&self.totals) {
            for (width, cell) in widths.iter_mut().zip(row) {
                *width = (*width).max(cell.chars().count());
            }
        }
        let mut out = String::new();
        let rule = widths.iter().sum::<usize>() + 2 * widths.len().saturating_sub(1);
        let rule = format!("{}\n", "-".repeat(rule));
        self.render_row(&mut out, &self.headers, &widths);
        out.push_str(&rule);
        for row in &self.rows {
            self.render_row(&mut out, row, &widths);
        }
        if !self.totals.is_empty() {
            out.push_str(&rule);
            for row in &self.totals {
                self.render_row(&mut out, row, &widths);
            }
        }
        out
    }

    fn render_row(&self, out: &mut String, cells: &[String], widths: &[usize]) {
        let mut line = String::new();
        for (column, (cell, width)) in cells.iter().zip(widths).enumerate() {
            if column > 0 {
                line.push_str("  ");
            }
            if self.is_left(column) {
                let _ = write!(line, "{cell:<width$}");
            } else {
                let _ = write!(line, "{cell:>width$}");
            }
        }
        out.push_str(line.trim_end());
        out.push('\n');
    }
}

/// `value` as a percentage with one decimal, blank for zero.
pub fn percent(value: f64) -> String {
    if value == 0.0 {
        String::new()
    } else {
        format!("{:.1}%", value * 100.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn columns_are_padded_to_the_widest_cell() {
        let mut table = Table::new(["Name", "Value"]);
        table.row(vec!["Heroic Strike".into(), "1".into()]);
        table.row(vec!["Slam".into(), "12345".into()]);
        assert_eq!(
            table.render(),
            "Name           Value\n\
             --------------------\n\
             Heroic Strike      1\n\
             Slam           12345\n"
        );
    }

    #[test]
    fn totals_follow_a_rule_under_the_rows() {
        let mut table = Table::new(["Name", "Value"]);
        table.row(vec!["Slam".into(), "1".into()]);
        table.total(vec!["Total".into(), "12345".into()]);
        assert_eq!(
            table.render(),
            "Name   Value\n\
             ------------\n\
             Slam       1\n\
             ------------\n\
             Total  12345\n"
        );
    }

    #[test]
    fn percent_is_blank_for_zero() {
        assert_eq!(percent(0.0), "");
        assert_eq!(percent(0.1234), "12.3%");
    }
}
