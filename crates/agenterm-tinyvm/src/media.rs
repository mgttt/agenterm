//! Allocation-free decoders for versioned cartridge media streams.

use crate::WasmError;

pub const GRID3D_MAGIC: &[u8; 4] = b"TAG3";
pub const TONES_MAGIC: &[u8; 4] = b"TAT1";
const GRID3D_HEADER_BYTES: usize = 32;
const GRID3D_CELL_BYTES: usize = 8;
const TONE_HEADER_BYTES: usize = 8;
const TONE_EVENT_BYTES: usize = 8;

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Grid3dCell {
    pub x: u8,
    pub y: u8,
    pub z: u8,
    /// 1 settled, 2 active, 3 landing ghost.
    pub kind: u8,
    /// RGBA8 packed as a little-endian u32.
    pub rgba: u32,
}

/// Strict view over one `tinyarcade:grid3d/v1` frame.
pub struct Grid3dFrame<'a> {
    pub width: u16,
    pub depth: u16,
    pub height: u16,
    pub score: u32,
    pub cleared_decks: u32,
    pub level: u32,
    pub flags: u32,
    cells: &'a [u8],
}

impl<'a> Grid3dFrame<'a> {
    pub fn decode(bytes: &'a [u8]) -> Result<Self, WasmError> {
        if bytes.len() < GRID3D_HEADER_BYTES
            || &bytes[..4] != GRID3D_MAGIC
            || read_u16(bytes, 4)? != 1
            || read_u16(bytes, 6)? as usize != GRID3D_HEADER_BYTES
        {
            return Err(WasmError::Trap("invalid grid3d frame header"));
        }
        let width = read_u16(bytes, 8)?;
        let depth = read_u16(bytes, 10)?;
        let height = read_u16(bytes, 12)?;
        let count = read_u16(bytes, 14)? as usize;
        let expected = count
            .checked_mul(GRID3D_CELL_BYTES)
            .and_then(|cells| cells.checked_add(GRID3D_HEADER_BYTES))
            .ok_or(WasmError::Trap("grid3d frame size"))?;
        if width == 0 || depth == 0 || height == 0 || expected != bytes.len() {
            return Err(WasmError::Trap("grid3d frame size"));
        }
        let frame = Self {
            width,
            depth,
            height,
            score: read_u32(bytes, 16)?,
            cleared_decks: read_u32(bytes, 20)?,
            level: read_u32(bytes, 24)?,
            flags: read_u32(bytes, 28)?,
            cells: &bytes[GRID3D_HEADER_BYTES..],
        };
        if frame.flags & !1 != 0 {
            return Err(WasmError::Trap("invalid grid3d flags"));
        }
        for cell in frame.cells() {
            let cell = cell?;
            if u16::from(cell.x) >= width
                || u16::from(cell.y) >= depth
                || u16::from(cell.z) >= height
                || !(1..=3).contains(&cell.kind)
            {
                return Err(WasmError::Trap("invalid grid3d cell"));
            }
        }
        Ok(frame)
    }

    pub fn cell_count(&self) -> usize {
        self.cells.len() / GRID3D_CELL_BYTES
    }

    pub fn cells(&self) -> impl Iterator<Item = Result<Grid3dCell, WasmError>> + '_ {
        self.cells.chunks_exact(GRID3D_CELL_BYTES).map(|record| {
            Ok(Grid3dCell {
                x: record[0],
                y: record[1],
                z: record[2],
                kind: record[3],
                rgba: u32::from_le_bytes([record[4], record[5], record[6], record[7]]),
            })
        })
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct ToneEvent {
    /// 1 lock, 2 deck clear, 3 game over.
    pub kind: u8,
    pub frequency_hz: u16,
    pub duration_ms: u16,
    pub amplitude_milli: u16,
}

/// Strict view over one `tinyarcade:tones/v1` batch.
pub struct ToneBatch<'a> {
    events: &'a [u8],
}

impl<'a> ToneBatch<'a> {
    pub fn decode(bytes: &'a [u8]) -> Result<Self, WasmError> {
        if bytes.len() < TONE_HEADER_BYTES || &bytes[..4] != TONES_MAGIC || read_u16(bytes, 4)? != 1
        {
            return Err(WasmError::Trap("invalid tone batch header"));
        }
        let count = read_u16(bytes, 6)? as usize;
        let expected = count
            .checked_mul(TONE_EVENT_BYTES)
            .and_then(|events| events.checked_add(TONE_HEADER_BYTES))
            .ok_or(WasmError::Trap("tone batch size"))?;
        if expected != bytes.len() {
            return Err(WasmError::Trap("tone batch size"));
        }
        let batch = Self {
            events: &bytes[TONE_HEADER_BYTES..],
        };
        for event in batch.events() {
            let event = event?;
            if !(1..=3).contains(&event.kind)
                || !(40..=20_000).contains(&event.frequency_hz)
                || !(1..=2_000).contains(&event.duration_ms)
                || event.amplitude_milli > 1_000
            {
                return Err(WasmError::Trap("invalid tone event"));
            }
        }
        Ok(batch)
    }

    pub fn event_count(&self) -> usize {
        self.events.len() / TONE_EVENT_BYTES
    }

    pub fn events(&self) -> impl Iterator<Item = Result<ToneEvent, WasmError>> + '_ {
        self.events.chunks_exact(TONE_EVENT_BYTES).map(|record| {
            if record[1] != 0 {
                return Err(WasmError::Trap("invalid tone event"));
            }
            Ok(ToneEvent {
                kind: record[0],
                frequency_hz: u16::from_le_bytes([record[2], record[3]]),
                duration_ms: u16::from_le_bytes([record[4], record[5]]),
                amplitude_milli: u16::from_le_bytes([record[6], record[7]]),
            })
        })
    }
}

fn read_u16(bytes: &[u8], offset: usize) -> Result<u16, WasmError> {
    let raw = bytes
        .get(offset..offset + 2)
        .ok_or(WasmError::Trap("media stream bounds"))?;
    Ok(u16::from_le_bytes([raw[0], raw[1]]))
}

fn read_u32(bytes: &[u8], offset: usize) -> Result<u32, WasmError> {
    let raw = bytes
        .get(offset..offset + 4)
        .ok_or(WasmError::Trap("media stream bounds"))?;
    Ok(u32::from_le_bytes([raw[0], raw[1], raw[2], raw[3]]))
}
