//! A tokenizer for the C tables in `data/`, copied as they are from SoLoud:
//! comments are skipped, string literals keep their raw contents (escapes
//! are not decoded; the tables read have none they use), and commas are
//! dropped.

/// One token of a C table.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Token {
    /// A string literal's contents, between its quotes.
    Str(&'static str),
    /// An identifier, a number or a `|` expression of identifiers.
    Word(&'static str),
    /// `{`, `}`, `[`, `]`, `=` or `;`.
    Punct(u8),
}

fn is_word_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b'-' | b'+' | b'|')
}

/// The tokens of `src`.
///
/// # Panics
///
/// On an unterminated comment or string, or a byte no table uses; the
/// tables are compiled in, and tests read them.
pub(super) fn tokens(src: &'static str) -> Vec<Token> {
    let bytes = src.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if b.is_ascii_whitespace() || b == b',' {
            i += 1;
        } else if bytes[i..].starts_with(b"/*") {
            let end = src[i + 2..].find("*/").expect("unterminated comment");
            i += 2 + end + 2;
        } else if b == b'"' {
            let start = i + 1;
            let mut j = start;
            while bytes[j] != b'"' {
                j += if bytes[j] == b'\\' { 2 } else { 1 };
            }
            out.push(Token::Str(&src[start..j]));
            i = j + 1;
        } else if is_word_byte(b) {
            let start = i;
            while i < bytes.len() && is_word_byte(bytes[i]) {
                i += 1;
            }
            out.push(Token::Word(&src[start..i]));
        } else if matches!(b, b'{' | b'}' | b'[' | b']' | b'=' | b';') {
            out.push(Token::Punct(b));
            i += 1;
        } else {
            panic!("unexpected byte {b:#04x} in a speech table");
        }
    }
    out
}

/// A C integer literal: decimal, or hexadecimal after `0x`.
///
/// # Panics
///
/// If `word` is neither.
pub(super) fn int(word: &str) -> i32 {
    let parsed = match word.strip_prefix("0x") {
        Some(hex) => i32::from_str_radix(hex, 16),
        None => word.parse(),
    };
    parsed.unwrap_or_else(|_| panic!("`{word}` is not an integer"))
}

/// A C `double` literal stored in a `float`.
///
/// # Panics
///
/// If `word` is not a number.
pub(super) fn float(word: &str) -> f32 {
    let value: f64 = word
        .parse()
        .unwrap_or_else(|_| panic!("`{word}` is not a number"));
    value as f32
}
