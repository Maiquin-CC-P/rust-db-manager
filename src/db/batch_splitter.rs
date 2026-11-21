/// State-machine T-SQL `GO` batch splitter.
///
/// In SQL Server, `GO` is NOT a T-SQL statement executed by the engine; it is a client batch separator
/// recognized by tools like SSMS and `sqlcmd`.
///
/// This parser splits a T-SQL script into executable batches while guaranteeing that `GO` inside:
/// - Single-line comments (`-- ... GO`)
/// - Block comments (`/* ... GO ... */`)
/// - String literals (`'Hello\nGO\nWorld'`)
/// - Bracketed identifiers (`[GO]`)
/// is NEVER treated as a batch boundary.
/// It also supports `GO <count>` (repeating the batch `<count>` times) and trailing comments on a `GO` line.
pub fn split_tsql_batches(script: &str) -> Vec<String> {
    let mut batches = Vec::new();
    let mut current_batch = String::new();

    let mut in_single_quote = false;
    let mut in_bracket = false;
    let mut in_block_comment = false;

    for raw_line in script.replace("\r\n", "\n").lines() {
        // Only check for standalone GO if we are not entering the line inside a multi-line string,
        // bracketed identifier, or block comment.
        if !in_single_quote && !in_bracket && !in_block_comment {
            if let Some(repeat_count) = parse_standalone_go_line(raw_line) {
                let trimmed = current_batch.trim();
                if !trimmed.is_empty() {
                    for _ in 0..repeat_count {
                        batches.push(trimmed.to_string());
                    }
                }
                current_batch.clear();
                continue;
            }
        }

        // Advance lexer state across the characters of `raw_line`
        let chars: Vec<char> = raw_line.chars().collect();
        let mut i = 0;
        let n = chars.len();

        while i < n {
            let c = chars[i];
            let next = if i + 1 < n { Some(chars[i + 1]) } else { None };

            if in_block_comment {
                if c == '*' && next == Some('/') {
                    in_block_comment = false;
                    i += 2;
                    continue;
                }
                i += 1;
                continue;
            }

            if in_single_quote {
                if c == '\'' {
                    if next == Some('\'') {
                        // Escaped single quote ''
                        i += 2;
                        continue;
                    } else {
                        in_single_quote = false;
                    }
                }
                i += 1;
                continue;
            }

            if in_bracket {
                if c == ']' {
                    in_bracket = false;
                }
                i += 1;
                continue;
            }

            // Normal code state
            if c == '-' && next == Some('-') {
                // Rest of the line is a line comment
                break;
            }
            if c == '/' && next == Some('*') {
                in_block_comment = true;
                i += 2;
                continue;
            }
            if c == '\'' {
                in_single_quote = true;
                i += 1;
                continue;
            }
            if c == '[' {
                in_bracket = true;
                i += 1;
                continue;
            }

            i += 1;
        }

        current_batch.push_str(raw_line);
        current_batch.push('\n');
    }

    let trimmed = current_batch.trim();
    if !trimmed.is_empty() {
        batches.push(trimmed.to_string());
    }

    batches
}

/// Checks if a line is a standalone `GO [count] [-- comment]` command.
/// Returns `Some(repeat_count)` if it is a valid batch separator line.
fn parse_standalone_go_line(line: &str) -> Option<usize> {
    // Strip trailing single-line comment `-- ...` if present
    let code_part = match line.find("--") {
        Some(idx) => &line[..idx],
        None => line,
    };
    let trimmed = code_part.trim().trim_end_matches(';').trim();
    if trimmed.is_empty() {
        return None;
    }

    let mut parts = trimmed.split_whitespace();
    let first = parts.next()?;
    if !first.eq_ignore_ascii_case("GO") {
        return None;
    }

    match parts.next() {
        None => Some(1),
        Some(count_str) => {
            if parts.next().is_some() {
                return None;
            }
            count_str.parse::<usize>().ok().filter(|&c| c >= 1)
        }
    }
}
