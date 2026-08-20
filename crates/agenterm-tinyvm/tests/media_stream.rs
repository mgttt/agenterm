//! Converter/renderer-owned black-box vectors for media stream v1.

use agenterm_tinyvm::{Grid3dFrame, ToneBatch, WasmError};

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
}
