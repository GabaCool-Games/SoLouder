// Ported from SoLoud include/zx7decompress.h (public domain / Unlicense,
// Jari Komppa), heavily based on the zx7 decompressor by Einar Saukas;
// altered for SoLouder. Only the decompressor is ported: the zx7
// compressor has a different licence (fixtures use a first-party encoder,
// scripts/gen_source_fixtures.py).

//! zx7 decompression within bounds (`zx7_decompress_bounded`, reference
//! patch 0087; SoLoud's `zx7_decompress` read and wrote without bounds).

struct Reader<'a> {
    input: &'a [u8],
    at: usize,
    mask: u8,
    bits: u8,
}

impl Reader<'_> {
    fn byte(&mut self) -> Option<u8> {
        let byte = *self.input.get(self.at)?;
        self.at += 1;
        Some(byte)
    }

    fn bit(&mut self) -> Option<bool> {
        self.mask >>= 1;
        if self.mask == 0 {
            self.mask = 128;
            self.bits = self.byte()?;
        }
        Some(self.bits & self.mask != 0)
    }

    /// An Elias gamma code, or `None` for the end marker (more than 15
    /// leading zeros) as well as for data that runs out.
    fn gamma(&mut self) -> Option<Option<u32>> {
        let mut zeros = 0;
        while !self.bit()? {
            zeros += 1;
        }
        if zeros > 15 {
            return Some(None);
        }
        let mut value = 1;
        for _ in 0..zeros {
            value = value << 1 | u32::from(self.bit()?);
        }
        Some(Some(value))
    }

    fn offset(&mut self) -> Option<usize> {
        let value = usize::from(self.byte()?);
        if value < 128 {
            return Some(value);
        }
        let mut high = 0;
        for _ in 0..4 {
            high = high << 1 | usize::from(self.bit()?);
        }
        Some((value & 127 | high << 7) + 128)
    }
}

/// Decompresses one zx7 stream from `input` into `output` from `start`,
/// copying back-references from anywhere in `output` before the write
/// position. The bytes of `input` read, or `None` if the stream would read
/// past `input`, write past `output` or copy from before its start.
pub(crate) fn decompress(input: &[u8], output: &mut [u8], start: usize) -> Option<usize> {
    let mut reader = Reader {
        input,
        at: 0,
        mask: 0,
        bits: 0,
    };
    let mut at = start;
    let put = |output: &mut [u8], at: &mut usize, byte: u8| -> Option<()> {
        *output.get_mut(*at)? = byte;
        *at += 1;
        Some(())
    };
    let first = reader.byte()?;
    put(output, &mut at, first)?;
    loop {
        if !reader.bit()? {
            let byte = reader.byte()?;
            put(output, &mut at, byte)?;
            continue;
        }
        let Some(value) = reader.gamma()? else {
            return Some(reader.at);
        };
        let length = value + 1;
        let offset = reader.offset()? + 1;
        for _ in 0..length {
            let from = at.checked_sub(offset)?;
            let byte = output[from];
            put(output, &mut at, byte)?;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::decompress;

    #[test]
    fn streams_decompress_within_bounds() {
        // 28 bytes of a SID dump, compressed by the fixture script.
        let packed = [
            0x0f, 0x00, 0x18, 0x22, 0x05, 0xa8, 0x06, 0x00, 0x00, 0x1c, 0x02, 0x01, 0x11, 0x04,
            0xd0, 0x87, 0x10, 0x80, 0x03, 0x25, 0x01, 0x41, 0x04, 0x00, 0x02, 0x08, 0x08, 0x03,
            0xb8, 0x8b, 0x00, 0x04,
        ];
        let mut out = [0; 28];
        assert_eq!(decompress(&packed, &mut out, 0), Some(packed.len()));
        assert_eq!(&out[..4], &[0x0f, 0x18, 0x22, 0x05]);
        assert_eq!(&out[26..], &[0xb8, 0x8b]);
        for len in 0..packed.len() {
            assert_eq!(decompress(&packed[..len], &mut out, 0), None);
        }
        assert_eq!(decompress(&packed, &mut [0; 27], 0), None);
        // 3001 zeros: a literal and a copy of 3000.
        let zeros = [0x00, 0x80, 0x0b, 0xb7, 0x00, 0x80, 0x00, 0x40];
        assert_eq!(decompress(&zeros, &mut [0; 3001], 0), Some(8));
        assert_eq!(decompress(&zeros, &mut [0; 3000], 0), None);
        assert_eq!(decompress(&zeros, &mut [0; 3001], 1), None);
        // A copy from before the output's start.
        assert_eq!(decompress(&[0x00, 0xc0, 0x04], &mut [0; 8], 0), None);
    }
}
