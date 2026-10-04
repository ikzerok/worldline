use super::*;

/// 有界、严格RFC4180子集；以状态机处理字段，绝不按逗号/行split。
pub fn parse_catalog_csv(input: &str) -> Result<CatalogCsvTable, CatalogImportDiagnostic> {
    if input.len() > MAX_CSV_BYTES {
        return Err(CatalogImportDiagnostic::new(
            "CSV_LIMIT",
            "CSV超过2 MiB预算",
        ));
    }
    let input = input.strip_prefix('\u{feff}').unwrap_or(input);
    let mut chars = input.chars().peekable();
    let mut records = Vec::new();
    let mut cells = Vec::new();
    let mut cell_lines = Vec::new();
    let mut cell_line = 1u32;
    let mut cell = String::new();
    let mut quoted = false;
    let mut closed = false;
    let mut started = false;
    let mut line = 1u32;
    let mut record_line = 1u32;
    let mut normalization_count = 0;
    let error = |code, message: &str, row, column, line| {
        CatalogImportDiagnostic::new(code, message).at(row, column, line)
    };
    while let Some(mut ch) = chars.next() {
        if ch == '\r' {
            if chars.next_if_eq(&'\n').is_none() {
                return Err(error(
                    "CSV_CR",
                    "不支持孤立CR；请使用LF或CRLF",
                    records.len() + 1,
                    cells.len() + 1,
                    line,
                ));
            }
            ch = '\n';
            if quoted {
                normalization_count += 1;
            }
        }
        if quoted {
            if ch == '"' {
                if chars.next_if_eq(&'"').is_some() {
                    cell.push('"');
                } else {
                    quoted = false;
                    closed = true;
                }
            } else {
                cell.push(ch);
            }
        } else if ch == ',' || ch == '\n' {
            cells.push(std::mem::take(&mut cell));
            cell_lines.push(cell_line);
            if cells.len() > MAX_COLUMNS {
                return Err(error(
                    "CSV_LIMIT",
                    "CSV超过64列预算",
                    records.len() + 1,
                    cells.len(),
                    line,
                ));
            }
            closed = false;
            started = false;
            if ch == '\n' {
                records.push((
                    CatalogCsvRow {
                        cells: std::mem::take(&mut cells),
                        line: record_line,
                    },
                    std::mem::take(&mut cell_lines),
                    line,
                ));
                if records.len() > MAX_ROWS + 1 {
                    return Err(error(
                        "CSV_LIMIT",
                        "CSV超过500数据行预算",
                        records.len(),
                        1,
                        line,
                    ));
                }
                record_line = line + 1;
                cell_line = line + 1;
            } else {
                cell_line = line;
            }
        } else if closed {
            return Err(error(
                "CSV_QUOTE",
                "闭引号后只能是逗号或换行",
                records.len() + 1,
                cells.len() + 1,
                line,
            ));
        } else if ch == '"' {
            if started {
                return Err(error(
                    "CSV_QUOTE",
                    "非引号字段中不能出现双引号",
                    records.len() + 1,
                    cells.len() + 1,
                    line,
                ));
            }
            quoted = true;
            started = true;
        } else {
            cell.push(ch);
            started = true;
        }
        if cell.len() > MAX_CELL_BYTES {
            return Err(error(
                "CSV_LIMIT",
                "单元格超过64 KiB预算",
                records.len() + 1,
                cells.len() + 1,
                line,
            ));
        }
        if ch == '\n' {
            line += 1;
        }
    }
    if quoted {
        return Err(error(
            "CSV_QUOTE",
            "CSV引号未闭合",
            records.len() + 1,
            cells.len() + 1,
            line,
        ));
    }
    if started || closed || !cells.is_empty() {
        cells.push(cell);
        cell_lines.push(cell_line);
        records.push((
            CatalogCsvRow {
                cells,
                line: record_line,
            },
            cell_lines,
            line,
        ));
    }
    if records.is_empty() {
        return Err(error("CSV_HEADER", "CSV缺少表头", 1, 1, 1));
    }
    let (header, header_lines, _) = records.remove(0);
    let headers = header.cells;
    if headers.len() > MAX_COLUMNS || records.len() > MAX_ROWS {
        return Err(error(
            "CSV_LIMIT",
            "CSV超过500行或64列预算",
            records.len() + 1,
            headers.len(),
            line,
        ));
    }
    let mut seen = std::collections::BTreeSet::new();
    for (index, header) in headers.iter().enumerate() {
        if header.trim().is_empty() || !seen.insert(header) {
            return Err(error(
                "CSV_HEADER",
                "表头不能为空或重复",
                1,
                index + 1,
                header_lines[index],
            ));
        }
    }
    for (index, (record, _, end_line)) in records.iter().enumerate() {
        if record.cells.len() != headers.len() {
            return Err(error(
                "CSV_COLUMNS",
                "数据行列数与表头不一致",
                index + 2,
                record.cells.len().min(headers.len()) + 1,
                *end_line,
            ));
        }
    }
    Ok(CatalogCsvTable {
        headers,
        rows: records.into_iter().map(|(record, _, _)| record).collect(),
        normalization_count,
    })
}
