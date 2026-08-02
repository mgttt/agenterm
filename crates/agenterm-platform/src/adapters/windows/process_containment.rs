use std::os::windows::io::{AsHandle as _, AsRawHandle as _, FromRawHandle as _, OwnedHandle};

use windows_sys::Win32::{
    Foundation::{
        ERROR_ALREADY_EXISTS, ERROR_FILE_NOT_FOUND, ERROR_MORE_DATA, GetLastError, SetLastError,
    },
    System::{
        JobObjects::{
            AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_CPU_RATE_CONTROL_ENABLE,
            JOB_OBJECT_CPU_RATE_CONTROL_HARD_CAP, JOB_OBJECT_LIMIT_ACTIVE_PROCESS,
            JOB_OBJECT_LIMIT_BREAKAWAY_OK, JOB_OBJECT_LIMIT_JOB_MEMORY,
            JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE, JOBOBJECT_BASIC_LIMIT_INFORMATION,
            JOBOBJECT_BASIC_PROCESS_ID_LIST, JOBOBJECT_CPU_RATE_CONTROL_INFORMATION,
            JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectBasicProcessIdList,
            JobObjectCpuRateControlInformation, JobObjectExtendedLimitInformation, OpenJobObjectW,
            QueryInformationJobObject, SetInformationJobObject, TerminateJobObject,
        },
        SystemServices::{JOB_OBJECT_ASSIGN_PROCESS, JOB_OBJECT_QUERY, JOB_OBJECT_TERMINATE},
    },
};

use crate::{
    process_containment::{
        ProcessContainmentError, ProcessContainmentErrorKind, ProcessContainmentOptions,
    },
    process_reference::ProcessReference,
};

pub struct ProcessContainment {
    handle: Option<OwnedHandle>,
}

impl ProcessContainment {
    pub(crate) fn create(
        name: Option<&str>,
        options: ProcessContainmentOptions,
    ) -> Result<Self, ProcessContainmentError> {
        let name = name.map(wide);
        unsafe { SetLastError(0) };
        let raw = unsafe {
            CreateJobObjectW(
                std::ptr::null(),
                name.as_ref()
                    .map_or(std::ptr::null(), |value| value.as_ptr()),
            )
        };
        if raw.is_null() {
            return Err(last_error("create-process-containment"));
        }
        let handle = unsafe { OwnedHandle::from_raw_handle(raw) };
        if name.is_some() && unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
            return Err(ProcessContainmentError::new(
                ProcessContainmentErrorKind::AlreadyExists,
                "create-process-containment",
                Some(ERROR_ALREADY_EXISTS),
                "a containment object with this name already exists",
            ));
        }
        let containment = Self {
            handle: Some(handle),
        };
        containment.configure(options)?;
        Ok(containment)
    }

    pub(crate) fn open(name: &str) -> Result<Self, ProcessContainmentError> {
        let name = wide(name);
        let access = JOB_OBJECT_ASSIGN_PROCESS | JOB_OBJECT_QUERY | JOB_OBJECT_TERMINATE;
        let raw = unsafe { OpenJobObjectW(access, 0, name.as_ptr()) };
        if raw.is_null() {
            let code = unsafe { GetLastError() };
            return Err(ProcessContainmentError::new(
                if code == ERROR_FILE_NOT_FOUND {
                    ProcessContainmentErrorKind::NotFound
                } else {
                    ProcessContainmentErrorKind::NativeFailure
                },
                "open-process-containment",
                Some(code),
                format!("OpenJobObjectW returned error {code}"),
            ));
        }
        Ok(Self {
            handle: Some(unsafe { OwnedHandle::from_raw_handle(raw) }),
        })
    }

    pub(crate) fn assign(&self, process: &ProcessReference) -> Result<(), ProcessContainmentError> {
        if unsafe {
            AssignProcessToJobObject(
                self.handle("assign-process-containment")?.as_raw_handle(),
                process.as_handle().as_raw_handle(),
            )
        } == 0
        {
            Err(last_error("assign-process-containment"))
        } else {
            Ok(())
        }
    }

    pub(crate) fn contains(
        &self,
        process: &ProcessReference,
    ) -> Result<bool, ProcessContainmentError> {
        let handle = self.handle("query-process-containment-membership")?;
        process
            .is_member_of(handle.as_handle())
            .map_err(|error| io_error("query-process-containment-membership", error))
    }

    pub(crate) fn process_ids(&self) -> Result<Vec<u32>, ProcessContainmentError> {
        let handle = self.handle("query-process-containment-members")?;
        let offset = std::mem::offset_of!(JOBOBJECT_BASIC_PROCESS_ID_LIST, ProcessIdList);
        let word = std::mem::size_of::<usize>();
        let header_words = offset.div_ceil(word);
        let mut capacity = 16usize;
        loop {
            let mut storage = vec![0usize; header_words + capacity];
            let info = storage
                .as_mut_ptr()
                .cast::<JOBOBJECT_BASIC_PROCESS_ID_LIST>();
            let bytes = storage
                .len()
                .checked_mul(word)
                .and_then(|size| u32::try_from(size).ok())
                .ok_or_else(|| {
                    ProcessContainmentError::new(
                        ProcessContainmentErrorKind::InvalidInput,
                        "query-process-containment-members",
                        None,
                        "member query buffer exceeds u32",
                    )
                })?;
            let ok = unsafe {
                QueryInformationJobObject(
                    handle.as_raw_handle(),
                    JobObjectBasicProcessIdList,
                    info.cast(),
                    bytes,
                    std::ptr::null_mut(),
                )
            };
            if ok != 0 {
                let count = unsafe { (*info).NumberOfProcessIdsInList as usize };
                let ids = unsafe {
                    std::slice::from_raw_parts(info.cast::<u8>().add(offset).cast::<usize>(), count)
                };
                return ids
                    .iter()
                    .map(|id| {
                        u32::try_from(*id).map_err(|_| {
                            ProcessContainmentError::new(
                                ProcessContainmentErrorKind::NativeFailure,
                                "query-process-containment-members",
                                None,
                                format!("native process id {id} exceeds u32"),
                            )
                        })
                    })
                    .collect();
            }
            let code = unsafe { GetLastError() };
            if code != ERROR_MORE_DATA {
                return Err(native_error("query-process-containment-members", code));
            }
            let assigned = unsafe { (*info).NumberOfAssignedProcesses as usize };
            capacity = assigned.max(capacity.saturating_mul(2));
        }
    }

    pub(crate) fn terminate(&self, exit_code: u32) -> Result<(), ProcessContainmentError> {
        if unsafe {
            TerminateJobObject(
                self.handle("terminate-process-containment")?
                    .as_raw_handle(),
                exit_code,
            )
        } == 0
        {
            Err(last_error("terminate-process-containment"))
        } else {
            Ok(())
        }
    }

    pub(crate) fn close(&mut self) {
        drop(self.handle.take());
    }

    fn configure(&self, options: ProcessContainmentOptions) -> Result<(), ProcessContainmentError> {
        let handle = self.handle("configure-process-containment")?;
        let mut basic: JOBOBJECT_BASIC_LIMIT_INFORMATION = unsafe { std::mem::zeroed() };
        if options.terminate_on_last_close {
            basic.LimitFlags |= JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        }
        if options.allow_breakaway {
            basic.LimitFlags |= JOB_OBJECT_LIMIT_BREAKAWAY_OK;
        }
        if let Some(limit) = options.limits.active_processes {
            basic.LimitFlags |= JOB_OBJECT_LIMIT_ACTIVE_PROCESS;
            basic.ActiveProcessLimit = limit;
        }
        let mut extended: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { std::mem::zeroed() };
        extended.BasicLimitInformation = basic;
        if let Some(limit) = options.limits.memory_bytes {
            extended.BasicLimitInformation.LimitFlags |= JOB_OBJECT_LIMIT_JOB_MEMORY;
            extended.JobMemoryLimit = usize::try_from(limit).map_err(|_| {
                ProcessContainmentError::new(
                    ProcessContainmentErrorKind::InvalidInput,
                    "configure-process-containment",
                    None,
                    "memory limit exceeds native address size",
                )
            })?;
        }
        if unsafe {
            SetInformationJobObject(
                handle.as_raw_handle(),
                JobObjectExtendedLimitInformation,
                (&raw const extended).cast(),
                std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            )
        } == 0
        {
            return Err(last_error("configure-process-containment-limits"));
        }
        if let Some(rate) = options.limits.cpu_rate_hundredths {
            let mut cpu: JOBOBJECT_CPU_RATE_CONTROL_INFORMATION = unsafe { std::mem::zeroed() };
            cpu.ControlFlags =
                JOB_OBJECT_CPU_RATE_CONTROL_ENABLE | JOB_OBJECT_CPU_RATE_CONTROL_HARD_CAP;
            cpu.Anonymous.CpuRate = rate;
            if unsafe {
                SetInformationJobObject(
                    handle.as_raw_handle(),
                    JobObjectCpuRateControlInformation,
                    (&raw const cpu).cast(),
                    std::mem::size_of::<JOBOBJECT_CPU_RATE_CONTROL_INFORMATION>() as u32,
                )
            } == 0
            {
                return Err(last_error("configure-process-containment-cpu-rate"));
            }
        }
        Ok(())
    }

    fn handle(&self, operation: &'static str) -> Result<&OwnedHandle, ProcessContainmentError> {
        self.handle.as_ref().ok_or_else(|| {
            ProcessContainmentError::new(
                ProcessContainmentErrorKind::Closed,
                operation,
                None,
                "containment owner is closed",
            )
        })
    }
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(Some(0)).collect()
}

fn last_error(operation: &'static str) -> ProcessContainmentError {
    native_error(operation, unsafe { GetLastError() })
}

fn native_error(operation: &'static str, code: u32) -> ProcessContainmentError {
    ProcessContainmentError::new(
        ProcessContainmentErrorKind::NativeFailure,
        operation,
        Some(code),
        format!("Win32 error {code}"),
    )
}

fn io_error(operation: &'static str, error: std::io::Error) -> ProcessContainmentError {
    ProcessContainmentError::new(
        ProcessContainmentErrorKind::NativeFailure,
        operation,
        error.raw_os_error().map(|code| code as u32),
        error.to_string(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{process::Command, time::Duration};

    fn unique_name() -> String {
        format!(
            r"Local\agenterm-platform-containment-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system time")
                .as_nanos()
        )
    }

    #[test]
    fn named_containment_is_exclusive_reopenable_and_controls_exact_process() {
        let name = unique_name();
        let containment = crate::process_containment::ProcessContainment::create(
            Some(&name),
            crate::process_containment::ProcessContainmentOptions {
                terminate_on_last_close: true,
                ..crate::process_containment::ProcessContainmentOptions::default()
            },
        )
        .expect("create containment");
        assert_eq!(
            crate::process_containment::ProcessContainment::create(
                Some(&name),
                crate::process_containment::ProcessContainmentOptions::default()
            )
            .err()
            .expect("duplicate containment")
            .kind(),
            ProcessContainmentErrorKind::AlreadyExists
        );
        let reopened = crate::process_containment::ProcessContainment::open(&name)
            .expect("reopen containment");
        let mut child = Command::new("cmd.exe")
            .args(["/d", "/c", "ping -n 30 127.0.0.1 >nul"])
            .spawn()
            .expect("spawn containment child");
        let process = ProcessReference::duplicate_from(child.as_handle())
            .expect("retain exact containment child");
        containment.assign(&process).expect("assign child");
        assert!(reopened.contains(&process).expect("query membership"));
        assert!(
            reopened
                .process_ids()
                .expect("query members")
                .contains(&child.id())
        );
        reopened.terminate(37).expect("terminate containment");
        assert_eq!(
            process
                .wait_for_exit(Some(Duration::from_secs(5)))
                .expect("wait contained child"),
            crate::process_reference::ProcessWait::Exited
        );
        assert!(!child.wait().expect("reap contained child").success());
    }

    #[test]
    fn applies_native_limits_and_closed_owner_is_typed() {
        let mut containment = ProcessContainment::create(
            None,
            ProcessContainmentOptions {
                terminate_on_last_close: true,
                allow_breakaway: false,
                limits: crate::process_containment::ProcessContainmentLimits {
                    memory_bytes: Some(128 * 1024 * 1024),
                    cpu_rate_hundredths: Some(5_000),
                    active_processes: Some(10),
                },
            },
        )
        .expect("create limited containment");
        let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { std::mem::zeroed() };
        assert_ne!(
            unsafe {
                QueryInformationJobObject(
                    containment
                        .handle("test-query-limits")
                        .unwrap()
                        .as_raw_handle(),
                    JobObjectExtendedLimitInformation,
                    (&raw mut info).cast(),
                    std::mem::size_of_val(&info) as u32,
                    std::ptr::null_mut(),
                )
            },
            0
        );
        assert_eq!(info.JobMemoryLimit, 128 * 1024 * 1024);
        assert_eq!(info.BasicLimitInformation.ActiveProcessLimit, 10);
        assert_ne!(
            info.BasicLimitInformation.LimitFlags & JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
            0
        );
        assert_eq!(
            info.BasicLimitInformation.LimitFlags & JOB_OBJECT_LIMIT_BREAKAWAY_OK,
            0
        );
        let mut cpu: JOBOBJECT_CPU_RATE_CONTROL_INFORMATION = unsafe { std::mem::zeroed() };
        assert_ne!(
            unsafe {
                QueryInformationJobObject(
                    containment
                        .handle("test-query-cpu-rate")
                        .unwrap()
                        .as_raw_handle(),
                    JobObjectCpuRateControlInformation,
                    (&raw mut cpu).cast(),
                    std::mem::size_of_val(&cpu) as u32,
                    std::ptr::null_mut(),
                )
            },
            0
        );
        assert_eq!(unsafe { cpu.Anonymous.CpuRate }, 5_000);
        containment.close();
        assert_eq!(
            containment
                .process_ids()
                .expect_err("closed containment")
                .kind(),
            ProcessContainmentErrorKind::Closed
        );
    }
}
