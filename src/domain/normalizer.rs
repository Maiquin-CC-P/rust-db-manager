use crate::domain::manifest::compute_normalized_str_sha256;

/// Strips SQL comments (`-- ...` and `/* ... */`), normalizes whitespace,
/// converts `CREATE OR ALTER` to `CREATE` for canonical structural comparison,
/// and computes a semantic SHA-256 hash.
pub fn normalize_tsql_definition(definition: &str) -> String {
    let without_comments = strip_sql_comments(definition);
    let mut tokens: Vec<String> = without_comments
        .split_whitespace()
        .map(|tok| tok.to_string())
        .collect();

    // Normalize "CREATE OR ALTER" -> "CREATE" at the beginning of a programmable module
    if tokens.len() >= 3
        && tokens[0].eq_ignore_ascii_case("CREATE")
        && tokens[1].eq_ignore_ascii_case("OR")
        && tokens[2].eq_ignore_ascii_case("ALTER")
    {
        tokens.drain(1..=2);
    }

    // Normalize PROC -> PROCEDURE
    if tokens.len() >= 2 && tokens[0].eq_ignore_ascii_case("CREATE") {
        if tokens[1].eq_ignore_ascii_case("PROC") {
            tokens[1] = "PROCEDURE".to_string();
        }
    }

    tokens.join(" ")
}

pub fn compute_semantic_sql_hash(definition: &str) -> String {
    let normalized = normalize_tsql_definition(definition);
    compute_normalized_str_sha256(&normalized.to_uppercase())
}

fn strip_sql_comments(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let chars: Vec<char> = input.chars().collect();
    let mut i = 0;
    let n = chars.len();

    let mut in_single_quote = false;
    let mut in_bracket = false;

    while i < n {
        let c = chars[i];
        let next = if i + 1 < n { Some(chars[i + 1]) } else { None };

        if in_single_quote {
            out.push(c);
            if c == '\'' {
                if next == Some('\'') {
                    out.push('\'');
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
            out.push(c);
            if c == ']' {
                in_bracket = false;
            }
            i += 1;
            continue;
        }

        if c == '\'' {
            in_single_quote = true;
            out.push(c);
            i += 1;
            continue;
        }

        if c == '[' {
            in_bracket = true;
            out.push(c);
            i += 1;
            continue;
        }

        // Line comment: --
        if c == '-' && next == Some('-') {
            i += 2;
            while i < n && chars[i] != '\n' {
                i += 1;
            }
            out.push(' ');
            continue;
        }

        // Block comment: /* ... */
        if c == '/' && next == Some('*') {
            i += 2;
            while i + 1 < n && !(chars[i] == '*' && chars[i + 1] == '/') {
                i += 1;
            }
            i += 2; // skip */
            out.push(' ');
            continue;
        }

        out.push(c);
        i += 1;
    }

    out
}
