//! Versioned C ownership boundary for Swift/Objective-C hosts.

use core::cell::Cell;
use core::mem::size_of;
use core::ptr;
use core::slice;
use std::boxed::Box;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::thread::{self, ThreadId};

use crate::{GameFrame, GameInput, GameLimits, GameRuntime, Limits, WasmError};

pub const STATUS_OK: i32 = 0;
pub const STATUS_INVALID_ARGUMENT: i32 = 1;
pub const STATUS_DECODE: i32 = 2;
pub const STATUS_TRAP: i32 = 3;
pub const STATUS_BUFFER_TOO_SMALL: i32 = 4;
pub const STATUS_WRONG_THREAD: i32 = 5;
pub const STATUS_FAILED_INSTANCE: i32 = 6;
pub const STATUS_PANIC: i32 = 7;

thread_local! {
    static LAST_ERROR: Cell<&'static str> = const { Cell::new("") };
}

#[repr(C)]
pub struct TinyArcadeConfigV1 {
    pub struct_size: u32,
    pub max_table_elems: u32,
    pub max_memory_pages: u32,
    pub max_steps: u64,
    pub max_render_bytes: u32,
    pub max_audio_bytes: u32,
    pub max_state_bytes: u32,
    pub rng_seed: u32,
}

pub struct TinyArcadeRuntimeV1 {
    owner: ThreadId,
    runtime: GameRuntime,
    frame: Option<GameFrame>,
    snapshot: Vec<u8>,
}

#[derive(Clone, Copy)]
struct FfiError {
    status: i32,
    message: &'static str,
}

impl FfiError {
    const fn new(status: i32, message: &'static str) -> Self {
        Self { status, message }
    }
}

fn set_error(message: &'static str) {
    LAST_ERROR.with(|slot| slot.set(message));
}

fn boundary(f: impl FnOnce() -> Result<(), FfiError>) -> i32 {
    set_error("");
    match catch_unwind(AssertUnwindSafe(f)) {
        Ok(Ok(())) => STATUS_OK,
        Ok(Err(error)) => {
            set_error(error.message);
            error.status
        }
        Err(_) => {
            set_error("panic inside tinyarcade runtime");
            STATUS_PANIC
        }
    }
}

fn wasm_error(error: WasmError) -> FfiError {
    match error {
        WasmError::Decode(message) => FfiError::new(STATUS_DECODE, message),
        WasmError::Trap("game instance failed") => {
            FfiError::new(STATUS_FAILED_INSTANCE, "game instance failed")
        }
        WasmError::Trap(message) => FfiError::new(STATUS_TRAP, message),
    }
}

unsafe fn runtime_mut<'a>(
    runtime: *mut TinyArcadeRuntimeV1,
) -> Result<&'a mut TinyArcadeRuntimeV1, FfiError> {
    let runtime = unsafe { runtime.as_mut() }.ok_or(FfiError::new(
        STATUS_INVALID_ARGUMENT,
        "null runtime handle",
    ))?;
    if runtime.owner != thread::current().id() {
        return Err(FfiError::new(
            STATUS_WRONG_THREAD,
            "runtime used from a different thread",
        ));
    }
    Ok(runtime)
}

unsafe fn copy_bytes(
    bytes: &[u8],
    output: *mut u8,
    capacity: usize,
    output_len: *mut usize,
) -> Result<(), FfiError> {
    if output_len.is_null() {
        return Err(FfiError::new(STATUS_INVALID_ARGUMENT, "null output length"));
    }
    unsafe { output_len.write(bytes.len()) };
    if capacity < bytes.len() || (output.is_null() && !bytes.is_empty()) {
        return Err(FfiError::new(
            STATUS_BUFFER_TOO_SMALL,
            "output buffer too small",
        ));
    }
    if !bytes.is_empty() {
        unsafe { ptr::copy_nonoverlapping(bytes.as_ptr(), output, bytes.len()) };
    }
    Ok(())
}

#[unsafe(no_mangle)]
pub extern "C" fn tinyarcade_v1_abi_version() -> u32 {
    1 << 16
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tinyarcade_v1_default_config(config: *mut TinyArcadeConfigV1) -> i32 {
    boundary(|| {
        if config.is_null() {
            return Err(FfiError::new(STATUS_INVALID_ARGUMENT, "null config"));
        }
        let defaults = Limits::default();
        let game = GameLimits::default();
        let value = TinyArcadeConfigV1 {
            struct_size: size_of::<TinyArcadeConfigV1>() as u32,
            max_table_elems: defaults.max_table_elems as u32,
            max_memory_pages: defaults.max_memory_pages as u32,
            max_steps: defaults.max_steps,
            max_render_bytes: game.max_render_bytes as u32,
            max_audio_bytes: game.max_audio_bytes as u32,
            max_state_bytes: game.max_state_bytes as u32,
            rng_seed: 1,
        };
        unsafe { config.write(value) };
        Ok(())
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tinyarcade_v1_open(
    wasm: *const u8,
    wasm_len: usize,
    config: *const TinyArcadeConfigV1,
    output: *mut *mut TinyArcadeRuntimeV1,
) -> i32 {
    boundary(|| {
        if output.is_null() {
            return Err(FfiError::new(
                STATUS_INVALID_ARGUMENT,
                "null runtime output",
            ));
        }
        unsafe { output.write(ptr::null_mut()) };
        let config = unsafe { config.as_ref() }
            .ok_or(FfiError::new(STATUS_INVALID_ARGUMENT, "null config"))?;
        if config.struct_size < size_of::<TinyArcadeConfigV1>() as u32
            || wasm.is_null()
            || wasm_len == 0
            || config.max_table_elems == 0
            || config.max_memory_pages == 0
            || config.max_steps == 0
        {
            return Err(FfiError::new(
                STATUS_INVALID_ARGUMENT,
                "invalid runtime configuration",
            ));
        }
        let bytes = unsafe { slice::from_raw_parts(wasm, wasm_len) };
        let runtime = GameRuntime::from_bytes(
            bytes,
            Limits {
                max_table_elems: config.max_table_elems as usize,
                max_memory_pages: config.max_memory_pages as usize,
                max_steps: config.max_steps,
            },
            GameLimits {
                max_render_bytes: config.max_render_bytes as usize,
                max_audio_bytes: config.max_audio_bytes as usize,
                max_state_bytes: config.max_state_bytes as usize,
            },
            config.rng_seed,
        )
        .map_err(wasm_error)?;
        let handle = Box::new(TinyArcadeRuntimeV1 {
            owner: thread::current().id(),
            runtime,
            frame: None,
            snapshot: Vec::new(),
        });
        unsafe { output.write(Box::into_raw(handle)) };
        Ok(())
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tinyarcade_v1_close(runtime: *mut TinyArcadeRuntimeV1) -> i32 {
    boundary(|| {
        unsafe { runtime_mut(runtime)? };
        drop(unsafe { Box::from_raw(runtime) });
        Ok(())
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tinyarcade_v1_tick(
    runtime: *mut TinyArcadeRuntimeV1,
    buttons: u32,
    clock_ms: u32,
) -> i32 {
    boundary(|| {
        let runtime = unsafe { runtime_mut(runtime)? };
        runtime.frame = Some(
            runtime
                .runtime
                .tick(GameInput { buttons, clock_ms })
                .map_err(wasm_error)?,
        );
        Ok(())
    })
}

unsafe fn copy_frame(
    runtime: *mut TinyArcadeRuntimeV1,
    render: bool,
    output: *mut u8,
    capacity: usize,
    output_len: *mut usize,
) -> i32 {
    boundary(|| {
        let runtime = unsafe { runtime_mut(runtime)? };
        let frame = runtime
            .frame
            .as_ref()
            .ok_or(FfiError::new(STATUS_INVALID_ARGUMENT, "no completed frame"))?;
        let bytes = if render { &frame.render } else { &frame.audio };
        unsafe { copy_bytes(bytes, output, capacity, output_len) }
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tinyarcade_v1_copy_render(
    runtime: *mut TinyArcadeRuntimeV1,
    output: *mut u8,
    capacity: usize,
    output_len: *mut usize,
) -> i32 {
    unsafe { copy_frame(runtime, true, output, capacity, output_len) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tinyarcade_v1_copy_audio(
    runtime: *mut TinyArcadeRuntimeV1,
    output: *mut u8,
    capacity: usize,
    output_len: *mut usize,
) -> i32 {
    unsafe { copy_frame(runtime, false, output, capacity, output_len) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tinyarcade_v1_suspend(runtime: *mut TinyArcadeRuntimeV1) -> i32 {
    boundary(|| {
        let runtime = unsafe { runtime_mut(runtime)? };
        runtime.snapshot = runtime.runtime.suspend().map_err(wasm_error)?;
        Ok(())
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tinyarcade_v1_copy_snapshot(
    runtime: *mut TinyArcadeRuntimeV1,
    output: *mut u8,
    capacity: usize,
    output_len: *mut usize,
) -> i32 {
    boundary(|| {
        let runtime = unsafe { runtime_mut(runtime)? };
        if runtime.snapshot.is_empty() {
            return Err(FfiError::new(
                STATUS_INVALID_ARGUMENT,
                "no completed snapshot",
            ));
        }
        unsafe { copy_bytes(&runtime.snapshot, output, capacity, output_len) }
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tinyarcade_v1_resume(
    runtime: *mut TinyArcadeRuntimeV1,
    snapshot: *const u8,
    snapshot_len: usize,
) -> i32 {
    boundary(|| {
        let runtime = unsafe { runtime_mut(runtime)? };
        if snapshot.is_null() || snapshot_len == 0 {
            return Err(FfiError::new(
                STATUS_INVALID_ARGUMENT,
                "invalid snapshot input",
            ));
        }
        let snapshot = unsafe { slice::from_raw_parts(snapshot, snapshot_len) };
        runtime.runtime.resume(snapshot).map_err(wasm_error)?;
        runtime.frame = None;
        Ok(())
    })
}

unsafe fn copy_manifest_string(
    runtime: *mut TinyArcadeRuntimeV1,
    game_id: bool,
    output: *mut u8,
    capacity: usize,
    output_len: *mut usize,
) -> i32 {
    boundary(|| {
        let runtime = unsafe { runtime_mut(runtime)? };
        let manifest = runtime.runtime.manifest();
        let value = if game_id {
            manifest.game_id.as_bytes()
        } else {
            manifest.game_version.as_bytes()
        };
        unsafe { copy_bytes(value, output, capacity, output_len) }
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tinyarcade_v1_copy_game_id(
    runtime: *mut TinyArcadeRuntimeV1,
    output: *mut u8,
    capacity: usize,
    output_len: *mut usize,
) -> i32 {
    unsafe { copy_manifest_string(runtime, true, output, capacity, output_len) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tinyarcade_v1_copy_game_version(
    runtime: *mut TinyArcadeRuntimeV1,
    output: *mut u8,
    capacity: usize,
    output_len: *mut usize,
) -> i32 {
    unsafe { copy_manifest_string(runtime, false, output, capacity, output_len) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tinyarcade_v1_is_failed(
    runtime: *mut TinyArcadeRuntimeV1,
    output: *mut i32,
) -> i32 {
    boundary(|| {
        let runtime = unsafe { runtime_mut(runtime)? };
        if output.is_null() {
            return Err(FfiError::new(
                STATUS_INVALID_ARGUMENT,
                "null failed-state output",
            ));
        }
        unsafe { output.write(i32::from(runtime.runtime.is_failed())) };
        Ok(())
    })
}

/// Copy the last error for the calling thread. This call intentionally does
/// not clear the error before reading it.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tinyarcade_v1_last_error(
    output: *mut u8,
    capacity: usize,
    output_len: *mut usize,
) -> i32 {
    let message = LAST_ERROR.with(Cell::get);
    match catch_unwind(AssertUnwindSafe(|| unsafe {
        copy_bytes(message.as_bytes(), output, capacity, output_len)
    })) {
        Ok(Ok(())) => STATUS_OK,
        Ok(Err(error)) => error.status,
        Err(_) => STATUS_PANIC,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::mem::MaybeUninit;

    fn leb(output: &mut Vec<u8>, mut value: usize) {
        loop {
            let mut byte = (value & 0x7f) as u8;
            value >>= 7;
            if value != 0 {
                byte |= 0x80;
            }
            output.push(byte);
            if value == 0 {
                break;
            }
        }
    }

    fn name(output: &mut Vec<u8>, value: &str) {
        leb(output, value.len());
        output.extend_from_slice(value.as_bytes());
    }

    fn section(module: &mut Vec<u8>, id: u8, payload: &[u8]) {
        module.push(id);
        leb(module, payload.len());
        module.extend_from_slice(payload);
    }

    fn body(code: &[u8]) -> Vec<u8> {
        let mut body = vec![0];
        body.extend_from_slice(code);
        body
    }

    fn cartridge() -> Vec<u8> {
        let mut module = b"\0asm\x01\0\0\0".to_vec();
        let mut manifest = Vec::new();
        name(&mut manifest, "tinyarcade.manifest.v1");
        manifest.extend_from_slice(b"TAM1");
        manifest.extend_from_slice(&1u32.to_le_bytes());
        manifest.extend_from_slice(&1u32.to_le_bytes());
        for value in ["c.test", "1.0.0"] {
            manifest.extend_from_slice(&(value.len() as u16).to_le_bytes());
            manifest.extend_from_slice(value.as_bytes());
        }
        manifest.extend_from_slice(&0u16.to_le_bytes());
        section(&mut module, 0, &manifest);
        section(
            &mut module,
            1,
            &[
                0x02, 0x60, 0x00, 0x01, 0x7f, 0x60, 0x02, 0x7f, 0x7f, 0x01, 0x7f,
            ],
        );
        let imports = ["save_state", "load_state", "submit_render"];
        let mut import_section = vec![imports.len() as u8];
        for field in imports {
            name(&mut import_section, "tinyarcade:core/v1");
            name(&mut import_section, field);
            import_section.extend_from_slice(&[0x00, 0x01]);
        }
        section(&mut module, 2, &import_section);
        section(&mut module, 3, &[0x05, 0, 0, 0, 0, 0]);
        section(&mut module, 5, &[0x01, 0x00, 0x01]);
        let mut exports = vec![0x05];
        for (field, index) in [
            ("game_abi_version", 3usize),
            ("game_init", 4),
            ("game_tick", 5),
            ("game_suspend", 6),
            ("game_resume", 7),
        ] {
            name(&mut exports, field);
            exports.push(0);
            leb(&mut exports, index);
        }
        section(&mut module, 7, &exports);
        let functions = [
            body(&[0x41, 0x01, 0x0b]),
            body(&[0x41, 0x00, 0x0b]),
            body(&[0x41, 0x00, 0x41, 0x01, 0x10, 0x02, 0x1a, 0x41, 0x00, 0x0b]),
            body(&[0x41, 0x00, 0x41, 0x01, 0x10, 0x00, 0x1a, 0x41, 0x00, 0x0b]),
            body(&[0x41, 0x00, 0x41, 0x01, 0x10, 0x01, 0x1a, 0x41, 0x00, 0x0b]),
        ];
        let mut code = vec![0x05];
        for function in &functions {
            leb(&mut code, function.len());
            code.extend_from_slice(function);
        }
        section(&mut module, 10, &code);
        section(&mut module, 11, &[0x01, 0x00, 0x41, 0x00, 0x0b, 0x01, 0x09]);
        module
    }

    unsafe fn config() -> TinyArcadeConfigV1 {
        let mut config = MaybeUninit::uninit();
        assert_eq!(
            unsafe { tinyarcade_v1_default_config(config.as_mut_ptr()) },
            STATUS_OK
        );
        unsafe { config.assume_init() }
    }

    unsafe fn open(wasm: &[u8]) -> *mut TinyArcadeRuntimeV1 {
        let config = unsafe { config() };
        let mut runtime = ptr::null_mut();
        assert_eq!(
            unsafe { tinyarcade_v1_open(wasm.as_ptr(), wasm.len(), &config, &mut runtime) },
            STATUS_OK
        );
        assert!(!runtime.is_null());
        runtime
    }

    #[test]
    fn c_handle_drives_frame_snapshot_resume_and_thread_owner() {
        let wasm = cartridge();
        let runtime = unsafe { open(&wasm) };
        assert_eq!(tinyarcade_v1_abi_version(), 1 << 16);
        assert_eq!(unsafe { tinyarcade_v1_tick(runtime, 0, 16) }, STATUS_OK);

        let mut required = 0usize;
        assert_eq!(
            unsafe { tinyarcade_v1_copy_render(runtime, ptr::null_mut(), 0, &mut required) },
            STATUS_BUFFER_TOO_SMALL
        );
        assert_eq!(required, 1);

        let mut error_len = 0usize;
        assert_eq!(
            unsafe { tinyarcade_v1_last_error(ptr::null_mut(), 0, &mut error_len) },
            STATUS_BUFFER_TOO_SMALL
        );
        let mut error = vec![0; error_len];
        assert_eq!(
            unsafe { tinyarcade_v1_last_error(error.as_mut_ptr(), error.len(), &mut error_len) },
            STATUS_OK
        );
        assert_eq!(error, b"output buffer too small");

        let mut render = [0u8; 1];
        assert_eq!(
            unsafe {
                tinyarcade_v1_copy_render(runtime, render.as_mut_ptr(), render.len(), &mut required)
            },
            STATUS_OK
        );
        assert_eq!(render, [9]);

        assert_eq!(unsafe { tinyarcade_v1_suspend(runtime) }, STATUS_OK);
        let mut snapshot_len = 0usize;
        assert_eq!(
            unsafe { tinyarcade_v1_copy_snapshot(runtime, ptr::null_mut(), 0, &mut snapshot_len) },
            STATUS_BUFFER_TOO_SMALL
        );
        let mut snapshot = vec![0; snapshot_len];
        assert_eq!(
            unsafe {
                tinyarcade_v1_copy_snapshot(
                    runtime,
                    snapshot.as_mut_ptr(),
                    snapshot.len(),
                    &mut snapshot_len,
                )
            },
            STATUS_OK
        );

        let address = runtime as usize;
        assert_eq!(
            std::thread::spawn(move || {
                let mut failed = 0;
                unsafe { tinyarcade_v1_is_failed(address as *mut TinyArcadeRuntimeV1, &mut failed) }
            })
            .join()
            .expect("thread probe"),
            STATUS_WRONG_THREAD
        );

        let restored = unsafe { open(&wasm) };
        assert_eq!(
            unsafe { tinyarcade_v1_resume(restored, snapshot.as_ptr(), snapshot.len()) },
            STATUS_OK
        );
        assert_eq!(unsafe { tinyarcade_v1_close(restored) }, STATUS_OK);
        assert_eq!(unsafe { tinyarcade_v1_close(runtime) }, STATUS_OK);
    }

    #[test]
    fn c_open_nulls_output_and_preserves_decode_detail() {
        let config = unsafe { config() };
        let bad = b"not wasm";
        let mut runtime = ptr::dangling_mut::<TinyArcadeRuntimeV1>();
        assert_eq!(
            unsafe { tinyarcade_v1_open(bad.as_ptr(), bad.len(), &config, &mut runtime) },
            STATUS_DECODE
        );
        assert!(runtime.is_null());
        let mut len = 0;
        assert_eq!(
            unsafe { tinyarcade_v1_last_error(ptr::null_mut(), 0, &mut len) },
            STATUS_BUFFER_TOO_SMALL
        );
        assert!(len > 0);
    }

    #[test]
    fn c_header_declares_every_versioned_export() {
        let header =
            std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/include/tinyarcade.h"))
                .expect("read C header");
        for symbol in [
            "tinyarcade_v1_abi_version",
            "tinyarcade_v1_default_config",
            "tinyarcade_v1_open",
            "tinyarcade_v1_close",
            "tinyarcade_v1_tick",
            "tinyarcade_v1_copy_render",
            "tinyarcade_v1_copy_audio",
            "tinyarcade_v1_suspend",
            "tinyarcade_v1_copy_snapshot",
            "tinyarcade_v1_resume",
            "tinyarcade_v1_copy_game_id",
            "tinyarcade_v1_copy_game_version",
            "tinyarcade_v1_is_failed",
            "tinyarcade_v1_last_error",
        ] {
            assert!(
                header.contains(&format!("{symbol}(")),
                "C header is missing {symbol}"
            );
        }
    }
}
