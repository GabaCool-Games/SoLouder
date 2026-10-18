// Ported from SoLoud src/audiosource/speech/tts.cpp, SoLoud's copy of rsynth
// by Nick Ing-Simmons et al. (public domain; SoLoud's changes public domain
// or CC0, Jari Komppa 2013); altered for SoLouder.

//! The letter-to-sound rules (read from `data/rules.inc`) and their
//! matching (`leftmatch`, `rightmatch`, `find_rule`, `guess_word`).

use std::sync::LazyLock;

use super::cdata::{self, Token};
use super::tts::{is_upper, is_vowel};

/// A rule: `out` replaces `matched` between the `left` and `right`
/// contexts. The last rule of each group has no `matched`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Rule {
    pub(super) left: &'static str,
    pub(super) matched: Option<&'static str>,
    pub(super) right: &'static str,
    pub(super) out: &'static str,
}

/// Punctuation, then `A` to `Z` (`Rules`).
const GROUPS: usize = 27;

fn group_index(name: &str) -> usize {
    if name == "punct_rules" {
        return 0;
    }
    let letter = name.as_bytes()[0];
    assert!(
        letter.is_ascii_uppercase() && &name[1..] == "_rules",
        "unexpected group `{name}`"
    );
    usize::from(letter - b'A') + 1
}

fn part(token: Token) -> Option<&'static str> {
    match token {
        Token::Str(text) => Some(text),
        Token::Word("Anything" | "Silent") => Some(""),
        Token::Word("Nothing") => Some(" "),
        Token::Word("0") => None,
        other => panic!("unexpected {other:?} in a rule"),
    }
}

fn parse(src: &'static str) -> Vec<Vec<Rule>> {
    let tokens = cdata::tokens(src);
    let mut groups = vec![Vec::new(); GROUPS];
    let mut i = 0;
    while i < tokens.len() {
        // static Rule <name>[] = { {left, match, right, out}, ... };
        let Token::Word(name) = tokens[i + 2] else {
            panic!("expected a group name");
        };
        let group = &mut groups[group_index(name)];
        i += 7;
        while tokens[i] == Token::Punct(b'{') {
            let rule = Rule {
                left: part(tokens[i + 1]).expect("a left context"),
                matched: part(tokens[i + 2]),
                right: part(tokens[i + 3]).expect("a right context"),
                out: part(tokens[i + 4]).expect("an output"),
            };
            assert_eq!(tokens[i + 5], Token::Punct(b'}'));
            group.push(rule);
            i += 6;
        }
        assert_eq!(tokens[i], Token::Punct(b'}'));
        assert_eq!(tokens[i + 1], Token::Punct(b';'));
        i += 2;
    }
    groups
}

static RULES: LazyLock<Vec<Vec<Rule>>> = LazyLock::new(|| parse(include_str!("data/rules.inc")));

/// The rule groups, punctuation first.
pub(super) fn groups() -> &'static [Vec<Rule>] {
    &RULES
}

fn is_consonant(b: u8) -> bool {
    is_upper(b) && !is_vowel(b)
}

fn is_voiced(b: u8) -> bool {
    matches!(
        b,
        b'B' | b'D' | b'V' | b'G' | b'J' | b'L' | b'M' | b'N' | b'R' | b'W' | b'Z'
    )
}

fn is_front(b: u8) -> bool {
    matches!(b, b'E' | b'I' | b'Y')
}

/// The byte at `i` of `word`, 0 outside it.
fn at(word: &[u8], i: isize) -> u8 {
    usize::try_from(i)
        .ok()
        .and_then(|i| word.get(i))
        .copied()
        .unwrap_or(0)
}

fn is_literal(b: u8) -> bool {
    b.is_ascii_alphabetic() || b == b'\'' || b == b' '
}

/// Whether `pattern` matches `word` backwards from `context`.
fn left_match(pattern: &str, word: &[u8], context: isize) -> bool {
    let mut text = context;
    for &pat in pattern.as_bytes().iter().rev() {
        if is_literal(pat) {
            if pat != at(word, text) {
                return false;
            }
            text -= 1;
            continue;
        }
        match pat {
            b'#' => {
                if !is_vowel(at(word, text)) {
                    return false;
                }
                text -= 1;
                while is_vowel(at(word, text)) {
                    text -= 1;
                }
            }
            b':' => {
                while is_consonant(at(word, text)) {
                    text -= 1;
                }
            }
            b'^' | b'.' | b'+' => {
                let b = at(word, text);
                let ok = match pat {
                    b'^' => is_consonant(b),
                    b'.' => is_voiced(b),
                    _ => is_front(b),
                };
                if !ok {
                    return false;
                }
                text -= 1;
            }
            // SoLoud prints "Bad char in left rule" to stderr.
            _ => return false,
        }
    }
    true
}

/// Whether `pattern` matches `word` forwards from `context`.
fn right_match(pattern: &str, word: &[u8], context: isize) -> bool {
    let mut text = context;
    for &pat in pattern.as_bytes() {
        if is_literal(pat) {
            if pat != at(word, text) {
                return false;
            }
            text += 1;
            continue;
        }
        match pat {
            b'#' => {
                if !is_vowel(at(word, text)) {
                    return false;
                }
                text += 1;
                while is_vowel(at(word, text)) {
                    text += 1;
                }
            }
            b':' => {
                while is_consonant(at(word, text)) {
                    text += 1;
                }
            }
            b'^' | b'.' | b'+' => {
                let b = at(word, text);
                let ok = match pat {
                    b'^' => is_consonant(b),
                    b'.' => is_voiced(b),
                    _ => is_front(b),
                };
                if !ok {
                    return false;
                }
                text += 1;
            }
            // A suffix: ER, E, ES, ED, ING or ELY.
            b'%' => match at(word, text) {
                b'E' => {
                    text += 1;
                    match at(word, text) {
                        b'L' => {
                            text += 1;
                            if at(word, text) == b'Y' {
                                text += 1;
                            } else {
                                text -= 1; // don't gobble the L
                            }
                        }
                        b'R' | b'S' | b'D' => text += 1,
                        _ => {}
                    }
                }
                b'I' if at(word, text + 1) == b'N' && at(word, text + 2) == b'G' => text += 3,
                _ => return false,
            },
            // SoLoud prints "Bad char in right rule" to stderr.
            _ => return false,
        }
    }
    true
}

/// Applies the first rule of `rules` that matches `word` at `index`,
/// appending its output; returns where the next match starts
/// (`find_rule`). SoLoud prints an error and skips a byte no rule matches.
fn find_rule(phone: &mut Vec<u8>, word: &[u8], index: usize, rules: &[Rule]) -> usize {
    for rule in rules {
        let Some(matched) = rule.matched else {
            return index + 1;
        };
        let mut remainder = index;
        let mut ok = true;
        for &m in matched.as_bytes() {
            if m != at(word, remainder as isize) {
                ok = false;
                break;
            }
            remainder += 1;
        }
        if !ok
            || !left_match(rule.left, word, index as isize - 1)
            || !right_match(rule.right, word, remainder as isize)
        {
            continue;
        }
        phone.extend_from_slice(rule.out.as_bytes());
        return remainder;
    }
    index + 1
}

/// Appends the phonemes of `word`, a space, upper-case bytes, a space and
/// a 0 (`guess_word`).
pub(super) fn guess_word(phone: &mut Vec<u8>, word: &[u8]) {
    let groups = groups();
    let mut index = 1; // skip the initial blank
    loop {
        let b = at(word, index as isize);
        let group = if is_upper(b) {
            usize::from(b - b'A') + 1
        } else {
            0
        };
        index = find_rule(phone, word, index, &groups[group]);
        if at(word, index as isize) == 0 {
            break;
        }
    }
}
