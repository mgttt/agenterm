//! Bounded deterministic input traces for converter goldens and bug reports.

use alloc::string::String;
use alloc::vec::Vec;

use crate::{
    CartridgeManifest, GameFrame, GameInput, GameRuntime, KNOWN_BUTTON_MASK, RenderFrame,
    ToneBatch, WasmError, cartridge_sha256,
};

const MAGIC: &[u8; 4] = b"TAR1";
const FORMAT_VERSION: u16 = 1;
const HEADER_BYTES: usize = 64;
const STEP_BYTES: usize = 80;
pub const MAX_REPLAY_STEPS: usize = 65_536;
pub const MAX_REPLAY_SNAPSHOT_BYTES: usize = 1024 * 1024;
pub const MAX_REPLAY_BYTES: usize = 8 * 1024 * 1024;

#[derive(Clone, PartialEq, Eq)]
pub struct ReplayStepV1 {
    pub input: GameInput,
    pub render_length: u32,
    pub audio_length: u32,
    pub render_sha256: [u8; 32],
    pub audio_sha256: [u8; 32],
}

pub struct ReplayTraceV1 {
    pub cartridge_sha256: [u8; 32],
    pub game_id: String,
    pub game_version: String,
    pub abi_version: u32,
    pub state_version: u32,
    pub initial_snapshot: Vec<u8>,
    pub steps: Vec<ReplayStepV1>,
}

pub struct ReplayRecorderV1 {
    trace: ReplayTraceV1,
    last_clock: Option<u32>,
}

impl ReplayRecorderV1 {
    pub fn start(wasm: &[u8], runtime: &mut GameRuntime) -> Result<Self, WasmError> {
        if runtime.cartridge_sha256() != cartridge_sha256(wasm) {
            return Err(WasmError::Trap("replay runtime/cartridge mismatch"));
        }
        CartridgeManifest::from_wasm(wasm)?;
        Self::start_runtime(runtime)
    }

    pub fn start_runtime(runtime: &mut GameRuntime) -> Result<Self, WasmError> {
        let manifest = runtime.manifest().clone();
        let initial_snapshot = runtime.suspend()?;
        if initial_snapshot.is_empty() || initial_snapshot.len() > MAX_REPLAY_SNAPSHOT_BYTES {
            return Err(WasmError::Trap("replay snapshot size"));
        }
        Ok(Self {
            trace: ReplayTraceV1 {
                cartridge_sha256: runtime.cartridge_sha256(),
                game_id: manifest.game_id,
                game_version: manifest.game_version,
                abi_version: manifest.abi_version,
                state_version: manifest.state_version,
                initial_snapshot,
                steps: Vec::new(),
            },
            last_clock: None,
        })
    }

    pub fn record_tick(
        &mut self,
        runtime: &mut GameRuntime,
        input: GameInput,
    ) -> Result<GameFrame, WasmError> {
        let mut frame = GameFrame::default();
        self.record_tick_into(runtime, input, &mut frame)?;
        Ok(frame)
    }

    /// Record one deterministic tick while recycling caller-owned frame
    /// storage through the runtime.
    pub fn record_tick_into(
        &mut self,
        runtime: &mut GameRuntime,
        input: GameInput,
        frame: &mut GameFrame,
    ) -> Result<(), WasmError> {
        frame.render.clear();
        frame.audio.clear();
        validate_input(input, self.last_clock)?;
        if self.trace.steps.len() >= MAX_REPLAY_STEPS {
            return Err(WasmError::Trap("replay step limit"));
        }
        self.trace
            .steps
            .try_reserve(1)
            .map_err(|_| WasmError::Trap("replay step allocation"))?;
        runtime.tick_into(input, frame)?;
        validate_frame(frame)?;
        let render_length =
            u32::try_from(frame.render.len()).map_err(|_| WasmError::Trap("replay frame size"))?;
        let audio_length =
            u32::try_from(frame.audio.len()).map_err(|_| WasmError::Trap("replay frame size"))?;
        self.trace.steps.push(ReplayStepV1 {
            input,
            render_length,
            audio_length,
            render_sha256: cartridge_sha256(&frame.render),
            audio_sha256: cartridge_sha256(&frame.audio),
        });
        self.last_clock = Some(input.clock_ms);
        Ok(())
    }

    pub fn finish(&self) -> Result<Vec<u8>, WasmError> {
        self.trace.encode()
    }
}

impl ReplayTraceV1 {
    pub fn encode(&self) -> Result<Vec<u8>, WasmError> {
        validate_identity(self)?;
        let snapshot_len = u32::try_from(self.initial_snapshot.len())
            .map_err(|_| WasmError::Trap("replay snapshot size"))?;
        let step_count =
            u32::try_from(self.steps.len()).map_err(|_| WasmError::Trap("replay step limit"))?;
        let total = HEADER_BYTES
            .checked_add(self.game_id.len())
            .and_then(|value| value.checked_add(self.game_version.len()))
            .and_then(|value| value.checked_add(self.initial_snapshot.len()))
            .and_then(|value| value.checked_add(self.steps.len().checked_mul(STEP_BYTES)?))
            .filter(|value| *value <= MAX_REPLAY_BYTES)
            .ok_or(WasmError::Trap("replay size limit"))?;
        let mut output = Vec::new();
        output
            .try_reserve_exact(total)
            .map_err(|_| WasmError::Trap("replay allocation"))?;
        output.extend_from_slice(MAGIC);
        output.extend_from_slice(&FORMAT_VERSION.to_le_bytes());
        output.extend_from_slice(&(HEADER_BYTES as u16).to_le_bytes());
        output.extend_from_slice(&self.cartridge_sha256);
        output.extend_from_slice(&self.abi_version.to_le_bytes());
        output.extend_from_slice(&self.state_version.to_le_bytes());
        output.extend_from_slice(&(self.game_id.len() as u16).to_le_bytes());
        output.extend_from_slice(&(self.game_version.len() as u16).to_le_bytes());
        output.extend_from_slice(&snapshot_len.to_le_bytes());
        output.extend_from_slice(&step_count.to_le_bytes());
        output.extend_from_slice(&0u32.to_le_bytes());
        output.extend_from_slice(self.game_id.as_bytes());
        output.extend_from_slice(self.game_version.as_bytes());
        output.extend_from_slice(&self.initial_snapshot);
        let mut last_clock = None;
        for step in &self.steps {
            validate_input(step.input, last_clock)?;
            if step.render_length > 64 * 1024 || step.audio_length > 16 * 1024 {
                return Err(WasmError::Trap("replay frame size"));
            }
            output.extend_from_slice(&step.input.buttons.to_le_bytes());
            output.extend_from_slice(&step.input.clock_ms.to_le_bytes());
            output.extend_from_slice(&step.render_length.to_le_bytes());
            output.extend_from_slice(&step.audio_length.to_le_bytes());
            output.extend_from_slice(&step.render_sha256);
            output.extend_from_slice(&step.audio_sha256);
            last_clock = Some(step.input.clock_ms);
        }
        debug_assert_eq!(output.len(), total);
        Ok(output)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, WasmError> {
        if bytes.len() < HEADER_BYTES || bytes.len() > MAX_REPLAY_BYTES {
            return Err(WasmError::Decode("replay size limit"));
        }
        let mut cursor = 0;
        if take(bytes, &mut cursor, 4)? != MAGIC
            || read_u16(bytes, &mut cursor)? != FORMAT_VERSION
            || read_u16(bytes, &mut cursor)? as usize != HEADER_BYTES
        {
            return Err(WasmError::Decode("invalid replay header"));
        }
        let mut cartridge_hash = [0; 32];
        cartridge_hash.copy_from_slice(take(bytes, &mut cursor, 32)?);
        let abi_version = read_u32(bytes, &mut cursor)?;
        let state_version = read_u32(bytes, &mut cursor)?;
        let game_id_len = read_u16(bytes, &mut cursor)? as usize;
        let game_version_len = read_u16(bytes, &mut cursor)? as usize;
        let snapshot_len = read_u32(bytes, &mut cursor)? as usize;
        let step_count = read_u32(bytes, &mut cursor)? as usize;
        if read_u32(bytes, &mut cursor)? != 0
            || !(1..=128).contains(&game_id_len)
            || !(1..=64).contains(&game_version_len)
            || !(1..=MAX_REPLAY_SNAPSHOT_BYTES).contains(&snapshot_len)
            || step_count > MAX_REPLAY_STEPS
        {
            return Err(WasmError::Decode("invalid replay bounds"));
        }
        HEADER_BYTES
            .checked_add(game_id_len)
            .and_then(|value| value.checked_add(game_version_len))
            .and_then(|value| value.checked_add(snapshot_len))
            .and_then(|value| value.checked_add(step_count.checked_mul(STEP_BYTES)?))
            .filter(|value| *value == bytes.len())
            .ok_or(WasmError::Decode("invalid replay length"))?;
        let game_id = copy_string(take(bytes, &mut cursor, game_id_len)?)?;
        let game_version = copy_string(take(bytes, &mut cursor, game_version_len)?)?;
        let initial_snapshot = copy_vec(take(bytes, &mut cursor, snapshot_len)?)?;
        let mut steps = Vec::new();
        steps
            .try_reserve_exact(step_count)
            .map_err(|_| WasmError::Trap("replay step allocation"))?;
        let mut last_clock = None;
        for _ in 0..step_count {
            let input = GameInput {
                buttons: read_u32(bytes, &mut cursor)?,
                clock_ms: read_u32(bytes, &mut cursor)?,
            };
            validate_input(input, last_clock)?;
            let render_length = read_u32(bytes, &mut cursor)?;
            let audio_length = read_u32(bytes, &mut cursor)?;
            if render_length > 64 * 1024 || audio_length > 16 * 1024 {
                return Err(WasmError::Decode("replay frame size"));
            }
            let mut render_sha256 = [0; 32];
            render_sha256.copy_from_slice(take(bytes, &mut cursor, 32)?);
            let mut audio_sha256 = [0; 32];
            audio_sha256.copy_from_slice(take(bytes, &mut cursor, 32)?);
            steps.push(ReplayStepV1 {
                input,
                render_length,
                audio_length,
                render_sha256,
                audio_sha256,
            });
            last_clock = Some(input.clock_ms);
        }
        let trace = Self {
            cartridge_sha256: cartridge_hash,
            game_id,
            game_version,
            abi_version,
            state_version,
            initial_snapshot,
            steps,
        };
        validate_identity(&trace)?;
        Ok(trace)
    }

    pub fn verify_cartridge(&self, wasm: &[u8]) -> Result<(), WasmError> {
        validate_identity(self)?;
        validate_steps(&self.steps)?;
        if cartridge_sha256(wasm) != self.cartridge_sha256 {
            return Err(WasmError::Trap("replay cartridge hash mismatch"));
        }
        let manifest = CartridgeManifest::from_wasm(wasm)?;
        if manifest.game_id != self.game_id
            || manifest.game_version != self.game_version
            || manifest.abi_version != self.abi_version
            || manifest.state_version != self.state_version
        {
            return Err(WasmError::Trap("replay cartridge identity mismatch"));
        }
        Ok(())
    }

    pub fn replay<F>(
        &self,
        wasm: &[u8],
        runtime: &mut GameRuntime,
        consume: F,
    ) -> Result<(), WasmError>
    where
        F: FnMut(usize, &GameFrame) -> Result<(), WasmError>,
    {
        self.verify_cartridge(wasm)?;
        if runtime.cartridge_sha256() != cartridge_sha256(wasm) {
            return Err(WasmError::Trap("replay runtime/cartridge mismatch"));
        }
        self.replay_loaded(runtime, consume)
    }

    pub fn replay_loaded<F>(
        &self,
        runtime: &mut GameRuntime,
        mut consume: F,
    ) -> Result<(), WasmError>
    where
        F: FnMut(usize, &GameFrame) -> Result<(), WasmError>,
    {
        validate_identity(self)?;
        validate_steps(&self.steps)?;
        if runtime.cartridge_sha256() != self.cartridge_sha256 {
            return Err(WasmError::Trap("replay runtime/cartridge mismatch"));
        }
        let manifest = runtime.manifest();
        if manifest.game_id != self.game_id
            || manifest.game_version != self.game_version
            || manifest.abi_version != self.abi_version
            || manifest.state_version != self.state_version
        {
            return Err(WasmError::Trap("replay runtime identity mismatch"));
        }
        runtime.resume(&self.initial_snapshot)?;
        let mut frame = GameFrame::default();
        for (index, step) in self.steps.iter().enumerate() {
            runtime.tick_into(step.input, &mut frame)?;
            validate_frame(&frame)?;
            if usize::try_from(step.render_length).ok() != Some(frame.render.len())
                || usize::try_from(step.audio_length).ok() != Some(frame.audio.len())
                || cartridge_sha256(&frame.render) != step.render_sha256
                || cartridge_sha256(&frame.audio) != step.audio_sha256
            {
                return Err(WasmError::Trap("replay frame mismatch"));
            }
            consume(index, &frame)?;
        }
        Ok(())
    }
}

fn validate_identity(trace: &ReplayTraceV1) -> Result<(), WasmError> {
    if trace.abi_version == 0
        || trace.state_version == 0
        || !valid_token(&trace.game_id, 3, 128, false)
        || !valid_token(&trace.game_version, 1, 64, true)
        || trace.initial_snapshot.is_empty()
        || trace.initial_snapshot.len() > MAX_REPLAY_SNAPSHOT_BYTES
        || trace.steps.len() > MAX_REPLAY_STEPS
    {
        return Err(WasmError::Trap("invalid replay identity"));
    }
    Ok(())
}

fn validate_steps(steps: &[ReplayStepV1]) -> Result<(), WasmError> {
    let mut last_clock = None;
    for step in steps {
        validate_input(step.input, last_clock)?;
        if step.render_length > 64 * 1024 || step.audio_length > 16 * 1024 {
            return Err(WasmError::Trap("replay frame size"));
        }
        last_clock = Some(step.input.clock_ms);
    }
    Ok(())
}

fn valid_token(value: &str, min: usize, max: usize, plus: bool) -> bool {
    (min..=max).contains(&value.len())
        && value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric()
                || matches!(byte, b'.' | b'_' | b'-')
                || (plus && byte == b'+')
        })
}

fn validate_input(input: GameInput, last_clock: Option<u32>) -> Result<(), WasmError> {
    if input.buttons & !KNOWN_BUTTON_MASK != 0
        || last_clock.is_some_and(|previous| input.clock_ms < previous)
    {
        return Err(WasmError::Trap("invalid replay input"));
    }
    Ok(())
}

fn validate_frame(frame: &GameFrame) -> Result<(), WasmError> {
    RenderFrame::decode(&frame.render)?;
    if !frame.audio.is_empty() {
        ToneBatch::decode(&frame.audio)?;
    }
    Ok(())
}

fn take<'a>(bytes: &'a [u8], cursor: &mut usize, count: usize) -> Result<&'a [u8], WasmError> {
    let end = cursor
        .checked_add(count)
        .filter(|end| *end <= bytes.len())
        .ok_or(WasmError::Decode("truncated replay"))?;
    let value = &bytes[*cursor..end];
    *cursor = end;
    Ok(value)
}

fn read_u16(bytes: &[u8], cursor: &mut usize) -> Result<u16, WasmError> {
    let raw = take(bytes, cursor, 2)?;
    Ok(u16::from_le_bytes([raw[0], raw[1]]))
}

fn read_u32(bytes: &[u8], cursor: &mut usize) -> Result<u32, WasmError> {
    let raw = take(bytes, cursor, 4)?;
    Ok(u32::from_le_bytes([raw[0], raw[1], raw[2], raw[3]]))
}

fn copy_vec(bytes: &[u8]) -> Result<Vec<u8>, WasmError> {
    let mut output = Vec::new();
    output
        .try_reserve_exact(bytes.len())
        .map_err(|_| WasmError::Trap("replay allocation"))?;
    output.extend_from_slice(bytes);
    Ok(output)
}

fn copy_string(bytes: &[u8]) -> Result<String, WasmError> {
    let value =
        core::str::from_utf8(bytes).map_err(|_| WasmError::Decode("invalid replay identity"))?;
    let mut output = String::new();
    output
        .try_reserve_exact(value.len())
        .map_err(|_| WasmError::Trap("replay allocation"))?;
    output.push_str(value);
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decoder_rejects_declared_work_before_allocating_it() {
        let mut bytes = [0u8; HEADER_BYTES];
        bytes[0..4].copy_from_slice(MAGIC);
        bytes[4..6].copy_from_slice(&FORMAT_VERSION.to_le_bytes());
        bytes[6..8].copy_from_slice(&(HEADER_BYTES as u16).to_le_bytes());
        bytes[48..50].copy_from_slice(&3u16.to_le_bytes());
        bytes[50..52].copy_from_slice(&1u16.to_le_bytes());
        bytes[52..56].copy_from_slice(&1u32.to_le_bytes());
        bytes[56..60].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(ReplayTraceV1::decode(&bytes).is_err());
    }
}
