use crate::contract::process_observation::ProcessObservation;

pub(crate) fn observe(pid: u32) -> ProcessObservation {
    use windows_sys::Win32::{
        Foundation::{
            CloseHandle, ERROR_ACCESS_DENIED, ERROR_INVALID_PARAMETER, FILETIME, GetLastError,
            STILL_ACTIVE,
        },
        System::Threading::{
            GetExitCodeProcess, GetProcessTimes, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
        },
    };

    let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    if process.is_null() {
        let error = unsafe { GetLastError() };
        return if error == ERROR_INVALID_PARAMETER {
            ProcessObservation::Dead {
                reason: "process_not_found".to_owned(),
            }
        } else if error == ERROR_ACCESS_DENIED {
            ProcessObservation::Unknown {
                reason: "process_access_denied".to_owned(),
            }
        } else {
            ProcessObservation::Unknown {
                reason: format!("process_open_failed:{error}"),
            }
        };
    }
    let mut exit_code = 0;
    if unsafe { GetExitCodeProcess(process, &mut exit_code) } == 0 {
        unsafe { CloseHandle(process) };
        return ProcessObservation::Unknown {
            reason: "process_exit_query_failed".to_owned(),
        };
    }
    if exit_code != STILL_ACTIVE as u32 {
        unsafe { CloseHandle(process) };
        return ProcessObservation::Dead {
            reason: "process_exited".to_owned(),
        };
    }
    let mut creation: FILETIME = unsafe { std::mem::zeroed() };
    let mut exit: FILETIME = unsafe { std::mem::zeroed() };
    let mut kernel: FILETIME = unsafe { std::mem::zeroed() };
    let mut user: FILETIME = unsafe { std::mem::zeroed() };
    let queried =
        unsafe { GetProcessTimes(process, &mut creation, &mut exit, &mut kernel, &mut user) } != 0;
    unsafe { CloseHandle(process) };
    if !queried {
        return ProcessObservation::Unknown {
            reason: "process_start_identity_query_failed".to_owned(),
        };
    }
    let ticks = (u64::from(creation.dwHighDateTime) << 32) | u64::from(creation.dwLowDateTime);
    ProcessObservation::Live {
        start_identity: Some(format!("windows-filetime:{ticks}")),
    }
}
