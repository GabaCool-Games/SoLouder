// Ported from stb_vorbis 1.22 by Sean Barrett (public domain or MIT);
// altered for SoLouder.

//! The decoder's configuration from the setup header (`Floor1`, `Residue`,
//! `Mapping`, `Mode` and the per-blocksize tables), stb's allocation rules,
//! and reading codebooks (the codebook part of `start_decoder`).

use super::codebook::{Codebook, FAST_HUFFMAN_LENGTH, NO_CODE};
use super::ogg::{Reader, VorbisError};
use super::source::Source;
use super::tables::{float32_unpack, ilog, lookup1_values};

/// The most memory a setup header may ask for. stb allocates whatever the
/// header says, up to 2 GiB a table; a few bytes of header can ask for
/// that much, so SoLouder refuses more than this, far beyond what encoders
/// write (libvorbis's setups take well under a megabyte).
pub(super) const SETUP_BUDGET: u64 = 256 << 20;

/// `Floor1` (stb rejects floor type 0 while reading the setup).
pub(super) struct Floor1 {
    pub partitions: u8,
    pub partition_class_list: [u8; 32],
    pub class_dimensions: [u8; 16],
    pub class_subclasses: [u8; 16],
    pub class_masterbooks: [u8; 16],
    pub subclass_books: [[i16; 8]; 16],
    pub xlist: [u16; 31 * 8 + 2],
    pub sorted_order: [u8; 31 * 8 + 2],
    pub neighbors: [[u8; 2]; 31 * 8 + 2],
    pub floor1_multiplier: u8,
    pub rangebits: u8,
    pub values: i32,
}

/// `Residue`, with its type (`residue_types`).
pub(super) struct Residue {
    pub rtype: u16,
    pub begin: u32,
    pub end: u32,
    pub part_size: u32,
    pub classbook: u8,
    /// `classdata[j][k]` at `j * classwords + k`.
    pub classdata: Vec<u8>,
    pub residue_books: Vec<[i16; 8]>,
}

/// `MappingChannel`.
#[derive(Clone, Copy, Default)]
pub(super) struct MappingChannel {
    pub magnitude: u8,
    pub angle: u8,
    pub mux: u8,
}

/// `Mapping`. Its `submap_floor[15]` and `submap_residue[15]` are laid out
/// as MSVC lays out the struct, because up to 16 submaps are read into
/// them: `submap_floor[15]` is `submap_residue[0]`, and `submap_residue[15]`
/// is the padding byte after the struct's last field.
pub(super) struct Mapping {
    pub coupling_steps: u16,
    pub chan: Vec<MappingChannel>,
    pub submaps: u8,
    pub submap: [u8; 31],
}

impl Mapping {
    pub fn submap_floor(&self, j: usize) -> usize {
        usize::from(self.submap[j])
    }

    pub fn submap_residue(&self, j: usize) -> usize {
        usize::from(self.submap[15 + j])
    }
}

/// `Mode`.
#[derive(Clone, Copy, Default)]
pub(super) struct Mode {
    pub blockflag: u8,
    pub mapping: u8,
    pub windowtype: u16,
    pub transformtype: u16,
}

/// The tables of one blocksize (`init_blocksize`): twiddle factors `A`,
/// `B` and `C`, the window and the bit-reverse table.
pub(super) struct BlockTables {
    pub a: Vec<f32>,
    pub b: Vec<f32>,
    pub c: Vec<f32>,
    pub window: Vec<f32>,
    pub bit_reverse: Vec<u16>,
}

/// Everything `start_decoder` reads.
pub(super) struct Setup {
    pub channels: i32,
    pub sample_rate: u32,
    pub blocksize: [i32; 2],
    pub codebooks: Vec<Codebook>,
    pub floors: Vec<Floor1>,
    pub residues: Vec<Residue>,
    pub mappings: Vec<Mapping>,
    pub modes: Vec<Mode>,
    pub tables: [BlockTables; 2],
    pub longest_floorlist: usize,
}

/// Whether `setup_malloc(sz)` returns memory: `sz` passes through `int`
/// and is rounded up to 8, and zero or negative sizes give null. Sizes it
/// would truncate (undefined behaviour afterwards) count as failures.
pub(super) fn setup_malloc_ok(sz: u64) -> bool {
    sz > 0 && sz <= 0x7fff_fff8
}

/// The same for `setup_temp_malloc`, which calls `malloc(0)` (not null with
/// MSVC) for a size of 0.
pub(super) fn setup_temp_malloc_ok(sz: u64) -> bool {
    sz <= 0x7fff_fff8
}

/// Counts the setup's memory against [`SETUP_BUDGET`].
pub(super) struct Budget(pub u64);

impl Budget {
    /// Takes `bytes` for a table whose allocation stb would attempt.
    pub fn take(&mut self, bytes: u64) -> Result<(), VorbisError> {
        self.0 = self.0.saturating_add(bytes);
        if self.0 > SETUP_BUDGET {
            return Err(VorbisError::SetupTooLarge);
        }
        Ok(())
    }

    /// `setup_malloc` of `bytes`: fails as stb fails, then as the budget does.
    pub fn malloc(&mut self, bytes: u64) -> Result<(), VorbisError> {
        if !setup_malloc_ok(bytes) {
            return Err(VorbisError::OutOfMem);
        }
        self.take(bytes)
    }
}

/// One codebook of the setup header.
pub(super) fn read_codebook<S: Source>(
    f: &mut Reader<S>,
    budget: &mut Budget,
) -> Result<Codebook, VorbisError> {
    let mut c = Codebook::new();
    if f.get_bits(8) != 0x42 || f.get_bits(8) != 0x43 || f.get_bits(8) != 0x56 {
        return Err(VorbisError::InvalidSetup);
    }
    let x = f.get_bits(8);
    c.dimensions = ((f.get_bits(8) << 8) + x) as i32;
    let x = f.get_bits(8);
    let y = f.get_bits(8);
    c.entries = ((f.get_bits(8) << 16) + (y << 8) + x) as i32;
    let ordered = f.get_bits(1) != 0;
    c.sparse = !ordered && f.get_bits(1) != 0;

    if c.dimensions == 0 && c.entries != 0 {
        return Err(VorbisError::InvalidSetup);
    }
    let entries = c.entries as usize;
    // An empty table is a null `setup_malloc`, for the lengths or for the
    // non-sparse copy that an empty sparse codebook is converted to.
    if entries == 0 {
        return Err(VorbisError::OutOfMem);
    }
    budget.take(entries as u64)?;
    let mut lengths = vec![0_u8; entries];
    let mut total = 0_i32;
    if ordered {
        let mut current_entry = 0_i32;
        let mut current_length = f.get_bits(5) as i32 + 1;
        while current_entry < c.entries {
            let limit = c.entries - current_entry;
            let n = f.get_bits(ilog(limit)) as i32;
            if current_length >= 32 {
                return Err(VorbisError::InvalidSetup);
            }
            if current_entry + n > c.entries {
                return Err(VorbisError::InvalidSetup);
            }
            let from = current_entry as usize;
            lengths[from..from + n as usize].fill(current_length as u8);
            current_entry += n;
            current_length += 1;
        }
    } else {
        for length in lengths.iter_mut() {
            let present = if c.sparse { f.get_bits(1) } else { 1 };
            if present != 0 {
                *length = f.get_bits(5) as u8 + 1;
                total += 1;
                if *length == 32 {
                    return Err(VorbisError::InvalidSetup);
                }
            } else {
                *length = NO_CODE;
            }
        }
    }
    if c.sparse && total >= c.entries >> 2 {
        // convert sparse items to non-sparse!
        c.sparse = false;
    }

    // compute the size of the sorted tables
    let sorted = if c.sparse {
        total
    } else {
        let long = |&&l: &&u8| l > FAST_HUFFMAN_LENGTH && l != NO_CODE;
        lengths.iter().filter(long).count() as i32
    };
    c.sorted_entries = sorted;
    let sorted = sorted as usize;
    let mut values = Vec::new();
    if !c.sparse {
        budget.take(4 * entries as u64)?;
        c.codewords = vec![0; entries];
    } else if sorted > 0 {
        budget.take(9 * sorted as u64)?;
        c.codeword_lengths = vec![0; sorted];
        c.codewords = vec![0; sorted];
        values = vec![0; sorted];
    }
    if !c.compute_codewords(&lengths, &mut values) {
        return Err(VorbisError::InvalidSetup);
    }
    if sorted > 0 {
        // an extra slot for sentinels, and one in front for sorted_values[-1]
        budget.take(8 * (sorted as u64 + 1))?;
        c.sorted_codewords = vec![0; sorted + 1];
        c.sorted_values = vec![0; sorted + 1];
        c.sorted_values[0] = -1;
        c.compute_sorted_huffman(&lengths, &values);
    }
    if c.sparse {
        c.codewords = Vec::new();
    } else {
        c.codeword_lengths = lengths;
    }
    c.compute_accelerated_huffman();

    c.lookup_type = f.get_bits(4) as u8;
    if c.lookup_type > 2 {
        return Err(VorbisError::InvalidSetup);
    }
    if c.lookup_type > 0 {
        read_lookup(f, &mut c, budget)?;
    }
    Ok(c)
}

/// The vector lookup table of a codebook, pre-expanded for lookup type 1
/// as stb does without `STB_VORBIS_DIVIDES_IN_CODEBOOK`.
fn read_lookup<S: Source>(
    f: &mut Reader<S>,
    c: &mut Codebook,
    budget: &mut Budget,
) -> Result<(), VorbisError> {
    c.minimum_value = float32_unpack(f.get_bits(32));
    c.delta_value = float32_unpack(f.get_bits(32));
    c.value_bits = f.get_bits(4) as u8 + 1;
    c.sequence_p = f.get_bits(1) != 0;
    if c.lookup_type == 1 {
        let values = lookup1_values(c.entries, c.dimensions);
        if values < 0 {
            return Err(VorbisError::InvalidSetup);
        }
        c.lookup_values = values as u32;
    } else {
        c.lookup_values = c.entries.wrapping_mul(c.dimensions) as u32;
    }
    if c.lookup_values == 0 {
        return Err(VorbisError::InvalidSetup);
    }
    let count = c.lookup_values as usize;
    if !setup_temp_malloc_ok(2 * count as u64) {
        return Err(VorbisError::OutOfMem);
    }
    budget.take(2 * count as u64)?;
    let mults: Vec<u16> = (0..count)
        .map(|_| f.get_bits(i32::from(c.value_bits)) as u16)
        .collect();
    let dims = c.dimensions as usize;
    if c.lookup_type == 1 {
        let sparse = c.sparse;
        let mut last = 0.0_f32;
        // pre-expand the lookup1-style multiplicands, to avoid a divide in the inner loop
        if sparse && c.sorted_entries == 0 {
            return Ok(());
        }
        let len = if sparse {
            c.sorted_entries as usize
        } else {
            c.entries as usize
        };
        let bytes = 4 * len as u64 * dims as u64;
        budget.malloc(bytes)?;
        c.multiplicands = vec![0.0; len * dims];
        for j in 0..len {
            let z = if sparse {
                c.sorted_value(j as i32) as u32
            } else {
                j as u32
            };
            let mut div = 1_u32;
            for k in 0..dims {
                let off = ((z / div) % c.lookup_values) as usize;
                let val = f32::from(mults[off]) * c.delta_value + c.minimum_value + last;
                c.multiplicands[j * dims + k] = val;
                if c.sequence_p {
                    last = val;
                }
                if k + 1 < dims {
                    if div > u32::MAX / c.lookup_values {
                        return Err(VorbisError::InvalidSetup);
                    }
                    div *= c.lookup_values;
                }
            }
        }
        c.lookup_type = 2;
    } else {
        let mut last = 0.0_f32;
        budget.malloc(4 * count as u64)?;
        c.multiplicands = Vec::with_capacity(count);
        for &mult in &mults {
            let val = f32::from(mult) * c.delta_value + c.minimum_value + last;
            c.multiplicands.push(val);
            if c.sequence_p {
                last = val;
            }
        }
    }
    Ok(())
}
