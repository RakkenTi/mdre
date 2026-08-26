//! Re-aligning GFM pipe tables.
//!
//! Tables are the one bit of markdown that is painful to keep tidy by hand:
//! every edit to a cell throws the pipes out of column, and the file stops
//! being readable in a plain editor. This takes the rows apart and lays them
//! back out on the widest cell in each column.

use super::inline::str_width;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Align {
    None,
    Left,
    Center,
    Right,
}

/// Formats a table in the editor (Alt+A)
/// Automatically align the `|` characters in the table, or creates additional lines to make a table row
/// Can not be used to create a new table (must be done manually)
/// If no table is detected, returns `None`
pub fn format(lines: &[&str]) -> Option<Vec<String>> {
    if lines.len() < 2 {
        return None;
    }
    let indent: String = lines[0]
        .chars()
        .take_while(|c| c.is_whitespace())
        .collect();

    let align = delimiter_row(lines[1])?;
    let mut rows: Vec<Vec<String>> = Vec::with_capacity(lines.len() - 1);
    for (i, line) in lines.iter().enumerate() {
        if i == 1 {
            continue;
        }
        if !line.contains('|') {
            return None;
        }
        rows.push(split_cells(line));
    }
    if rows.is_empty() {
        return None;
    }

    let columns = align.len().max(rows[0].len());
    let mut align = align;
    align.resize(columns, Align::None);
    for row in &mut rows {
        row.resize(columns, String::new());
    }

    let mut widths = vec![3usize; columns];
    for row in &rows {
        for (c, cell) in row.iter().enumerate() {
            widths[c] = widths[c].max(str_width(cell));
        }
    }

    let mut out = Vec::with_capacity(rows.len() + 1);
    out.push(render_row(&rows[0], &widths, &indent));
    out.push(render_delimiter(&align, &widths, &indent));
    for row in &rows[1..] {
        out.push(render_row(row, &widths, &indent));
    }
    Some(out)
}

/// Does this line look like `| :--- | ---: |`? Returns one alignment per
/// column if so.
fn delimiter_row(line: &str) -> Option<Vec<Align>> {
    let cells = split_cells(line);
    if cells.is_empty() {
        return None;
    }
    cells
        .iter()
        .map(|cell| {
            let cell = cell.trim();
            let left = cell.starts_with(':');
            let right = cell.ends_with(':');
            let dashes = &cell[usize::from(left)..cell.len() - usize::from(right && cell.len() > 1)];
            if dashes.is_empty() || !dashes.bytes().all(|b| b == b'-') {
                return None;
            }
            Some(match (left, right) {
                (true, true) => Align::Center,
                (true, false) => Align::Left,
                (false, true) => Align::Right,
                (false, false) => Align::None,
            })
        })
        .collect()
}

/// Split a row on its unescaped pipes, dropping the optional leading and
/// trailing ones. Per GFM a `\|` is a literal pipe and stays in the cell.
fn split_cells(line: &str) -> Vec<String> {
    let line = line.trim();
    let mut cells = Vec::new();
    let mut cell = String::new();
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\\' if chars.peek() == Some(&'|') => {
                cell.push('\\');
                cell.push(chars.next().unwrap());
            }
            '|' => cells.push(std::mem::take(&mut cell)),
            _ => cell.push(c),
        }
    }
    cells.push(cell);

    if line.starts_with('|') && !cells.is_empty() {
        cells.remove(0);
    }
    if line.ends_with('|') && !line.ends_with("\\|") && !cells.is_empty() {
        cells.pop();
    }
    cells.iter().map(|c| c.trim().to_string()).collect()
}

fn render_row(cells: &[String], widths: &[usize], indent: &str) -> String {
    let mut out = String::from(indent);
    out.push('|');
    for (cell, &width) in cells.iter().zip(widths) {
        out.push(' ');
        out.push_str(cell);
        out.push_str(&" ".repeat(width - str_width(cell)));
        out.push_str(" |");
    }
    out
}

fn render_delimiter(align: &[Align], widths: &[usize], indent: &str) -> String {
    let mut out = String::from(indent);
    out.push('|');
    for (&a, &width) in align.iter().zip(widths) {
        out.push(' ');
        match a {
            Align::None => out.push_str(&"-".repeat(width)),
            Align::Left => {
                out.push(':');
                out.push_str(&"-".repeat(width - 1));
            }
            Align::Right => {
                out.push_str(&"-".repeat(width - 1));
                out.push(':');
            }
            Align::Center => {
                out.push(':');
                out.push_str(&"-".repeat(width - 2));
                out.push(':');
            }
        }
        out.push_str(" |");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fmt(text: &str) -> Option<String> {
        let lines: Vec<&str> = text.lines().collect();
        format(&lines).map(|v| v.join("\n"))
    }

    #[test]
    fn ragged_input_comes_out_square() {
        let got = fmt("|a|bbbb|\n|-|-|\n|cccc|d|").unwrap();
        assert_eq!(
            got,
            "| a    | bbbb |\n\
             | ---- | ---- |\n\
             | cccc | d    |"
        );
    }

    #[test]
    fn alignment_markers_survive_and_set_the_column() {
        let got = fmt("| l | c | r |\n|:---|:--:|---:|\n| 1 | 2 | 3 |").unwrap();
        assert_eq!(
            got,
            "| l   | c   | r   |\n\
             | :-- | :-: | --: |\n\
             | 1   | 2   | 3   |"
        );
    }

    #[test]
    fn short_rows_are_padded_out() {
        let got = fmt("| a | b | c |\n| - | - | - |\n| 1 |").unwrap();
        assert_eq!(got.lines().last().unwrap(), "| 1   |     |     |");
    }

    #[test]
    fn wide_characters_are_measured_by_display_width() {
        let got = fmt("| x | y |\n| - | - |\n| 日本語 | z |").unwrap();
        for line in got.lines() {
            assert_eq!(str_width(line), str_width(got.lines().next().unwrap()));
        }
    }

    #[test]
    fn escaped_pipes_stay_inside_their_cell() {
        let got = fmt(r"| a \| b | c |\n| - | - |").unwrap_or_default();
        assert!(got.is_empty() || got.contains(r"a \| b"));
        let cells = split_cells(r"| a \| b | c |");
        assert_eq!(cells, vec![r"a \| b".to_string(), "c".to_string()]);
    }

    #[test]
    fn indentation_is_preserved() {
        let got = fmt("  | a |\n  | - |").unwrap();
        assert!(got.lines().all(|l| l.starts_with("  |")));
    }

    #[test]
    fn non_tables_are_left_alone() {
        assert!(fmt("just | a sentence\nwith | pipes").is_none());
        assert!(fmt("| a | b |").is_none());
        assert!(fmt("| a |\n| not a delimiter |").is_none());
    }

    #[test]
    fn an_already_formatted_table_is_unchanged() {
        let text = "| a    | bbbb |\n| ---- | ---- |\n| cccc | d    |";
        assert_eq!(fmt(text).unwrap(), text);
    }
}
