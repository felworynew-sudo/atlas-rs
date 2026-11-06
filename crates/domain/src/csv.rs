//! Минимальный разбор CSV/TSV для импорта: кавычки, экранирование `""`, переводы строк, BOM, автоопределение разделителя.

/// Разделитель по первой строке: `;`, табуляция или `,` (больше всего вхождений вне кавычек).
pub fn detect_delimiter(text: &str) -> char {
    let first = text.trim_start_matches('\u{feff}').lines().next().unwrap_or("");
    let mut best = (',', 0usize);
    for d in [';', '\t', ','] {
        let mut in_q = false;
        let mut n = 0;
        for c in first.chars() {
            if c == '"' {
                in_q = !in_q;
            } else if c == d && !in_q {
                n += 1;
            }
        }
        if n > best.1 {
            best = (d, n);
        }
    }
    best.0
}

/// Разбор в строки полей; полностью пустые строки пропускаются. Возвращает `(номер строки файла с 1, поля)`.
pub fn parse(text: &str, delim: char) -> Result<Vec<(usize, Vec<String>)>, String> {
    let text = text.trim_start_matches('\u{feff}');
    let mut rows: Vec<(usize, Vec<String>)> = Vec::new();
    let mut row: Vec<String> = Vec::new();
    let mut field = String::new();
    let mut in_q = false;
    let mut line = 1usize;
    let mut row_line = 1usize;
    let mut chars = text.chars().peekable();
    let push_row = |row: &mut Vec<String>, line_no: usize, rows: &mut Vec<(usize, Vec<String>)>| {
        if row.iter().any(|f| !f.trim().is_empty()) {
            rows.push((line_no, std::mem::take(row)));
        } else {
            row.clear();
        }
    };
    while let Some(c) = chars.next() {
        if in_q {
            if c == '"' {
                if chars.peek() == Some(&'"') {
                    field.push('"');
                    chars.next();
                } else {
                    in_q = false;
                }
            } else {
                if c == '\n' {
                    line += 1;
                }
                field.push(c);
            }
        } else if c == '"' && field.is_empty() {
            in_q = true;
        } else if c == delim {
            row.push(std::mem::take(&mut field));
        } else if c == '\r' {
            // обрабатывается вместе с '\n'
        } else if c == '\n' {
            row.push(std::mem::take(&mut field));
            push_row(&mut row, row_line, &mut rows);
            line += 1;
            row_line = line;
        } else {
            field.push(c);
        }
    }
    if in_q {
        return Err("Незакрытые кавычки в файле".into());
    }
    if !field.is_empty() || !row.is_empty() {
        row.push(field);
        push_row(&mut row, row_line, &mut rows);
    }
    Ok(rows)
}

/// Число из ячейки: пробелы (в т.ч. неразрывные) убираются, запятая считается десятичным разделителем.
pub fn parse_number(s: &str) -> Option<f64> {
    let cleaned: String = s.chars().filter(|c| !c.is_whitespace() && *c != '\u{a0}').collect::<String>().replace(',', ".");
    if cleaned.is_empty() {
        return None;
    }
    cleaned.parse::<f64>().ok().filter(|v| v.is_finite())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn delimiter_detection() {
        assert_eq!(detect_delimiter("a;b;c\n1;2;3"), ';');
        assert_eq!(detect_delimiter("a\tb\tc"), '\t');
        assert_eq!(detect_delimiter("a,b,c"), ',');
        assert_eq!(detect_delimiter("\"a;b\",c"), ',');
    }

    #[test]
    fn parses_quotes_crlf_and_blank_lines() {
        let text = "\u{feff}name,price\r\n\"Молоко, 1л\",100\r\n\r\n\"Сыр \"\"Гауда\"\"\",250\r\n";
        let rows = parse(text, ',').unwrap();
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[1], (2, vec!["Молоко, 1л".to_string(), "100".to_string()]));
        assert_eq!(rows[2], (4, vec!["Сыр \"Гауда\"".to_string(), "250".to_string()]));
    }

    #[test]
    fn multiline_quoted_field_keeps_line_numbers() {
        let rows = parse("a,b\n\"x\ny\",1\nz,2", ',').unwrap();
        assert_eq!(rows[1].1[0], "x\ny");
        assert_eq!(rows[2].0, 4);
        assert!(parse("a,\"b", ',').is_err());
    }

    #[test]
    fn numbers_with_comma_and_spaces() {
        assert_eq!(parse_number("1 250,50"), Some(1250.5));
        assert_eq!(parse_number("12.5"), Some(12.5));
        assert_eq!(parse_number(""), None);
        assert_eq!(parse_number("abc"), None);
    }
}
