//! Windows cumulative process resource counters.

use std::time::Duration;

use crate::contract::process_metrics::{
    ProcessMetrics, ProcessMetricsError, ProcessMetricsErrorKind, checked_page_faults,
};

pub(crate) fn metrics(pid: u32) -> Result<ProcessMetrics, ProcessMetricsError> {
    if pid == 0 {
        return Err(ProcessMetricsError::new(
            ProcessMetricsErrorKind::InvalidId,
            "process ID zero does not identify one process",
        ));
    }
    use windows_sys::Win32::{
        Foundation::{CloseHandle, FILETIME},
        System::{
            ProcessStatus::{K32GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS},
            Threading::{GetProcessTimes, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION},
        },
    };
    let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    if process.is_null() {
        let error = std::io::Error::last_os_error();
        let kind = if error.raw_os_error()
            == Some(windows_sys::Win32::Foundation::ERROR_INVALID_PARAMETER as i32)
        {
            ProcessMetricsErrorKind::NotFound
        } else {
            ProcessMetricsErrorKind::Open
        };
        return Err(ProcessMetricsError::new(kind, error.to_string()));
    }
    let mut creation = FILETIME::default();
    let mut exit = FILETIME::default();
    let mut kernel = FILETIME::default();
    let mut user = FILETIME::default();
    let mut memory = PROCESS_MEMORY_COUNTERS {
        cb: std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32,
        ..Default::default()
    };
    let times_ok =
        unsafe { GetProcessTimes(process, &mut creation, &mut exit, &mut kernel, &mut user) } != 0;
    let memory_ok = unsafe { K32GetProcessMemoryInfo(process, &mut memory, memory.cb) } != 0;
    let error = (!times_ok || !memory_ok).then(std::io::Error::last_os_error);
    unsafe { CloseHandle(process) };
    if let Some(error) = error {
        return Err(ProcessMetricsError::new(
            ProcessMetricsErrorKind::Read,
            error.to_string(),
        ));
    }
    let units_100ns = filetime_ticks(kernel).saturating_add(filetime_ticks(user));
    Ok(ProcessMetrics {
        cpu_time: Duration::from_nanos(units_100ns.saturating_mul(100)),
        resident_bytes: memory.WorkingSetSize as u64,
        page_faults: checked_page_faults(u64::from(memory.PageFaultCount), None, None)?,
    })
}

fn filetime_ticks(value: windows_sys::Win32::Foundation::FILETIME) -> u64 {
    (u64::from(value.dwHighDateTime) << 32) | u64::from(value.dwLowDateTime)
}
