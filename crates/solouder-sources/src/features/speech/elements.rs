// Ported from SoLoud src/audiosource/speech/klatt.cpp, SoLoud's copy of
// rsynth by Nick Ing-Simmons et al. (public domain; SoLoud's changes public
// domain or CC0, Jari Komppa 2013); altered for SoLouder.

//! The speech elements (`Elements.def`, read from `data/elements.def`) and
//! phonemes to elements (`klatt::phone_to_elm`).

use std::sync::LazyLock;

use super::cdata::{self, Token};

/// Parameters each element sets, in `Interp` order (`Eparm_e`).
pub(super) const PARAMS: usize = 19;
pub(super) const FN: usize = 0;
pub(super) const F1: usize = 1;
pub(super) const F2: usize = 2;
pub(super) const F3: usize = 3;
pub(super) const B1: usize = 4;
pub(super) const B2: usize = 5;
pub(super) const B3: usize = 6;
pub(super) const A1: usize = 8;
pub(super) const A2: usize = 9;
pub(super) const A3: usize = 10;
pub(super) const A4: usize = 11;
pub(super) const A5: usize = 12;
pub(super) const A6: usize = 13;
pub(super) const AB: usize = 14;
pub(super) const AV: usize = 15;
pub(super) const AVC: usize = 16;
pub(super) const ASP: usize = 17;
pub(super) const AF: usize = 18;

/// How one parameter moves into and out of an element (`Interp`).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(super) struct Interp {
    pub(super) steady: f32,
    pub(super) fixed: f32,
    /// Percent of the other element's steady value.
    pub(super) proportion: i32,
    pub(super) ext_delay: i32,
    pub(super) int_delay: i32,
}

/// One element (`Element`); only the fields klatt reads.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Element {
    pub(super) name: &'static str,
    /// Rank: the higher sets the transition between two elements.
    pub(super) rank: i32,
    pub(super) du: i32,
    pub(super) ud: i32,
    pub(super) vowel: bool,
    pub(super) interp: [Interp; PARAMS],
}

/// A C `char` field: the literal's low byte, signed.
fn char_field(word: &str) -> i32 {
    i32::from(cdata::int(word) as i8)
}

fn parse(src: &'static str) -> Vec<Element> {
    let tokens = cdata::tokens(src);
    let mut words = tokens.iter().filter_map(|token| match token {
        Token::Str(text) | Token::Word(text) => Some(*text),
        Token::Punct(_) => None,
    });
    let mut out = Vec::new();
    while let Some(name) = words.next() {
        let mut next = || words.next().expect("a cut-short element");
        let rank = char_field(next());
        let du = char_field(next());
        let ud = char_field(next());
        let _font = next();
        let _dict = next();
        let _ipa = next();
        let vowel = next().split('|').any(|flag| flag == "ELM_FEATURE_VWL");
        let interp = std::array::from_fn(|_| Interp {
            steady: cdata::float(next()),
            fixed: cdata::float(next()),
            proportion: char_field(next()),
            ext_delay: char_field(next()),
            int_delay: char_field(next()),
        });
        out.push(Element {
            name,
            rank,
            du,
            ud,
            vowel,
            interp,
        });
    }
    out
}

static ELEMENTS: LazyLock<Vec<Element>> =
    LazyLock::new(|| parse(include_str!("data/elements.def")));

/// Every element, `END` first (`gElement`).
pub(super) fn elements() -> &'static [Element] {
    &ELEMENTS
}

/// The longest an element plays, in frames: `(mDU + mUD) / 2` at most.
pub(super) fn longest() -> usize {
    elements()
        .iter()
        .map(|e| ((e.du + e.ud) / 2).max(0) as usize)
        .max()
        .unwrap_or(0)
}

/// Phonemes and their elements (`phoneme_to_elements`), two-byte phonemes
/// first, as the lookup needs.
const PHONEMES: [(&str, &[&str]); 53] = [
    ("tS", &["CH", "CI"]),
    ("dZ", &["J", "JY", "QQ", "JY"]),
    ("rr", &["R", "QQ", "R"]),
    ("eI", &["AI", "I"]),
    ("aI", &["IE", "I"]),
    ("oI", &["OI", "I"]),
    ("aU", &["OU", "OV"]),
    ("@U", &["OA", "OV"]),
    ("I@", &["IA", "IB"]),
    ("e@", &["AIR", "IB"]),
    ("U@", &["OOR", "IB"]),
    ("O@", &["OR", "IB"]),
    ("oU", &["OI", "OV"]),
    (" ", &["Q"]),
    ("p", &["P", "PY", "PZ"]),
    ("t", &["T", "TY", "TZ"]),
    ("k", &["K", "KY", "KZ"]),
    ("b", &["B", "BY", "BZ"]),
    ("d", &["D", "DY", "DZ"]),
    ("g", &["G", "GY", "GZ"]),
    ("m", &["M"]),
    ("n", &["N"]),
    ("N", &["NG"]),
    ("f", &["F"]),
    ("T", &["TH"]),
    ("s", &["S"]),
    ("S", &["SH"]),
    ("h", &["H"]),
    ("v", &["V", "QQ", "V"]),
    ("D", &["DH", "QQ", "DI"]),
    ("z", &["Z", "QQ", "ZZ"]),
    ("Z", &["ZH", "QQ", "ZH"]),
    ("l", &["L"]),
    ("r", &["R"]),
    ("R", &["RX"]),
    ("w", &["W"]),
    ("x", &["X"]),
    ("%", &["QQ"]),
    ("j", &["Y"]),
    ("I", &["I"]),
    ("e", &["E"]),
    ("&", &["AA"]),
    ("V", &["U"]),
    ("0", &["O"]),
    ("U", &["OO"]),
    ("@", &["A"]),
    ("i", &["EE"]),
    ("3", &["ER"]),
    ("A", &["AR"]),
    ("O", &["AW"]),
    ("u", &["UU"]),
    ("o", &["OI"]),
    (".", &["END"]),
];

/// A phoneme's key: its first byte, plus its second shifted left by 8.
#[derive(Debug)]
struct Phoneme {
    key: i32,
    elements: Vec<u8>,
}

static PHONEME_TABLE: LazyLock<Vec<Phoneme>> = LazyLock::new(|| {
    let index = |name: &str| {
        let at = elements().iter().position(|e| e.name == name);
        at.expect("a phoneme names an element") as u8
    };
    PHONEMES
        .iter()
        .map(|(text, names)| {
            let b = text.as_bytes();
            let key = i32::from(b[0]) + (i32::from(b.get(1).copied().unwrap_or(0)) << 8);
            let elements = names.iter().map(|name| index(name)).collect();
            Phoneme { key, elements }
        })
        .collect()
});

/// The phoneme at `s`, and where the next starts
/// (`phoneme_to_element_lookup`). A byte past `phonemes` reads as 0
/// (reference patch 0096).
fn lookup(phonemes: &[u8], s: usize) -> (usize, Option<&'static [u8]>) {
    // C's `char` is signed: bytes from 0x80 give negative keys, which no
    // phoneme has.
    let key8 = i32::from(phonemes[s] as i8);
    let next = i32::from(phonemes.get(s + 1).copied().unwrap_or(0) as i8);
    let key16 = if next == 0 {
        -1
    } else {
        key8.wrapping_add(next.wrapping_shl(8))
    };
    for phoneme in PHONEME_TABLE.iter() {
        if phoneme.key == key16 {
            return (s + 2, Some(&phoneme.elements));
        }
        if phoneme.key == key8 {
            return (s + 1, Some(&phoneme.elements));
        }
    }
    (s + 1, None)
}

/// Appends each element of `phonemes` with its duration and stress
/// (`klatt::phone_to_elm`); returns the frames they play. Phonemes end at
/// the first 0. `'`, `,` and `+` set the stress of the vowel after them
/// (reference patch 0097); other unknown bytes are skipped.
pub(super) fn phone_to_elm(phonemes: &[u8], out: &mut Vec<u8>) -> i32 {
    let table = elements();
    let mut stress = 0;
    let mut frames = 0;
    let mut s = 0;
    while s < phonemes.len() && phonemes[s] != 0 {
        let (next, found) = lookup(phonemes, s);
        match found {
            Some(list) => {
                for &x in list {
                    let element = &table[usize::from(x)];
                    out.push(x);
                    if !element.vowel {
                        stress = 0;
                    }
                    // #337: the duration ignores the stress, as in SoLoud.
                    let duration = (element.du + element.ud) / 2;
                    frames += duration;
                    out.push(duration as u8);
                    out.push(stress);
                }
            }
            None => match phonemes[s] {
                b'\'' => stress = 3,
                b',' => stress = 2,
                b'+' => stress = 1,
                _ => {}
            },
        }
        s = next;
    }
    frames
}
