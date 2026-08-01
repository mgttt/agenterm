use std::fs::OpenOptions;
use std::os::fd::AsRawFd;

use crate::contract::native_virtualization::{
    NativeVirtualizationBackend, NativeVirtualizationFacts,
};

const BACKEND: NativeVirtualizationBackend = NativeVirtualizationBackend::Kvm;
const KVM_GET_API_VERSION: libc::c_ulong = 0xAE00;
const KVM_API_VERSION: u32 = 12;

pub(crate) fn probe() -> NativeVirtualizationFacts {
    let device = match OpenOptions::new().read(true).write(true).open("/dev/kvm") {
        Ok(device) => device,
        Err(error) => return classify_io_error(&error),
    };
    let version = unsafe { libc::ioctl(device.as_raw_fd(), KVM_GET_API_VERSION) };
    if version < 0 {
        return classify_io_error(&std::io::Error::last_os_error());
    }
    let Ok(version) = u32::try_from(version) else {
        return NativeVirtualizationFacts::failed(BACKEND, i64::from(version));
    };
    if version == KVM_API_VERSION {
        NativeVirtualizationFacts::available(BACKEND, Some(version))
    } else {
        NativeVirtualizationFacts::incompatible(BACKEND, version)
    }
}

fn classify_io_error(error: &std::io::Error) -> NativeVirtualizationFacts {
    let code = error.raw_os_error().map_or(-1_i64, i64::from);
    match error.raw_os_error() {
        Some(libc::EACCES | libc::EPERM) => NativeVirtualizationFacts::access_denied(BACKEND, code),
        Some(libc::ENOENT | libc::ENODEV) => {
            NativeVirtualizationFacts::unavailable_with_code(BACKEND, code)
        }
        _ => NativeVirtualizationFacts::failed(BACKEND, code),
    }
}
