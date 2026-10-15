// Ported from stb_vorbis 1.22 by Sean Barrett (public domain or MIT);
// altered for SoLouder.

//! Codebooks and their Huffman tables: `Codebook`, `compute_codewords`,
//! `compute_accelerated_huffman` and `compute_sorted_huffman`, as stb
//! builds them with its defaults (`FAST_HUFFMAN_SHORT`, a fast table of 10
//! bits, binary search for longer codes).

use super::tables::bit_reverse;

/// `STB_VORBIS_FAST_HUFFMAN_LENGTH`.
pub(super) const FAST_HUFFMAN_LENGTH: u8 = 10;
/// `FAST_HUFFMAN_TABLE_SIZE`.
pub(super) const FAST_HUFFMAN_TABLE_SIZE: usize = 1 << FAST_HUFFMAN_LENGTH;
/// `FAST_HUFFMAN_TABLE_MASK`.
pub(super) const FAST_HUFFMAN_TABLE_MASK: u32 = FAST_HUFFMAN_TABLE_SIZE as u32 - 1;
/// `NO_CODE`: the length of an entry without a code.
pub(super) const NO_CODE: u8 = 255;

/// `Codebook`. An empty `Vec` stands for stb's null pointer.
pub(super) struct Codebook {
    pub dimensions: i32,
    pub entries: i32,
    /// Lengths by symbol, or by sorted index when sparse.
    pub codeword_lengths: Vec<u8>,
    pub minimum_value: f32,
    pub delta_value: f32,
    pub value_bits: u8,
    pub lookup_type: u8,
    pub sequence_p: bool,
    pub sparse: bool,
    pub lookup_values: u32,
    pub multiplicands: Vec<f32>,
    pub codewords: Vec<u32>,
    pub fast_huffman: Box<[i16; FAST_HUFFMAN_TABLE_SIZE]>,
    pub sorted_codewords: Vec<u32>,
    /// `sorted_values` with the slot stb keeps before it: index `x + 1`
    /// holds `sorted_values[x]`, index 0 the -1 of `sorted_values[-1]`.
    pub sorted_values: Vec<i32>,
    pub sorted_entries: i32,
}

impl Codebook {
    /// A zeroed codebook, as stb's `memset` leaves it.
    pub fn new() -> Self {
        Self {
            dimensions: 0,
            entries: 0,
            codeword_lengths: Vec::new(),
            minimum_value: 0.0,
            delta_value: 0.0,
            value_bits: 0,
            lookup_type: 0,
            sequence_p: false,
            sparse: false,
            lookup_values: 0,
            multiplicands: Vec::new(),
            codewords: Vec::new(),
            fast_huffman: Box::new([0; FAST_HUFFMAN_TABLE_SIZE]),
            sorted_codewords: Vec::new(),
            sorted_values: Vec::new(),
            sorted_entries: 0,
        }
    }

    /// `c->sorted_values[x]`, -1 for x = -1.
    #[inline]
    pub fn sorted_value(&self, x: i32) -> i32 {
        self.sorted_values
            .get((x + 1) as usize)
            .copied()
            .unwrap_or(-1)
    }

    fn add_entry(
        &mut self,
        huff_code: u32,
        symbol: usize,
        count: usize,
        len: u8,
        values: &mut [u32],
    ) {
        if !self.sparse {
            self.codewords[symbol] = huff_code;
        } else {
            self.codewords[count] = huff_code;
            self.codeword_lengths[count] = len;
            values[count] = symbol as u32;
        }
    }

    /// `compute_codewords`: assigns codes in symbol order to the lengths in
    /// `len`; false for an overspecified tree.
    pub fn compute_codewords(&mut self, len: &[u8], values: &mut [u32]) -> bool {
        let mut available = [0_u32; 32];
        let mut m = 0;
        // find the first entry
        let Some(k) = len.iter().position(|&l| l < NO_CODE) else {
            return true;
        };
        // lengths are 1 to 31 here: reading them rejects longer ones
        self.add_entry(0, k, m, len[k], values);
        m += 1;
        // add all available leaves
        for (i, slot) in available
            .iter_mut()
            .enumerate()
            .take(usize::from(len[k]) + 1)
            .skip(1)
        {
            *slot = 1 << (32 - i);
        }
        for (i, &li) in len.iter().enumerate().skip(k + 1) {
            if li == NO_CODE {
                continue;
            }
            let mut z = usize::from(li);
            // find lowest available leaf (should always be earliest,
            // which is what the specification calls for)
            while z > 0 && available[z] == 0 {
                z -= 1;
            }
            if z == 0 {
                return false;
            }
            let res = available[z];
            available[z] = 0;
            self.add_entry(bit_reverse(res), i, m, li, values);
            m += 1;
            // propagate availability up the tree
            for y in (z + 1..=usize::from(li)).rev() {
                available[y] = res.wrapping_add(1_u32 << (32 - y));
            }
        }
        true
    }

    /// `compute_accelerated_huffman`.
    pub fn compute_accelerated_huffman(&mut self) {
        self.fast_huffman.fill(-1);
        let len = if self.sparse {
            self.sorted_entries
        } else {
            self.entries
        };
        // largest possible value we can encode!
        let len = len.min(32767);
        for i in 0..len as usize {
            let length = self.codeword_lengths[i];
            if length <= FAST_HUFFMAN_LENGTH {
                let mut z = if self.sparse {
                    bit_reverse(self.sorted_codewords[i])
                } else {
                    self.codewords[i]
                };
                // set table entries for all bit combinations in the higher bits
                while (z as usize) < FAST_HUFFMAN_TABLE_SIZE {
                    self.fast_huffman[z as usize] = i as i16;
                    z += 1 << length;
                }
            }
        }
    }

    fn include_in_sort(&self, len: u8) -> bool {
        if self.sparse {
            return true;
        }
        len != NO_CODE && len > FAST_HUFFMAN_LENGTH
    }

    /// The binary search of `compute_sorted_huffman` and
    /// `codebook_decode_scalar_raw`: the last sorted index whose code is at
    /// most `code`.
    #[inline]
    pub fn search_sorted(&self, code: u32) -> i32 {
        let (mut x, mut n) = (0_i32, self.sorted_entries);
        while n > 1 {
            // invariant: sc[x] <= code < sc[x+n]
            let m = x + (n >> 1);
            if self.sorted_codewords[m as usize] <= code {
                x = m;
                n -= n >> 1;
            } else {
                n >>= 1;
            }
        }
        x
    }

    /// `compute_sorted_huffman`: the codes for binary search, bit-reversed
    /// and sorted, and what each one decodes to.
    pub fn compute_sorted_huffman(&mut self, lengths: &[u8], values: &[u32]) {
        let sorted = self.sorted_entries as usize;
        if !self.sparse {
            let mut k = 0;
            for (i, &len) in lengths.iter().enumerate().take(self.entries as usize) {
                if self.include_in_sort(len) {
                    self.sorted_codewords[k] = bit_reverse(self.codewords[i]);
                    k += 1;
                }
            }
        } else {
            for i in 0..sorted {
                self.sorted_codewords[i] = bit_reverse(self.codewords[i]);
            }
        }
        // Codes are distinct, so any sort gives qsort's order.
        self.sorted_codewords[..sorted].sort_unstable();
        self.sorted_codewords[sorted] = 0xffff_ffff;

        let len = if self.sparse {
            sorted
        } else {
            self.entries as usize
        };
        for i in 0..len {
            let huff_len = if self.sparse {
                lengths[values[i] as usize]
            } else {
                lengths[i]
            };
            if self.include_in_sort(huff_len) {
                let code = bit_reverse(self.codewords[i]);
                let x = self.search_sorted(code) as usize;
                if self.sparse {
                    self.sorted_values[x + 1] = values[i] as i32;
                    self.codeword_lengths[x] = huff_len;
                } else {
                    self.sorted_values[x + 1] = i as i32;
                }
            }
        }
    }
}
