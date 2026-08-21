//! Converter/renderer-owned black-box vectors for media stream v1.

use agenterm_tinyvm::{Grid3dFrame, Indexed2dFrame, RenderFrame, ToneBatch, WasmError};

fn must_ok<T>(result: Result<T, WasmError>, context: &str) -> T {
    match result {
        Ok(value) => value,
        Err(error) => panic!("{context}: {}", error.message()),
    }
}

#[test]
fn grid3d_frame_decodes_exact_board_cells_and_hud() {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"TAG3");
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&32u16.to_le_bytes());
    bytes.extend_from_slice(&5u16.to_le_bytes());
    bytes.extend_from_slice(&5u16.to_le_bytes());
    bytes.extend_from_slice(&10u16.to_le_bytes());
    bytes.extend_from_slice(&2u16.to_le_bytes());
    bytes.extend_from_slice(&420u32.to_le_bytes());
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(&2u32.to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend_from_slice(&[1, 2, 3, 1]);
    bytes.extend_from_slice(&0xff00_00ffu32.to_le_bytes());
    bytes.extend_from_slice(&[4, 4, 9, 2]);
    bytes.extend_from_slice(&0x00ff_00ffu32.to_le_bytes());

    let frame = must_ok(Grid3dFrame::decode(&bytes), "decode grid3d frame");
    assert_eq!((frame.width, frame.depth, frame.height), (5, 5, 10));
    assert_eq!((frame.score, frame.cleared_decks, frame.level), (420, 3, 2));
    let cells: Vec<_> = frame
        .cells()
        .map(|cell| must_ok(cell, "decode grid3d cell"))
        .collect();
    assert_eq!(cells.len(), 2);
    assert_eq!(
        (cells[1].x, cells[1].y, cells[1].z, cells[1].kind),
        (4, 4, 9, 2)
    );
}

#[test]
fn grid3d_rejects_trailing_bytes_and_out_of_board_cells() {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"TAG3\x01\0\x20\0\x05\0\x05\0\x0a\0\x01\0");
    bytes.extend_from_slice(&[0; 16]);
    bytes.extend_from_slice(&[5, 0, 0, 1, 0, 0, 0, 0]);
    assert!(matches!(
        Grid3dFrame::decode(&bytes),
        Err(WasmError::Trap("invalid grid3d cell"))
    ));
    bytes[32] = 0;
    bytes.push(0);
    assert!(matches!(
        Grid3dFrame::decode(&bytes),
        Err(WasmError::Trap("grid3d frame size"))
    ));
}

#[test]
fn tone_batch_decodes_and_rejects_unsafe_values() {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"TAT1");
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&[2, 0]);
    bytes.extend_from_slice(&880u16.to_le_bytes());
    bytes.extend_from_slice(&120u16.to_le_bytes());
    bytes.extend_from_slice(&750u16.to_le_bytes());
    let batch = must_ok(ToneBatch::decode(&bytes), "decode tone batch");
    let event = must_ok(batch.events().next().expect("event"), "valid tone event");
    assert_eq!(
        (event.kind, event.frequency_hz, event.duration_ms),
        (2, 880, 120)
    );

    bytes[14..16].copy_from_slice(&1001u16.to_le_bytes());
    assert!(matches!(
        ToneBatch::decode(&bytes),
        Err(WasmError::Trap("invalid tone event"))
    ));

    let mut bounded = Vec::new();
    bounded.extend_from_slice(b"TAT1\x01\0\x10\0");
    for _ in 0..16 {
        bounded.extend_from_slice(&[1, 0]);
        bounded.extend_from_slice(&440u16.to_le_bytes());
        bounded.extend_from_slice(&250u16.to_le_bytes());
        bounded.extend_from_slice(&500u16.to_le_bytes());
    }
    assert_eq!(
        must_ok(ToneBatch::decode(&bounded), "decode maximum tone batch").event_count(),
        16
    );

    bounded[6..8].copy_from_slice(&17u16.to_le_bytes());
    let seventeenth: [u8; 8] = bounded[8..16].try_into().expect("tone record");
    bounded.extend_from_slice(&seventeenth);
    assert!(matches!(
        ToneBatch::decode(&bounded),
        Err(WasmError::Trap("tone batch size"))
    ));

    let mut too_long = Vec::new();
    too_long.extend_from_slice(b"TAT1\x01\0\x03\0");
    for duration in [2_000u16, 2_000, 1] {
        too_long.extend_from_slice(&[2, 0]);
        too_long.extend_from_slice(&880u16.to_le_bytes());
        too_long.extend_from_slice(&duration.to_le_bytes());
        too_long.extend_from_slice(&750u16.to_le_bytes());
    }
    assert!(matches!(
        ToneBatch::decode(&too_long),
        Err(WasmError::Trap("tone batch duration"))
    ));
}

fn indexed2d_frame() -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"TAI2");
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&16u16.to_le_bytes());
    bytes.extend_from_slice(&3u16.to_le_bytes());
    bytes.extend_from_slice(&2u16.to_le_bytes());
    bytes.extend_from_slice(&2u16.to_le_bytes());
    bytes.extend_from_slice(&0u16.to_le_bytes());
    bytes.extend_from_slice(&0xff00_00ffu32.to_le_bytes());
    bytes.extend_from_slice(&0x00ff_00ffu32.to_le_bytes());
    bytes.extend_from_slice(&[0, 1, 1, 0, 1, 0]);
    bytes
}

#[test]
fn indexed2d_decodes_palette_and_exact_pixel_plane() {
    let bytes = indexed2d_frame();
    let frame = must_ok(Indexed2dFrame::decode(&bytes), "decode indexed2d frame");
    assert_eq!((frame.width, frame.height), (3, 2));
    assert_eq!(
        frame.palette_rgba().collect::<Vec<_>>(),
        vec![0xff00_00ff, 0x00ff_00ff]
    );
    assert_eq!(frame.pixels(), &[0, 1, 1, 0, 1, 0]);
    assert_eq!(frame.metadata_schema, None);
    assert!(frame.metadata().is_empty());
    assert!(matches!(
        must_ok(RenderFrame::decode(&bytes), "decode render frame"),
        RenderFrame::Indexed2d(_)
    ));
}

#[test]
fn indexed2d_metadata_is_bounded_schema_tagged_and_exact() {
    let mut bytes = indexed2d_frame();
    bytes[14..16].copy_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(b"TAM1");
    bytes.extend_from_slice(&0x3147_4c53u32.to_le_bytes());
    bytes.extend_from_slice(&4u16.to_le_bytes());
    bytes.extend_from_slice(&0u16.to_le_bytes());
    bytes.extend_from_slice(&[1, 2, 3, 4]);

    let frame = must_ok(Indexed2dFrame::decode(&bytes), "decode indexed2d metadata");
    assert_eq!(frame.metadata_schema, Some(0x3147_4c53));
    assert_eq!(frame.metadata(), &[1, 2, 3, 4]);
    assert_eq!(frame.pixels(), &[0, 1, 1, 0, 1, 0]);

    let metadata_offset = indexed2d_frame().len();
    for invalid in [
        {
            let mut invalid = bytes.clone();
            invalid[metadata_offset] = b'X';
            invalid
        },
        {
            let mut invalid = bytes.clone();
            invalid[metadata_offset + 4..metadata_offset + 8].copy_from_slice(&0u32.to_le_bytes());
            invalid
        },
        {
            let mut invalid = bytes.clone();
            invalid[metadata_offset + 10] = 1;
            invalid
        },
        {
            let mut invalid = bytes.clone();
            invalid.push(5);
            invalid
        },
    ] {
        assert!(Indexed2dFrame::decode(&invalid).is_err());
    }

    let mut oversized = indexed2d_frame();
    oversized[14..16].copy_from_slice(&1u16.to_le_bytes());
    oversized.extend_from_slice(b"TAM1");
    oversized.extend_from_slice(&1u32.to_le_bytes());
    oversized.extend_from_slice(&1_025u16.to_le_bytes());
    oversized.extend_from_slice(&0u16.to_le_bytes());
    oversized.resize(oversized.len() + 1_025, 0);
    assert!(matches!(
        Indexed2dFrame::decode(&oversized),
        Err(WasmError::Trap("indexed2d metadata size"))
    ));
}

#[test]
fn indexed2d_rejects_unknown_indices_flags_trailing_bytes_and_oversize() {
    let mut bytes = indexed2d_frame();
    *bytes.last_mut().expect("pixel") = 2;
    assert!(matches!(
        Indexed2dFrame::decode(&bytes),
        Err(WasmError::Trap("invalid indexed2d pixel"))
    ));

    let mut bytes = indexed2d_frame();
    bytes[14] = 2;
    assert!(matches!(
        Indexed2dFrame::decode(&bytes),
        Err(WasmError::Trap("indexed2d frame size"))
    ));

    let mut bytes = indexed2d_frame();
    bytes.push(0);
    assert!(matches!(
        Indexed2dFrame::decode(&bytes),
        Err(WasmError::Trap("indexed2d frame size"))
    ));

    let oversized = vec![0; 64 * 1024 + 1];
    assert!(matches!(
        Indexed2dFrame::decode(&oversized),
        Err(WasmError::Trap("invalid indexed2d frame header"))
    ));
    assert!(matches!(
        RenderFrame::decode(b"NOPE"),
        Err(WasmError::Trap("unknown render stream"))
    ));
}

#[test]
fn indexed2d_accepts_classic_256x240_and_320x200_frames_under_default_budget() {
    for (width, height) in [(256u16, 240u16), (320u16, 200u16)] {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"TAI2");
        bytes.extend_from_slice(&1u16.to_le_bytes());
        bytes.extend_from_slice(&16u16.to_le_bytes());
        bytes.extend_from_slice(&width.to_le_bytes());
        bytes.extend_from_slice(&height.to_le_bytes());
        bytes.extend_from_slice(&256u16.to_le_bytes());
        bytes.extend_from_slice(&0u16.to_le_bytes());
        for color in 0..256u32 {
            bytes.extend_from_slice(&(color | 0xff00_0000).to_le_bytes());
        }
        bytes.resize(16 + 256 * 4 + usize::from(width) * usize::from(height), 255);
        assert!(bytes.len() <= 64 * 1024);
        let frame = must_ok(Indexed2dFrame::decode(&bytes), "decode classic frame");
        assert_eq!((frame.width, frame.height), (width, height));
        assert_eq!(frame.palette_count(), 256);
    }
}
