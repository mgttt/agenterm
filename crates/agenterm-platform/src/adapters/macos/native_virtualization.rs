use crate::contract::native_virtualization::{
    NativeVirtualizationBackend, NativeVirtualizationFacts,
};

const BACKEND: NativeVirtualizationBackend = NativeVirtualizationBackend::HypervisorFramework;

pub(crate) fn probe() -> NativeVirtualizationFacts {
    let mut supported = 0_i32;
    let mut size = std::mem::size_of_val(&supported);
    let status = unsafe {
        libc::sysctlbyname(
            c"kern.hv_support".as_ptr(),
            (&mut supported as *mut i32).cast(),
            &mut size,
            std::ptr::null_mut(),
            0,
        )
    };
    if status == 0 {
        if size < std::mem::size_of_val(&supported) {
            return NativeVirtualizationFacts::failed(BACKEND, i64::from(libc::EOVERFLOW));
        }
        return if supported == 0 {
            NativeVirtualizationFacts::unavailable(BACKEND)
        } else {
            NativeVirtualizationFacts::available(BACKEND, None)
        };
    }

    let error = std::io::Error::last_os_error();
    let code = error.raw_os_error().map_or(-1_i64, i64::from);
    match error.raw_os_error() {
        Some(libc::EACCES | libc::EPERM) => NativeVirtualizationFacts::access_denied(BACKEND, code),
        Some(libc::ENOENT | libc::ENOTSUP) => {
            NativeVirtualizationFacts::unavailable_with_code(BACKEND, code)
        }
        _ => NativeVirtualizationFacts::failed(BACKEND, code),
    }
}
