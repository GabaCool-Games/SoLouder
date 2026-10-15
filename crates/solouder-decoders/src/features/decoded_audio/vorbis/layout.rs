// Ported from stb_vorbis 1.22 by Sean Barrett (public domain or MIT);
// altered for SoLouder.

//! Floors, residues, mappings and modes of the setup header (the middle of
//! `start_decoder`).

use super::codebook::Codebook;
use super::ogg::{Reader, VorbisError};
use super::setup::{Budget, Floor1, Mapping, MappingChannel, Mode, Residue};
use super::source::Source;
use super::tables::ilog;

/// `neighbors`: the closest points below and above point `n`, among the
/// points before it.
fn neighbors(x: &[u16], n: usize) -> (u8, u8) {
    let (mut low, mut high) = (-1_i32, 65536_i32);
    let (mut plow, mut phigh) = (0, 0);
    for (i, &xi) in x.iter().enumerate().take(n) {
        let xi = i32::from(xi);
        if xi > low && xi < i32::from(x[n]) {
            plow = i as u8;
            low = xi;
        }
        if xi < high && xi > i32::from(x[n]) {
            phigh = i as u8;
            high = xi;
        }
    }
    (plow, phigh)
}

/// The floors (`floor_count`, `floor_types`, `floor_config`) and the
/// longest floor list.
pub(super) fn read_floors<S: Source>(
    f: &mut Reader<S>,
    codebook_count: usize,
) -> Result<(Vec<Floor1>, usize), VorbisError> {
    let floor_count = f.get_bits(6) + 1;
    let mut floors = Vec::with_capacity(floor_count as usize);
    let mut longest_floorlist = 0;
    for _ in 0..floor_count {
        let floor_type = f.get_bits(16);
        if floor_type > 1 {
            return Err(VorbisError::InvalidSetup);
        }
        if floor_type == 0 {
            // order, rate, bark_map_size, amplitude_bits, amplitude_offset
            for bits in [8, 16, 16, 6, 8] {
                f.get_bits(bits);
            }
            let number_of_books = f.get_bits(4) + 1;
            for _ in 0..number_of_books {
                f.get_bits(8);
            }
            return Err(VorbisError::FeatureNotSupported);
        }
        let mut g = Floor1 {
            partitions: 0,
            partition_class_list: [0; 32],
            class_dimensions: [0; 16],
            class_subclasses: [0; 16],
            class_masterbooks: [0; 16],
            subclass_books: [[0; 8]; 16],
            xlist: [0; 31 * 8 + 2],
            sorted_order: [0; 31 * 8 + 2],
            neighbors: [[0; 2]; 31 * 8 + 2],
            floor1_multiplier: 0,
            rangebits: 0,
            values: 0,
        };
        let mut max_class = -1;
        g.partitions = f.get_bits(5) as u8;
        for j in 0..usize::from(g.partitions) {
            g.partition_class_list[j] = f.get_bits(4) as u8;
            max_class = max_class.max(i32::from(g.partition_class_list[j]));
        }
        for j in 0..(max_class + 1) as usize {
            g.class_dimensions[j] = f.get_bits(3) as u8 + 1;
            g.class_subclasses[j] = f.get_bits(2) as u8;
            if g.class_subclasses[j] != 0 {
                g.class_masterbooks[j] = f.get_bits(8) as u8;
                if usize::from(g.class_masterbooks[j]) >= codebook_count {
                    return Err(VorbisError::InvalidSetup);
                }
            }
            for k in 0..1 << g.class_subclasses[j] {
                g.subclass_books[j][k] = f.get_bits(8) as i16 - 1;
                if i32::from(g.subclass_books[j][k]) >= codebook_count as i32 {
                    return Err(VorbisError::InvalidSetup);
                }
            }
        }
        g.floor1_multiplier = f.get_bits(2) as u8 + 1;
        g.rangebits = f.get_bits(4) as u8;
        g.xlist[0] = 0;
        g.xlist[1] = 1 << g.rangebits;
        g.values = 2;
        for j in 0..usize::from(g.partitions) {
            let c = usize::from(g.partition_class_list[j]);
            for _ in 0..g.class_dimensions[c] {
                g.xlist[g.values as usize] = f.get_bits(i32::from(g.rangebits)) as u16;
                g.values += 1;
            }
        }
        // precompute the sorting (equal points are rejected, so any sort
        // gives qsort's order)
        let values = g.values as usize;
        let mut p: Vec<(u16, u8)> = (0..values).map(|j| (g.xlist[j], j as u8)).collect();
        p.sort_by_key(|&(x, _)| x);
        if p.windows(2).any(|w| w[0].0 == w[1].0) {
            return Err(VorbisError::InvalidSetup);
        }
        for (j, &(_, id)) in p.iter().enumerate() {
            g.sorted_order[j] = id;
        }
        // precompute the neighbors
        for j in 2..values {
            let (low, hi) = neighbors(&g.xlist, j);
            g.neighbors[j] = [low, hi];
        }
        longest_floorlist = longest_floorlist.max(values);
        floors.push(g);
    }
    Ok((floors, longest_floorlist))
}

/// The residues (`residue_count`, `residue_types`, `residue_config`).
pub(super) fn read_residues<S: Source>(
    f: &mut Reader<S>,
    codebooks: &[Codebook],
    budget: &mut Budget,
) -> Result<Vec<Residue>, VorbisError> {
    let residue_count = f.get_bits(6) + 1;
    let mut residues = Vec::with_capacity(residue_count as usize);
    for _ in 0..residue_count {
        let mut residue_cascade = [0_u8; 64];
        let rtype = f.get_bits(16) as u16;
        if rtype > 2 {
            return Err(VorbisError::InvalidSetup);
        }
        let begin = f.get_bits(24);
        let end = f.get_bits(24);
        if end < begin {
            return Err(VorbisError::InvalidSetup);
        }
        let part_size = f.get_bits(24) + 1;
        let classifications = f.get_bits(6) as u8 + 1;
        let classbook = f.get_bits(8) as u8;
        let Some(book) = codebooks.get(usize::from(classbook)) else {
            return Err(VorbisError::InvalidSetup);
        };
        for cascade in residue_cascade
            .iter_mut()
            .take(usize::from(classifications))
        {
            let low_bits = f.get_bits(3) as u8;
            let high_bits = if f.get_bits(1) != 0 {
                f.get_bits(5) as u8
            } else {
                0
            };
            *cascade = high_bits.wrapping_mul(8).wrapping_add(low_bits);
        }
        let mut residue_books = vec![[0_i16; 8]; usize::from(classifications)];
        for (books, &cascade) in residue_books.iter_mut().zip(&residue_cascade) {
            for (k, book) in books.iter_mut().enumerate() {
                if cascade & (1 << k) != 0 {
                    *book = f.get_bits(8) as i16;
                    if usize::from(*book as u16) >= codebooks.len() {
                        return Err(VorbisError::InvalidSetup);
                    }
                } else {
                    *book = -1;
                }
            }
        }
        // precompute the classifications[] array to avoid inner-loop mod/divide
        let entries = book.entries as usize;
        let classwords = book.dimensions as usize;
        if classwords == 0 {
            // Codebooks with entries have dimensions; an empty one is
            // rejected when read.
            return Err(VorbisError::InvalidSetup);
        }
        budget.take(entries as u64 * classwords as u64)?;
        let mut classdata = vec![0_u8; entries * classwords];
        for (j, row) in classdata.chunks_exact_mut(classwords).enumerate() {
            let mut temp = j as i32;
            for k in (0..classwords).rev() {
                row[k] = (temp % i32::from(classifications)) as u8;
                temp /= i32::from(classifications);
            }
        }
        residues.push(Residue {
            rtype,
            begin,
            end,
            part_size,
            classbook,
            classdata,
            residue_books,
        });
    }
    Ok(residues)
}

/// The mappings (`mapping_count`, `mapping`).
pub(super) fn read_mappings<S: Source>(
    f: &mut Reader<S>,
    channels: i32,
    floor_count: usize,
    residue_count: usize,
) -> Result<Vec<Mapping>, VorbisError> {
    let mapping_count = f.get_bits(6) + 1;
    let mut mappings = Vec::with_capacity(mapping_count as usize);
    for _ in 0..mapping_count {
        let mapping_type = f.get_bits(16);
        if mapping_type != 0 {
            return Err(VorbisError::InvalidSetup);
        }
        let mut m = Mapping {
            coupling_steps: 0,
            chan: vec![MappingChannel::default(); channels as usize],
            submaps: 1,
            submap: [0; 31],
        };
        if f.get_bits(1) != 0 {
            m.submaps = f.get_bits(4) as u8 + 1;
        }
        if f.get_bits(1) != 0 {
            m.coupling_steps = f.get_bits(8) as u16 + 1;
            if i32::from(m.coupling_steps) > channels {
                return Err(VorbisError::InvalidSetup);
            }
            for k in 0..usize::from(m.coupling_steps) {
                let magnitude = f.get_bits(ilog(channels - 1)) as u8;
                let angle = f.get_bits(ilog(channels - 1)) as u8;
                m.chan[k].magnitude = magnitude;
                m.chan[k].angle = angle;
                if i32::from(magnitude) >= channels || i32::from(angle) >= channels {
                    return Err(VorbisError::InvalidSetup);
                }
                if magnitude == angle {
                    return Err(VorbisError::InvalidSetup);
                }
            }
        }
        // reserved field
        if f.get_bits(2) != 0 {
            return Err(VorbisError::InvalidSetup);
        }
        if m.submaps > 1 {
            for chan in m.chan.iter_mut() {
                chan.mux = f.get_bits(4) as u8;
                if chan.mux >= m.submaps {
                    return Err(VorbisError::InvalidSetup);
                }
            }
        }
        for j in 0..usize::from(m.submaps) {
            f.get_bits(8); // discard
            m.submap[j] = f.get_bits(8) as u8;
            m.submap[15 + j] = f.get_bits(8) as u8;
            if m.submap_floor(j) >= floor_count {
                return Err(VorbisError::InvalidSetup);
            }
            if m.submap_residue(j) >= residue_count {
                return Err(VorbisError::InvalidSetup);
            }
        }
        mappings.push(m);
    }
    Ok(mappings)
}

/// The modes (`mode_count`, `mode_config`).
pub(super) fn read_modes<S: Source>(
    f: &mut Reader<S>,
    mapping_count: usize,
) -> Result<Vec<Mode>, VorbisError> {
    let mode_count = f.get_bits(6) + 1;
    let mut modes = Vec::with_capacity(mode_count as usize);
    for _ in 0..mode_count {
        let m = Mode {
            blockflag: f.get_bits(1) as u8,
            windowtype: f.get_bits(16) as u16,
            transformtype: f.get_bits(16) as u16,
            mapping: f.get_bits(8) as u8,
        };
        if m.windowtype != 0 || m.transformtype != 0 {
            return Err(VorbisError::InvalidSetup);
        }
        if usize::from(m.mapping) >= mapping_count {
            return Err(VorbisError::InvalidSetup);
        }
        modes.push(m);
    }
    Ok(modes)
}
