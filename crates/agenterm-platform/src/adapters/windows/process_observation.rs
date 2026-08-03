use crate::contract::process_observation::ProcessObservation;

// WaitForSingleObject requires SYNCHRONIZE, which
// PROCESS_QUERY_LIMITED_INFORMATION does not include on its own.
const SYNCHRONIZE_ACCESS: u32 = 0x0010_0000;

pub(crate) fn observe(pid: u32) -> ProcessObservation {
    use windows_sys::Win32::{
        Foundation::{
            CloseHandle, ERROR_ACCESS_DENIED, ERROR_INVALID_PARAMETER, FILETIME, GetLastError,
            WAIT_OBJECT_0, WAIT_TIMEOUT,
        },
        System::Threading::{
            GetProcessTimes, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, WaitForSingleObject,
        },
    };

    let process = unsafe {
        OpenProcess(
            PROCESS_QUERY_LIMITED_INFORMATION | SYNCHRONIZE_ACCESS,
            0,
            pid,
        )
    };
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
    // A process handle becomes signaled exactly when the process terminates.
    // This is unambiguous, unlike comparing GetExitCodeProcess's output
    // against STILL_ACTIVE (259): a process that legitimately exits with
    // code 259 would otherwise be reported as still running.
    match unsafe { WaitForSingleObject(process, 0) } {
        WAIT_TIMEOUT => {}
        WAIT_OBJECT_0 => {
            unsafe { CloseHandle(process) };
            return ProcessObservation::Dead {
                reason: "process_exited".to_owned(),
            };
        }
        _ => {
            unsafe { CloseHandle(process) };
            return ProcessObservation::Unknown {
                reason: "process_exit_query_failed".to_owned(),
            };
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn still_active_exit_code_is_correctly_reported_as_dead() {
        // 259 is STILL_ACTIVE. GetExitCodeProcess alone can't distinguish "a
        // running process" from "a process that exited with code 259" --
        // this is exactly the ambiguity WaitForSingleObject's signaled state
        // resolves.
        let mut child = std::process::Command::new("cmd.exe")
            .args(["/d", "/c", "exit 259"])
            .spawn()
            .expect("spawn still-active exit code probe");
        let status = child.wait().expect("reap still-active exit code probe");
        assert_eq!(status.code(), Some(259));
        assert!(matches!(
            observe(child.id()),
            ProcessObservation::Dead { .. }
        ));
    }
}
