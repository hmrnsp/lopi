/// Renders left-aligned columns separated by two spaces, with a header line.
pub fn render(header: &[&str], rows: &[Vec<String>]) -> String {
    let header: Vec<String> = header.iter().map(|cell| cell.to_string()).collect();
    let all: Vec<Vec<String>> = std::iter::once(header)
        .chain(rows.iter().cloned())
        .collect();
    let mut out = String::new();
    for line in align(&all) {
        out.push_str(&line);
        out.push('\n');
    }
    out
}

/// Pads cells so columns line up; one string per row. All rows must have the same number
/// of cells. The last column is not padded, so lines carry no trailing spaces.
pub fn align(rows: &[Vec<String>]) -> Vec<String> {
    let columns = rows.first().map_or(0, Vec::len);
    let mut widths = vec![0; columns];
    for row in rows {
        debug_assert_eq!(
            row.len(),
            columns,
            "every row needs the same number of cells"
        );
        for (width, cell) in widths.iter_mut().zip(row) {
            *width = (*width).max(cell.chars().count());
        }
    }
    rows.iter()
        .map(|row| {
            let mut line = String::new();
            for (i, cell) in row.iter().enumerate() {
                line.push_str(cell);
                if i + 1 < row.len() {
                    let pad = widths[i] - cell.chars().count();
                    line.extend(std::iter::repeat_n(' ', pad + 2));
                }
            }
            line.trim_end().to_string()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(cells: &[&str]) -> Vec<String> {
        cells.iter().map(|cell| cell.to_string()).collect()
    }

    #[test]
    fn aligns_columns() {
        let rows = [
            row(&["kantor", "root@10.0.0.5:2222", "office"]),
            row(&["vps", "h", ""]),
        ];
        assert_eq!(
            render(&["NAME", "TARGET", "NOTE"], &rows),
            "NAME    TARGET              NOTE\n\
             kantor  root@10.0.0.5:2222  office\n\
             vps     h\n"
        );
    }

    #[test]
    fn align_without_rows() {
        assert!(align(&[]).is_empty());
    }
}
