#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
use std::ffi::c_void;

#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
use windows_sys::Win32::Foundation::{
    ERROR_ACCESS_DENIED, ERROR_MOD_NOT_FOUND, ERROR_PROC_NOT_FOUND, FreeLibrary, GetLastError,
    HMODULE,
};
#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
use windows_sys::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};

use crate::contract::native_virtualization::{
    NativeVirtualizationBackend, NativeVirtualizationFacts,
};

const BACKEND: NativeVirtualizationBackend = NativeVirtualizationBackend::WindowsHypervisorPlatform;
#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
const WHV_CAPABILITY_CODE_HYPERVISOR_PRESENT: i32 = 0;
#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
const E_ACCESSDENIED: i32 = 0x8007_0005_u32 as i32;
#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
const E_BAD_LENGTH: i32 = 0x8007_0018_u32 as i32;
#[cfg(target_arch = "aarch64")]
const WHV_CAPABILITY_CODE_FEATURES: i32 = 1;
#[cfg(target_arch = "aarch64")]
const WHV_FEATURE_ARM64_SUPPORT: u64 = 1 << 11;

#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
type GetCapability = unsafe extern "system" fn(i32, *mut c_void, u32, *mut u32) -> i32;

#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
struct Library(HMODULE);

#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
impl Drop for Library {
    fn drop(&mut self) {
        unsafe { FreeLibrary(self.0) };
    }
}

pub(crate) fn probe() -> NativeVirtualizationFacts {
    #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
    return NativeVirtualizationFacts::unavailable(BACKEND);

    #[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
    probe_supported_architecture()
}

#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
fn probe_supported_architecture() -> NativeVirtualizationFacts {
    let name = "WinHvPlatform.dll\0".encode_utf16().collect::<Vec<_>>();
    let module = unsafe { LoadLibraryW(name.as_ptr()) };
    if module.is_null() {
        return classify_discovery_error(unsafe { GetLastError() });
    }
    let module = Library(module);
    let Some(procedure) =
        (unsafe { GetProcAddress(module.0, c"WHvGetCapability".as_ptr().cast()) })
    else {
        return classify_discovery_error(unsafe { GetLastError() });
    };
    let get_capability: GetCapability = unsafe { std::mem::transmute(procedure) };

    let present = match query_u64(get_capability, WHV_CAPABILITY_CODE_HYPERVISOR_PRESENT) {
        Ok(value) => value,
        Err(status) => return classify_hresult(status),
    };
    if present == 0 {
        return NativeVirtualizationFacts::unavailable(BACKEND);
    }

    #[cfg(target_arch = "aarch64")]
    {
        let features = match query_u64(get_capability, WHV_CAPABILITY_CODE_FEATURES) {
            Ok(value) => value,
            Err(status) => return classify_hresult(status),
        };
        if features & WHV_FEATURE_ARM64_SUPPORT == 0 {
            return NativeVirtualizationFacts::unavailable(BACKEND);
        }
    }

    NativeVirtualizationFacts::available(BACKEND, None)
}

#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
fn query_u64(get_capability: GetCapability, code: i32) -> Result<u64, i32> {
    let mut value = 0_u64;
    let mut written = 0_u32;
    let status = unsafe {
        get_capability(
            code,
            (&mut value as *mut u64).cast(),
            std::mem::size_of::<u64>() as u32,
            &mut written,
        )
    };
    if status < 0 {
        Err(status)
    } else if written < std::mem::size_of::<u32>() as u32 {
        Err(E_BAD_LENGTH)
    } else {
        Ok(value)
    }
}

#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
fn classify_hresult(status: i32) -> NativeVirtualizationFacts {
    if status == E_ACCESSDENIED {
        NativeVirtualizationFacts::access_denied(BACKEND, i64::from(status))
    } else {
        NativeVirtualizationFacts::failed(BACKEND, i64::from(status))
    }
}

#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
fn classify_discovery_error(code: u32) -> NativeVirtualizationFacts {
    match code {
        ERROR_MOD_NOT_FOUND | ERROR_PROC_NOT_FOUND => {
            NativeVirtualizationFacts::unavailable_with_code(BACKEND, i64::from(code))
        }
        ERROR_ACCESS_DENIED => NativeVirtualizationFacts::access_denied(BACKEND, i64::from(code)),
        _ => NativeVirtualizationFacts::failed(BACKEND, i64::from(code)),
    }
}
