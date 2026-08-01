use std::process::{Child, Command, ExitStatus};

use crate::contract::process_spawn::ProcessExit;

pub(crate) fn classify_exit_status(status: &ExitStatus) -> ProcessExit {
    status
        .code()
        .map(ProcessExit::Code)
        .unwrap_or(ProcessExit::Unavailable)
}

pub(crate) fn configure_detached_command(command: &mut Command) -> Result<(), String> {
    use std::os::windows::process::CommandExt as _;
    use windows_sys::Win32::System::Threading::{CREATE_BREAKAWAY_FROM_JOB, CREATE_NO_WINDOW};
    command.creation_flags(CREATE_BREAKAWAY_FROM_JOB | CREATE_NO_WINDOW);
    Ok(())
}

pub(crate) fn is_breakaway_denied(error: &std::io::Error) -> bool {
    error.raw_os_error() == Some(windows_sys::Win32::Foundation::ERROR_ACCESS_DENIED as i32)
}

pub(crate) fn configure_caller_job_fallback(command: &mut Command) -> Result<(), String> {
    use std::os::windows::process::CommandExt as _;
    use windows_sys::Win32::System::Threading::CREATE_NO_WINDOW;
    command.creation_flags(CREATE_NO_WINDOW);
    Ok(())
}

pub(crate) fn spawn(command: &mut Command) -> std::io::Result<Child> {
    let _guard = StandardHandleInheritanceGuard::new()?;
    command.spawn()
}

#[allow(dead_code)]
pub(crate) fn with_inheritable_handles<T>(
    handles: &[std::os::windows::io::BorrowedHandle<'_>],
    operation: impl FnOnce() -> T,
) -> std::io::Result<T> {
    use std::{collections::HashSet, os::windows::io::AsRawHandle as _};
    use windows_sys::Win32::Foundation::{
        GetHandleInformation, HANDLE, HANDLE_FLAG_INHERIT, SetHandleInformation,
    };

    if handles.is_empty() {
        return Ok(operation());
    }

    let mut guard = ExplicitHandleInheritanceGuard {
        _lock: Some(crate::process_spawn::lock_handle_inheritance()),
        changed: Vec::new(),
    };
    let mut seen = HashSet::with_capacity(handles.len());
    for handle in handles {
        let handle = handle.as_raw_handle() as HANDLE;
        if !seen.insert(handle as usize) {
            continue;
        }
        let mut flags = 0;
        if unsafe { GetHandleInformation(handle, &raw mut flags) } == 0 {
            return Err(std::io::Error::last_os_error());
        }
        if flags & HANDLE_FLAG_INHERIT == 0 {
            if unsafe { SetHandleInformation(handle, HANDLE_FLAG_INHERIT, HANDLE_FLAG_INHERIT) }
                == 0
            {
                return Err(std::io::Error::last_os_error());
            }
            guard.changed.push((handle, flags));
        }
    }

    let output = operation();
    guard.restore()?;
    Ok(output)
}

#[allow(dead_code)]
struct ExplicitHandleInheritanceGuard {
    _lock: Option<crate::process_spawn::HandleInheritanceLock>,
    changed: Vec<(windows_sys::Win32::Foundation::HANDLE, u32)>,
}

#[allow(dead_code)]
impl ExplicitHandleInheritanceGuard {
    fn restore(&mut self) -> std::io::Result<()> {
        use windows_sys::Win32::Foundation::{HANDLE_FLAG_INHERIT, SetHandleInformation};

        let mut first_error = None;
        for (handle, flags) in self.changed.drain(..) {
            if unsafe {
                SetHandleInformation(handle, HANDLE_FLAG_INHERIT, flags & HANDLE_FLAG_INHERIT)
            } == 0
                && first_error.is_none()
            {
                first_error = Some(std::io::Error::last_os_error());
            }
        }
        self._lock.take();
        match first_error {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }
}

impl Drop for ExplicitHandleInheritanceGuard {
    fn drop(&mut self) {
        let _ = self.restore();
    }
}

/// `CreateProcessW` can inherit the parent's ambient standard handles in
/// addition to explicitly configured child stdio. Clear those flags only for
/// the serialized spawn window and restore them even when process creation
/// fails.
struct StandardHandleInheritanceGuard {
    _lock: crate::process_spawn::HandleInheritanceLock,
    handles: Vec<(windows_sys::Win32::Foundation::HANDLE, u32)>,
}

impl StandardHandleInheritanceGuard {
    fn new() -> std::io::Result<Self> {
        use windows_sys::Win32::Foundation::{
            ERROR_INVALID_HANDLE, GetHandleInformation, HANDLE_FLAG_INHERIT, INVALID_HANDLE_VALUE,
            SetHandleInformation,
        };
        use windows_sys::Win32::System::Console::{
            GetStdHandle, STD_ERROR_HANDLE, STD_INPUT_HANDLE, STD_OUTPUT_HANDLE,
        };

        let mut guard = Self {
            _lock: crate::process_spawn::lock_handle_inheritance(),
            handles: Vec::new(),
        };
        for standard_handle in [STD_INPUT_HANDLE, STD_OUTPUT_HANDLE, STD_ERROR_HANDLE] {
            let handle = unsafe { GetStdHandle(standard_handle) };
            if handle.is_null() || handle == INVALID_HANDLE_VALUE {
                continue;
            }
            let mut flags = 0_u32;
            if unsafe { GetHandleInformation(handle, &mut flags) } == 0 {
                let error = std::io::Error::last_os_error();
                if error.raw_os_error() == Some(ERROR_INVALID_HANDLE as i32) {
                    continue;
                }
                return Err(error);
            }
            if flags & HANDLE_FLAG_INHERIT != 0 {
                if unsafe { SetHandleInformation(handle, HANDLE_FLAG_INHERIT, 0) } == 0 {
                    return Err(std::io::Error::last_os_error());
                }
                guard.handles.push((handle, flags));
            }
        }
        Ok(guard)
    }
}

impl Drop for StandardHandleInheritanceGuard {
    fn drop(&mut self) {
        use windows_sys::Win32::Foundation::{HANDLE_FLAG_INHERIT, SetHandleInformation};
        for (handle, flags) in self.handles.drain(..) {
            let _ = unsafe {
                SetHandleInformation(handle, HANDLE_FLAG_INHERIT, flags & HANDLE_FLAG_INHERIT)
            };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_handle_scope_restores_flags_and_deduplicates() {
        use std::os::windows::io::{AsHandle as _, AsRawHandle as _};
        use windows_sys::Win32::Foundation::{GetHandleInformation, HANDLE_FLAG_INHERIT};

        let path = std::env::temp_dir().join(format!(
            "agenterm-platform-inherit-dedup-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);
        let file = std::fs::File::create(&path).expect("create inheritance fixture");
        let flags = || {
            let mut flags = 0;
            assert_ne!(
                unsafe { GetHandleInformation(file.as_raw_handle(), &raw mut flags) },
                0
            );
            flags
        };
        assert_eq!(flags() & HANDLE_FLAG_INHERIT, 0);
        let borrowed = file.as_handle();
        with_inheritable_handles(&[borrowed, borrowed], || {
            assert_ne!(flags() & HANDLE_FLAG_INHERIT, 0);
        })
        .expect("explicit inheritance transaction");
        assert_eq!(flags() & HANDLE_FLAG_INHERIT, 0);
        drop(file);
        std::fs::remove_file(path).expect("remove inheritance fixture");
    }

    #[test]
    fn explicit_handle_scope_restores_flags_during_unwind() {
        use std::os::windows::io::{AsHandle as _, AsRawHandle as _};
        use windows_sys::Win32::Foundation::{GetHandleInformation, HANDLE_FLAG_INHERIT};

        let path = std::env::temp_dir().join(format!(
            "agenterm-platform-inherit-unwind-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);
        let file = std::fs::File::create(&path).expect("create inheritance fixture");
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _ = with_inheritable_handles(&[file.as_handle()], || panic!("probe"));
        }));
        assert!(result.is_err());
        let mut flags = 0;
        assert_ne!(
            unsafe { GetHandleInformation(file.as_raw_handle(), &raw mut flags) },
            0
        );
        assert_eq!(flags & HANDLE_FLAG_INHERIT, 0);
        drop(file);
        std::fs::remove_file(path).expect("remove inheritance fixture");
    }

    #[test]
    fn only_access_denied_triggers_caller_job_fallback() {
        assert!(is_breakaway_denied(&std::io::Error::from_raw_os_error(5)));
        assert!(!is_breakaway_denied(&std::io::Error::from_raw_os_error(2)));
    }
}
