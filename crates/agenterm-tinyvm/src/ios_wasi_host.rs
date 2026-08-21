//! One-shot iOS-facing WASI command runner.
//!
//! This is a separately built research/host surface. It is not part of the
//! default TinyArcade XCFramework or the bundled-only game ABI.

use alloc::string::ToString;
use core::panic::AssertUnwindSafe;
use core::{mem, slice, str};
use std::panic::catch_unwind;

use crate::{
    DescriptorRights, HostContext, HostLimits, Limits, StdHostBackend, StdHostLimits,
    WASI_PROC_EXIT_TRAP, WasiPreview1, WasmError, WasmModule,
};

const MAX_WASM_BYTES: usize = 16 * 1024 * 1024;
const MAX_PATH_BYTES: usize = 4 * 1024;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct TinyWasiHostConfigV1 {
    struct_size: u32,
    max_table_elems: u32,
    max_memory_pages: u32,
    max_call_depth: u32,
    max_activation_slots: u32,
    max_host_handles: u32,
    max_guest_descriptors: u32,
    max_steps: u64,
}

impl Default for TinyWasiHostConfigV1 {
    fn default() -> Self {
        Self {
            struct_size: mem::size_of::<Self>() as u32,
            max_table_elems: 4_096,
            max_memory_pages: 256,
            max_call_depth: 256,
            max_activation_slots: 65_536,
            max_host_handles: 64,
            max_guest_descriptors: 64,
            max_steps: 10_000_000,
        }
    }
}

#[repr(C)]
#[cfg_attr(test, derive(Debug))]
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum TinyWasiHostStatusV1 {
    Ok = 0,
    InvalidArgument = 1,
    DecodeError = 2,
    GuestTrap = 3,
    StorageError = 4,
    Panic = 5,
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tinyvm_wasi_host_v1_default_config(
    output: *mut TinyWasiHostConfigV1,
) -> TinyWasiHostStatusV1 {
    ffi(|| {
        let output = unsafe { output.as_mut() }.ok_or(TinyWasiHostStatusV1::InvalidArgument)?;
        *output = TinyWasiHostConfigV1::default();
        Ok(())
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tinyvm_wasi_host_v1_run(
    wasm: *const u8,
    wasm_len: usize,
    preopen_path: *const u8,
    preopen_path_len: usize,
    config: *const TinyWasiHostConfigV1,
    did_exit: *mut u32,
    exit_code: *mut u32,
) -> TinyWasiHostStatusV1 {
    ffi(|| {
        let did_exit = unsafe { did_exit.as_mut() }.ok_or(TinyWasiHostStatusV1::InvalidArgument)?;
        let exit_code =
            unsafe { exit_code.as_mut() }.ok_or(TinyWasiHostStatusV1::InvalidArgument)?;
        *did_exit = 0;
        *exit_code = 0;
        let config = unsafe { config.as_ref() }.ok_or(TinyWasiHostStatusV1::InvalidArgument)?;
        validate_config(config)?;
        let wasm = unsafe { borrowed_bytes(wasm, wasm_len, MAX_WASM_BYTES)? };
        let path = unsafe { borrowed_bytes(preopen_path, preopen_path_len, MAX_PATH_BYTES)? };
        let path = str::from_utf8(path).map_err(|_| TinyWasiHostStatusV1::InvalidArgument)?;
        if path.is_empty() {
            return Err(TinyWasiHostStatusV1::InvalidArgument);
        }

        let mut backend = StdHostBackend::new(StdHostLimits {
            max_handles: config.max_host_handles,
        });
        let native = backend
            .open_ambient_preopen(path)
            .map_err(|_| TinyWasiHostStatusV1::StorageError)?;
        let mut context = HostContext::new(
            backend,
            HostLimits {
                max_descriptors: config.max_guest_descriptors,
                ..HostLimits::default()
            },
        );
        let rights = DescriptorRights::PATH_OPEN
            .union(DescriptorRights::PATH_UNLINK)
            .union(DescriptorRights::READ)
            .union(DescriptorRights::WRITE)
            .union(DescriptorRights::SEEK)
            .union(DescriptorRights::STAT);
        context
            .register_preopen(native, "/save".to_string(), rights)
            .map_err(|_| TinyWasiHostStatusV1::StorageError)?;
        let wasi = WasiPreview1::new(context);
        let limits = Limits {
            max_table_elems: config.max_table_elems as usize,
            max_memory_pages: config.max_memory_pages as usize,
            max_steps: config.max_steps,
            max_call_depth: config.max_call_depth as usize,
            max_activation_slots: config.max_activation_slots as usize,
        };
        let mut module = WasmModule::from_bytes_with(wasm, limits).map_err(wasm_status)?;
        wasi.bind(&mut module).map_err(wasm_status)?;
        let mut instance = match module.instantiate() {
            Ok(instance) => instance,
            Err(error) => return finish_error(&wasi, error, did_exit, exit_code),
        };
        match instance.invoke_by_name("_start", &[]) {
            Ok(results) if results.is_empty() => Ok(()),
            Ok(_) => Err(TinyWasiHostStatusV1::GuestTrap),
            Err(error) => finish_error(&wasi, error, did_exit, exit_code),
        }
    })
}

fn finish_error(
    wasi: &WasiPreview1<StdHostBackend>,
    error: WasmError,
    did_exit: &mut u32,
    exit_code: &mut u32,
) -> Result<(), TinyWasiHostStatusV1> {
    if matches!(error, WasmError::Trap(message) if message == WASI_PROC_EXIT_TRAP) {
        let code = wasi
            .take_exit_code()
            .ok_or(TinyWasiHostStatusV1::GuestTrap)?;
        *did_exit = 1;
        *exit_code = code;
        Ok(())
    } else {
        Err(wasm_status(error))
    }
}

fn validate_config(config: &TinyWasiHostConfigV1) -> Result<(), TinyWasiHostStatusV1> {
    if config.struct_size as usize != mem::size_of::<TinyWasiHostConfigV1>()
        || config.max_table_elems == 0
        || config.max_memory_pages == 0
        || config.max_call_depth == 0
        || config.max_activation_slots == 0
        || config.max_host_handles == 0
        || config.max_guest_descriptors == 0
        || config.max_steps == 0
    {
        return Err(TinyWasiHostStatusV1::InvalidArgument);
    }
    Ok(())
}

fn wasm_status(error: WasmError) -> TinyWasiHostStatusV1 {
    match error {
        WasmError::Decode(_) => TinyWasiHostStatusV1::DecodeError,
        WasmError::Trap(_) => TinyWasiHostStatusV1::GuestTrap,
    }
}

unsafe fn borrowed_bytes<'a>(
    pointer: *const u8,
    length: usize,
    maximum: usize,
) -> Result<&'a [u8], TinyWasiHostStatusV1> {
    if length == 0 || length > maximum || pointer.is_null() {
        return Err(TinyWasiHostStatusV1::InvalidArgument);
    }
    Ok(unsafe { slice::from_raw_parts(pointer, length) })
}

fn ffi(operation: impl FnOnce() -> Result<(), TinyWasiHostStatusV1>) -> TinyWasiHostStatusV1 {
    match catch_unwind(AssertUnwindSafe(operation)) {
        Ok(Ok(())) => TinyWasiHostStatusV1::Ok,
        Ok(Err(status)) => status,
        Err(_) => TinyWasiHostStatusV1::Panic,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::ptr;

    #[test]
    fn config_layout_and_invalid_pointers_fail_closed() {
        assert_eq!(mem::size_of::<TinyWasiHostConfigV1>(), 40);
        let mut config = TinyWasiHostConfigV1::default();
        assert_eq!(
            unsafe { tinyvm_wasi_host_v1_default_config(&mut config) },
            TinyWasiHostStatusV1::Ok
        );
        let mut did_exit = 9;
        let mut exit_code = 9;
        assert_eq!(
            unsafe {
                tinyvm_wasi_host_v1_run(
                    ptr::null(),
                    0,
                    ptr::null(),
                    0,
                    &config,
                    &mut did_exit,
                    &mut exit_code,
                )
            },
            TinyWasiHostStatusV1::InvalidArgument
        );
        assert_eq!(did_exit, 0);
        assert_eq!(exit_code, 0);
    }
}
