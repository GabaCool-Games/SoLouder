use super::*;

/// A stored dump of `ops` for chip 0, looping to `loop_pos`.
fn dump(ops: &[u16], loop_pos: usize) -> Vec<u8> {
    let length = ops.len() * 2;
    let chunks = length.div_ceil(1024);
    let mut out = b"CHIPTUNE".to_vec();
    for word in [
        28,
        0x100,
        chunks,
        length - (chunks - 1) * 1024,
        loop_pos / 1024,
        loop_pos % 1024,
    ] {
        out.extend_from_slice(&(word as u16).to_le_bytes());
    }
    out.extend_from_slice(&50_u32.to_le_bytes());
    out.extend_from_slice(&985_248_u32.to_le_bytes());
    out.extend(ops.iter().flat_map(|op| op.to_le_bytes()));
    out
}

#[test]
fn headers_are_checked() {
    let ops = [0x1811, 0x8100, 0x0400, 0x8200];
    let good = ChipDump::parse(&dump(&ops, 2), &[0]).unwrap();
    assert_eq!((good.length, good.loop_pos, good.flags), (8, 2, 1));
    assert_eq!((good.cpu_speed, good.chip_speed), (50, 985_248));
    assert_eq!(good.ops, ops);
    assert!(good.loop_waits());
    let bad = |bytes: &[u8]| ChipDump::parse(bytes, &[0]).unwrap_err();
    assert!(matches!(
        ChipDump::parse(&dump(&ops, 0), &[1, 2]).unwrap_err(),
        ChipDumpError::UnsupportedChip(0)
    ));
    assert!(matches!(bad(&dump(&ops, 8)), ChipDumpError::BadHeader));
    let mut bytes = dump(&ops, 0);
    bytes[12] = 0;
    assert!(matches!(bad(&bytes), ChipDumpError::BadHeader));
    let mut bytes = dump(&ops, 0);
    bytes[8] = 200;
    assert!(matches!(bad(&bytes), ChipDumpError::BadHeader));
    let mut bytes = dump(&ops, 0);
    bytes[0] = b'D';
    assert!(matches!(bad(&bytes), ChipDumpError::NotChipTune));
    let whole = dump(&[0x8100; 8], 0);
    for len in MIN_FILE..whole.len() {
        assert!(matches!(bad(&whole[..len]), ChipDumpError::Truncated));
    }
    for len in 0..MIN_FILE {
        assert!(matches!(bad(&whole[..len]), ChipDumpError::Truncated));
    }
    // An odd length: the last op's high byte is 0.
    let mut bytes = dump(&[0x8100, 0x8100, 0x8100, 0x1234], 0);
    bytes[14] = 7;
    let odd = ChipDump::parse(&bytes, &[0]).unwrap();
    assert_eq!(odd.ops[3], 0x0034);
    assert!(
        !ChipDump::parse(&dump(&[0x8100, 0x0400, 0x0401, 0x0402], 2), &[0])
            .unwrap()
            .loop_waits()
    );
    assert!(
        !ChipDump::parse(&dump(&[0x8000, 0x0400, 0x0401, 0x0402], 0), &[0])
            .unwrap()
            .loop_waits()
    );
}

#[test]
fn compressed_chunks_are_bounded() {
    let mut bytes = dump(&[0; 14], 0);
    bytes[11] = 0;
    bytes.truncate(28);
    bytes.extend_from_slice(&[
        0x0f, 0x00, 0x18, 0x22, 0x05, 0xa8, 0x06, 0x00, 0x00, 0x1c, 0x02, 0x01, 0x11, 0x04, 0xd0,
        0x87, 0x10, 0x80, 0x03, 0x25, 0x01, 0x41, 0x04, 0x00, 0x02, 0x08, 0x08, 0x03, 0xb8, 0x8b,
        0x00, 0x04,
    ]);
    let dump = ChipDump::parse(&bytes, &[0]).unwrap();
    assert_eq!(&dump.ops[..2], &[0x180f, 0x0522]);
    assert_eq!(dump.ops[13], 0x8bb8);
    bytes.truncate(50);
    assert!(matches!(
        ChipDump::parse(&bytes, &[0]),
        Err(ChipDumpError::BadCompression)
    ));
}
