// Ported from SoLoud src/audiosource/speech/tts.cpp, SoLoud's copy of rsynth
// by Nick Ing-Simmons et al. (public domain; SoLoud's changes public domain
// or CC0, Jari Komppa 2013); altered for SoLouder.

//! Text to phonemes (`xlate_string`).
//!
//! SoLoud's `ctype` calls get C's signed `char`s, so bytes from 0x80 pass
//! negative values (undefined behaviour). The C runtimes the reference
//! builds with give them no class in the "C" locale, and neither do these
//! ASCII versions: such bytes are not letters, digits, spaces or
//! punctuation, whatever the locale. The phoneme counts SoLoud returns are
//! never used and are not kept.

use super::rules;

pub(super) fn is_upper(b: u8) -> bool {
    b.is_ascii_uppercase()
}

fn is_alpha(b: u8) -> bool {
    b.is_ascii_alphabetic()
}

fn is_digit(b: u8) -> bool {
    b.is_ascii_digit()
}

/// C's `isspace`, which unlike `u8::is_ascii_whitespace` takes `\v`.
fn is_space(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r')
}

fn is_punct(b: u8) -> bool {
    b.is_ascii_punctuation()
}

pub(super) fn is_vowel(b: u8) -> bool {
    matches!(b, b'A' | b'E' | b'I' | b'O' | b'U')
}

/// The names `spell_out` reads for each 7-bit byte (`ASCII`).
const ASCII: [&str; 128] = [
    "null",
    "",
    "",
    "",
    "",
    "",
    "",
    "",
    "",
    "",
    "",
    "",
    "",
    "",
    "",
    "",
    "",
    "",
    "",
    "",
    "",
    "",
    "",
    "",
    "",
    "",
    "",
    "",
    "",
    "",
    "",
    "",
    "space",
    "exclamation mark",
    "double quote",
    "hash",
    "dollar",
    "percent",
    "ampersand",
    "quote",
    "open parenthesis",
    "close parenthesis",
    "asterisk",
    "plus",
    "comma",
    "minus",
    "full stop",
    "slash",
    "zero",
    "one",
    "two",
    "three",
    "four",
    "five",
    "six",
    "seven",
    "eight",
    "nine",
    "colon",
    "semi colon",
    "less than",
    "equals",
    "greater than",
    "question mark",
    "at",
    "ay",
    "bee",
    "see",
    "dee",
    "e",
    "eff",
    "gee",
    "aych",
    "i",
    "jay",
    "kay",
    "ell",
    "em",
    "en",
    "ohe",
    "pee",
    "kju",
    "are",
    "es",
    "tee",
    "you",
    "vee",
    "double you",
    "eks",
    "why",
    "zed",
    "open bracket",
    "back slash",
    "close bracket",
    "circumflex",
    "underscore",
    "back quote",
    "ay",
    "bee",
    "see",
    "dee",
    "e",
    "eff",
    "gee",
    "aych",
    "i",
    "jay",
    "kay",
    "ell",
    "em",
    "en",
    "ohe",
    "pee",
    "kju",
    "are",
    "es",
    "tee",
    "you",
    "vee",
    "double you",
    "eks",
    "why",
    "zed",
    "open brace",
    "vertical bar",
    "close brace",
    "tilde",
    "delete",
];

const CARDINALS: [&str; 20] = [
    "zero",
    "one",
    "two",
    "three",
    "four",
    "five",
    "six",
    "seven",
    "eight",
    "nine",
    "ten",
    "eleven",
    "twelve",
    "thirteen",
    "fourteen",
    "fifteen",
    "sixteen",
    "seventeen",
    "eighteen",
    "nineteen",
];

const TWENTIES: [&str; 8] = [
    "twenty", "thirty", "forty", "fifty", "sixty", "seventy", "eighty", "ninety",
];

/// A number in words (`xlate_cardinal`).
fn xlate_cardinal(value: i32, phone: &mut Vec<u8>) {
    let mut value = value;
    if value < 0 {
        xlate_string(b"minus", phone);
        value = value.wrapping_neg();
        if value < 0 {
            xlate_string(b"a lot", phone);
            return;
        }
    }
    for (unit, name) in [(1_000_000_000, "billion"), (1_000_000, "million")] {
        if value >= unit {
            xlate_cardinal(value / unit, phone);
            xlate_string(name.as_bytes(), phone);
            value %= unit;
            if value == 0 {
                return;
            }
            if value < 100 {
                xlate_string(b"and", phone);
            }
        }
    }
    // 1100 to 1999 are eleven hundred to nineteen hundred.
    if (1000..=1099).contains(&value) || value >= 2000 {
        xlate_cardinal(value / 1000, phone);
        xlate_string(b"thousand", phone);
        value %= 1000;
        if value == 0 {
            return;
        }
        if value < 100 {
            xlate_string(b"and", phone);
        }
    }
    if value >= 100 {
        xlate_string(CARDINALS[(value / 100) as usize].as_bytes(), phone);
        xlate_string(b"hundred", phone);
        value %= 100;
        if value == 0 {
            return;
        }
    }
    if value >= 20 {
        xlate_string(TWENTIES[((value - 20) / 10) as usize].as_bytes(), phone);
        value %= 10;
        if value == 0 {
            return;
        }
    }
    xlate_string(CARDINALS[value as usize].as_bytes(), phone);
}

/// Each byte's name (`spell_out`).
fn spell_out(word: &[u8], phone: &mut Vec<u8>) {
    for &b in word {
        xlate_string(ASCII[usize::from(b & 0x7f)].as_bytes(), phone);
    }
}

/// Whether `word` should be spelled out rather than read: no vowel, mixed
/// case, or no lower case (`suspect_word`).
fn suspect_word(word: &[u8]) -> bool {
    let mut seen_lower = false;
    let mut seen_upper = false;
    let mut seen_vowel = false;
    let mut last = 0;
    for (i, &b) in word.iter().enumerate() {
        let mut ch = b;
        if i != 0 && last != b'-' && is_upper(ch) {
            seen_upper = true;
        }
        if ch.is_ascii_lowercase() {
            seen_lower = true;
            ch = ch.to_ascii_uppercase();
        }
        if is_vowel(ch) || ch == b'Y' {
            seen_vowel = true;
        }
        last = ch;
    }
    !seen_vowel || (seen_upper && seen_lower) || !seen_lower
}

/// A word through the rules (`NRL`).
fn nrl(word: &[u8], phone: &mut Vec<u8>) {
    let mut buffer = Vec::with_capacity(word.len() + 3);
    buffer.push(b' ');
    buffer.extend(word.iter().map(u8::to_ascii_uppercase));
    buffer.push(b' ');
    buffer.push(0);
    rules::guess_word(phone, &buffer);
}

/// A word, then a space (`xlate_word`): `[phonemes]` are copied, suspect
/// words spelled out (with no space after, as in SoLoud), others read.
fn xlate_word(word: &[u8], phone: &mut Vec<u8>) {
    if word.first() != Some(&b'[') {
        if suspect_word(word) {
            spell_out(word, phone);
            return;
        }
        nrl(word, phone);
    } else {
        let mut inner = &word[1..];
        if inner.last() == Some(&b']') {
            inner = &inner[..inner.len() - 1];
        }
        phone.extend_from_slice(inner);
    }
    phone.push(b' ');
}

/// Appends the phonemes of `string`, which ends at its first 0 or its end
/// (`xlate_string`).
pub(super) fn xlate_string(string: &[u8], phone: &mut Vec<u8>) {
    let at = |i: usize| string.get(i).copied().unwrap_or(0);
    let mut s = 0;
    while is_space(at(s)) {
        s += 1;
    }
    while at(s) != 0 {
        let ch = at(s);
        let word = s;
        if is_alpha(ch) {
            while is_alpha(at(s)) || (matches!(at(s), b'\'' | b'-' | b'.') && is_alpha(at(s + 1))) {
                s += 1;
            }
            let ch = at(s);
            if ch == 0
                || is_space(ch)
                || is_punct(ch)
                || (is_digit(ch) && !suspect_word(&string[word..s]))
            {
                xlate_word(&string[word..s], phone);
            } else {
                while at(s) != 0 && !is_space(at(s)) && !is_punct(at(s)) {
                    s += 1;
                }
                spell_out(&string[word..s], phone);
            }
            continue;
        }
        if is_digit(ch) || (ch == b'-' && is_digit(at(s + 1))) {
            let sign: i32 = if ch == b'-' { -1 } else { 1 };
            if sign < 0 {
                s += 1;
            }
            // Wrapping, as the reference's MSVC build does.
            let mut value: i32 = 0;
            while is_digit(at(s)) {
                value = value
                    .wrapping_mul(10)
                    .wrapping_add(i32::from(at(s)) - i32::from(b'0'));
                s += 1;
            }
            xlate_cardinal(value.wrapping_mul(sign), phone);
            if at(s) == b'.' && is_digit(at(s + 1)) {
                s += 1;
                let fraction = s;
                xlate_string(b"point", phone);
                while is_digit(at(s)) {
                    s += 1;
                }
                spell_out(&string[fraction..s], phone);
            }
        } else if ch == b'['
            && string[s..]
                .iter()
                .take_while(|&&b| b != 0)
                .any(|&b| b == b']')
        {
            while at(s) != 0 {
                s += 1;
                if at(s - 1) == b']' {
                    break;
                }
            }
            xlate_word(&string[word..s], phone);
        } else if is_punct(ch) {
            s += 1;
            match ch {
                b'!' | b'?' | b'.' => phone.push(b'.'),
                b'"' | b':' | b'-' | b';' | b',' | b'(' | b')' => phone.push(b' '),
                // `[` without `]` too.
                _ => spell_out(&string[word..word + 1], phone),
            }
        } else {
            while at(s) != 0 && !is_space(at(s)) {
                s += 1;
            }
            spell_out(&string[word..s], phone);
        }
        while is_space(at(s)) {
            s += 1;
        }
    }
}
