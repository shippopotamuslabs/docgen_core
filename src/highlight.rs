//! Hand-rolled syntax highlighting (lightweight, no syntect).

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenKind {
    Default,
    Keyword,
    String,
    Comment,
    Number,
}

#[derive(Debug, Clone)]
pub struct Span {
    pub start: usize,
    pub end: usize,
    pub kind: TokenKind,
}

/// Expand tabs so monospace layouts match editors (common source of "flat" code in exports).
fn expand_tabs(s: &str, tab_cols: usize) -> String {
    let mut out = String::with_capacity(s.len());
    let mut lines = s.split('\n').peekable();
    while let Some(line) = lines.next() {
        let mut col = 0usize;
        for ch in line.chars() {
            if ch == '\t' {
                let sp = tab_cols.saturating_sub(col % tab_cols).max(1);
                for _ in 0..sp {
                    out.push(' ');
                }
                col += sp;
            } else {
                out.push(ch);
                col += 1;
            }
        }
        if lines.peek().is_some() {
            out.push('\n');
        }
    }
    out
}

/// Pretty-print JSON when `language` is `json` / `JSON`; always normalizes tabs for display.
pub fn prepare_code_text(language: Option<&str>, text: &str) -> String {
    let lang = language.as_deref().unwrap_or("").to_ascii_lowercase();
    let base = if lang == "json" {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(text) {
            serde_json::to_string_pretty(&v).unwrap_or_else(|_| text.to_string())
        } else {
            text.to_string()
        }
    } else {
        text.to_string()
    };
    expand_tabs(&base, 4)
}

pub fn tokenize(language: Option<&str>, text: &str) -> Vec<Span> {
    let lang = language.as_deref().unwrap_or("").to_ascii_lowercase();
    match lang.as_str() {
        "rust" | "rs" => tokenize_rust_like(text, RUST_KEYWORDS),
        "python" | "py" => tokenize_rust_like(text, PYTHON_KEYWORDS),
        "js" | "javascript" | "ts" | "typescript" | "kotlin" | "kt" | "java" | "c" | "cpp" | "go"
        | "swift" | "bash" | "sh" | "sql" | "yaml" | "yml" | "toml" => {
            tokenize_rust_like(text, GENERIC_KEYWORDS)
        }
        "json" => tokenize_json(text),
        _ => vec![],
    }
}

const RUST_KEYWORDS: &[&str] = &[
    "as", "async", "await", "break", "const", "continue", "crate", "dyn", "else", "enum", "extern",
    "false", "fn", "for", "if", "impl", "in", "let", "loop", "match", "mod", "move", "mut", "pub",
    "ref", "return", "self", "Self", "static", "struct", "super", "trait", "true", "type",
    "unsafe", "use", "where", "while",
];

const PYTHON_KEYWORDS: &[&str] = &[
    "and", "as", "assert", "async", "await", "break", "class", "continue", "def", "del", "elif",
    "else", "except", "False", "finally", "for", "from", "global", "if", "import", "in", "is",
    "lambda", "None", "nonlocal", "not", "or", "pass", "raise", "return", "True", "try", "while",
    "with", "yield",
];

const GENERIC_KEYWORDS: &[&str] = &[
    "if", "else", "for", "while", "return", "function", "const", "let", "var", "class", "import",
    "export", "from", "def", "fn", "match", "case", "break", "continue", "true", "false", "null",
    "void", "public", "private", "static", "new", "try", "catch", "throw", "package", "select",
];

fn tokenize_rust_like(text: &str, keywords: &[&str]) -> Vec<Span> {
    let mut out = Vec::new();
    let bytes = text.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        let c = bytes[i];
        if c == b'/' && i + 1 < bytes.len() && bytes[i + 1] == b'/' {
            let start = i;
            while i < bytes.len() && bytes[i] != b'\n' {
                i += 1;
            }
            out.push(Span {
                start,
                end: i,
                kind: TokenKind::Comment,
            });
            continue;
        }
        if c == b'/' && i + 1 < bytes.len() && bytes[i + 1] == b'*' {
            let start = i;
            i += 2;
            while i + 1 < bytes.len() && !(bytes[i] == b'*' && bytes[i + 1] == b'/') {
                i += 1;
            }
            i = (i + 2).min(bytes.len());
            out.push(Span {
                start,
                end: i,
                kind: TokenKind::Comment,
            });
            continue;
        }
        if c == b'"' || c == b'\'' {
            let quote = c;
            let start = i;
            i += 1;
            while i < bytes.len() {
                if bytes[i] == b'\\' {
                    i = (i + 2).min(bytes.len());
                    continue;
                }
                if bytes[i] == quote {
                    i += 1;
                    break;
                }
                i += 1;
            }
            out.push(Span {
                start,
                end: i,
                kind: TokenKind::String,
            });
            continue;
        }
        if c.is_ascii_digit() {
            let start = i;
            while i < bytes.len() && (bytes[i].is_ascii_digit() || bytes[i] == b'_' || bytes[i] == b'.')
            {
                i += 1;
            }
            out.push(Span {
                start,
                end: i,
                kind: TokenKind::Number,
            });
            continue;
        }
        if c.is_ascii_alphabetic() || c == b'_' {
            let start = i;
            while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                i += 1;
            }
            let word = &text[start..i];
            if keywords.iter().any(|k| *k == word) {
                out.push(Span {
                    start,
                    end: i,
                    kind: TokenKind::Keyword,
                });
            }
            continue;
        }
        i += 1;
    }
    out
}

fn tokenize_json(text: &str) -> Vec<Span> {
    let mut out = Vec::new();
    let bytes = text.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        let c = bytes[i];
        if c == b'"' {
            let start = i;
            i += 1;
            while i < bytes.len() {
                if bytes[i] == b'\\' {
                    i = (i + 2).min(bytes.len());
                    continue;
                }
                if bytes[i] == b'"' {
                    i += 1;
                    break;
                }
                i += 1;
            }
            out.push(Span {
                start,
                end: i,
                kind: TokenKind::String,
            });
            continue;
        }
        if c.is_ascii_digit() || c == b'-' {
            let start = i;
            i += 1;
            while i < bytes.len()
                && (bytes[i].is_ascii_digit()
                    || bytes[i] == b'.'
                    || bytes[i] == b'e'
                    || bytes[i] == b'E'
                    || bytes[i] == b'-'
                    || bytes[i] == b'+')
            {
                i += 1;
            }
            out.push(Span {
                start,
                end: i,
                kind: TokenKind::Number,
            });
            continue;
        }
        if i + 4 <= bytes.len() && text[i..i + 4].eq_ignore_ascii_case("true") {
            out.push(Span {
                start: i,
                end: i + 4,
                kind: TokenKind::Keyword,
            });
            i += 4;
            continue;
        }
        if i + 5 <= bytes.len() && text[i..i + 5].eq_ignore_ascii_case("false") {
            out.push(Span {
                start: i,
                end: i + 5,
                kind: TokenKind::Keyword,
            });
            i += 5;
            continue;
        }
        if i + 4 <= bytes.len() && text[i..i + 4].eq_ignore_ascii_case("null") {
            out.push(Span {
                start: i,
                end: i + 4,
                kind: TokenKind::Keyword,
            });
            i += 4;
            continue;
        }
        i += 1;
    }
    out
}

/// Light-background syntax colors.
#[allow(dead_code)]
pub fn rgb_for_kind(k: TokenKind) -> [f32; 3] {
    match k {
        TokenKind::Default => [0.0, 0.0, 0.0],
        TokenKind::Keyword => [0.0, 0.0, 0.55],
        TokenKind::String => [0.0, 0.45, 0.0],
        TokenKind::Comment => [0.45, 0.45, 0.45],
        TokenKind::Number => [0.55, 0.0, 0.0],
    }
}

/// Light-background syntax colors; DOCX/PDF code export uses [`hex_for_kind_code_dark`].
#[allow(dead_code)]
pub fn hex_for_kind(k: TokenKind) -> &'static str {
    match k {
        TokenKind::Default => "000000",
        TokenKind::Keyword => "00008B",
        TokenKind::String => "006400",
        TokenKind::Comment => "737373",
        TokenKind::Number => "8B0000",
    }
}

/// Syntax colors for dark code blocks in DOCX (light text on `#1E222A`-style fill).
pub fn hex_for_kind_code_dark(k: TokenKind) -> &'static str {
    match k {
        TokenKind::Default => "D4D4D4",
        TokenKind::Keyword => "C586C0",
        TokenKind::String => "CE9178",
        TokenKind::Comment => "6A9955",
        TokenKind::Number => "B5CEA8",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prepare_code_text_expands_tabs() {
        let out = prepare_code_text(Some("python"), "a\tb\n\tc");
        // 4-column tab stops: after "a" (col 1) → advance to col 4 (three spaces).
        assert_eq!(out, "a   b\n    c");
    }
}
