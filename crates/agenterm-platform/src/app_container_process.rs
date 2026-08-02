//! AppContainer process creation that cannot execute before caller containment.

use std::{fmt, path::Path};

use crate::{
    process_conventions::{
        InvalidEnvironmentEntryPolicy, WindowsEnvironmentBlockError, windows_environment_block,
    },
    process_reference::ProcessReference,
};

/// One validated capability SID and the native access-check attributes to apply.
#[derive(Clone, Copy, Debug)]
pub struct AppContainerProcessCapability<'a> {
    sid: &'a [u8],
    attributes: u32,
}

impl<'a> AppContainerProcessCapability<'a> {
    #[must_use]
    pub const fn new(sid: &'a [u8], attributes: u32) -> Self {
        Self { sid, attributes }
    }

    #[must_use]
    pub const fn sid(self) -> &'a [u8] {
        self.sid
    }

    #[must_use]
    pub const fn attributes(self) -> u32 {
        self.attributes
    }
}

/// Mechanism inputs for one explicitly suspended AppContainer process.
///
/// The embedding product owns command construction, environment selection,
/// capability policy, containment assignment, and lifecycle hooks.
#[derive(Clone, Copy, Debug)]
pub struct AppContainerProcessOptions<'a> {
    pub app_container_sid: &'a [u8],
    pub capabilities: &'a [AppContainerProcessCapability<'a>],
    pub command_line: &'a str,
    pub working_directory: &'a Path,
    pub environment: &'a [(String, String)],
    pub invalid_environment_entries: InvalidEnvironmentEntryPolicy,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum AppContainerProcessErrorKind {
    InvalidInput,
    Unsupported,
    NativeFailure,
}

impl AppContainerProcessErrorKind {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::InvalidInput => "invalid-input",
            Self::Unsupported => "unsupported",
            Self::NativeFailure => "native-failure",
        }
    }
}

#[derive(Debug)]
pub struct AppContainerProcessError {
    kind: AppContainerProcessErrorKind,
    operation: &'static str,
    native_code: Option<u32>,
    detail: String,
}

impl AppContainerProcessError {
    #[must_use]
    pub const fn kind(&self) -> AppContainerProcessErrorKind {
        self.kind
    }

    #[must_use]
    pub const fn operation(&self) -> &'static str {
        self.operation
    }

    #[must_use]
    pub const fn native_code(&self) -> Option<u32> {
        self.native_code
    }

    pub(crate) fn invalid(operation: &'static str, detail: impl Into<String>) -> Self {
        Self {
            kind: AppContainerProcessErrorKind::InvalidInput,
            operation,
            native_code: None,
            detail: detail.into(),
        }
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    pub(crate) fn unsupported(operation: &'static str) -> Self {
        Self {
            kind: AppContainerProcessErrorKind::Unsupported,
            operation,
            native_code: None,
            detail: "AppContainer process creation is only available on Windows".to_owned(),
        }
    }

    #[cfg(windows)]
    pub(crate) fn native(operation: &'static str, code: u32) -> Self {
        Self {
            kind: AppContainerProcessErrorKind::NativeFailure,
            operation,
            native_code: Some(code),
            detail: format!("native error {code}"),
        }
    }

    #[cfg(windows)]
    pub(crate) fn io(operation: &'static str, error: std::io::Error) -> Self {
        Self {
            kind: AppContainerProcessErrorKind::NativeFailure,
            operation,
            native_code: error.raw_os_error().map(|code| code as u32),
            detail: error.to_string(),
        }
    }
}

impl fmt::Display for AppContainerProcessError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{} failed: {}", self.operation, self.detail)
    }
}

impl std::error::Error for AppContainerProcessError {}

#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) struct PreparedAppContainerProcess<'a> {
    pub(crate) app_container_sid: &'a [u8],
    pub(crate) capabilities: &'a [AppContainerProcessCapability<'a>],
    pub(crate) command_line: &'a str,
    pub(crate) working_directory: &'a Path,
    pub(crate) environment: Vec<u16>,
}

/// A process whose primary thread has never executed caller code.
///
/// Dropping this value before a successful [`Self::resume`] terminates and
/// waits for the exact process object. This makes error paths fail closed.
pub struct SuspendedAppContainerProcess {
    process: Option<ProcessReference>,
    resume: crate::selected::app_container_process::ResumeToken,
}

impl SuspendedAppContainerProcess {
    #[must_use]
    pub fn process(&self) -> &ProcessReference {
        self.process
            .as_ref()
            .expect("a public suspended process always owns its process reference")
    }

    /// Resumes the primary thread and transfers exact process ownership.
    ///
    /// A resume failure still triggers the suspended-process Drop guard.
    pub fn resume(mut self) -> Result<ProcessReference, AppContainerProcessError> {
        crate::selected::app_container_process::resume(&mut self.resume)?;
        Ok(self
            .process
            .take()
            .expect("a public suspended process always owns its process reference"))
    }
}

impl Drop for SuspendedAppContainerProcess {
    fn drop(&mut self) {
        if let Some(process) = self.process.as_ref() {
            crate::selected::app_container_process::abort_suspended(process, &mut self.resume);
        }
    }
}

/// Creates a suspended AppContainer process without inherited native objects.
pub fn spawn_suspended(
    options: AppContainerProcessOptions<'_>,
) -> Result<SuspendedAppContainerProcess, AppContainerProcessError> {
    let prepared = prepare(options)?;
    let (process, resume) = crate::selected::app_container_process::spawn(&prepared)?;
    Ok(SuspendedAppContainerProcess {
        process: Some(process),
        resume,
    })
}

/// Windows extension for an exact inherited HANDLE allowlist.
#[cfg(windows)]
pub fn spawn_suspended_with_handles(
    options: AppContainerProcessOptions<'_>,
    handles: &[std::os::windows::io::BorrowedHandle<'_>],
) -> Result<SuspendedAppContainerProcess, AppContainerProcessError> {
    let prepared = prepare(options)?;
    let (process, resume) =
        crate::selected::app_container_process::spawn_with_handles(&prepared, handles)?;
    Ok(SuspendedAppContainerProcess {
        process: Some(process),
        resume,
    })
}

fn prepare(
    options: AppContainerProcessOptions<'_>,
) -> Result<PreparedAppContainerProcess<'_>, AppContainerProcessError> {
    const OPERATION: &str = "prepare-app-container-process";
    if options.app_container_sid.is_empty() {
        return Err(AppContainerProcessError::invalid(
            OPERATION,
            "AppContainer SID must not be empty",
        ));
    }
    if options.command_line.is_empty() {
        return Err(AppContainerProcessError::invalid(
            OPERATION,
            "command line must not be empty",
        ));
    }
    if options.command_line.contains('\0') {
        return Err(AppContainerProcessError::invalid(
            OPERATION,
            "command line contains NUL",
        ));
    }
    if options.working_directory.as_os_str().is_empty() {
        return Err(AppContainerProcessError::invalid(
            OPERATION,
            "working directory must not be empty",
        ));
    }
    let environment =
        windows_environment_block(options.environment, options.invalid_environment_entries)
            .map_err(environment_error)?;
    Ok(PreparedAppContainerProcess {
        app_container_sid: options.app_container_sid,
        capabilities: options.capabilities,
        command_line: options.command_line,
        working_directory: options.working_directory,
        environment,
    })
}

fn environment_error(error: WindowsEnvironmentBlockError) -> AppContainerProcessError {
    AppContainerProcessError::invalid("prepare-app-container-environment", error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn options<'a>(sid: &'a [u8], command: &'a str) -> AppContainerProcessOptions<'a> {
        AppContainerProcessOptions {
            app_container_sid: sid,
            capabilities: &[],
            command_line: command,
            working_directory: Path::new("."),
            environment: &[],
            invalid_environment_entries: InvalidEnvironmentEntryPolicy::Reject,
        }
    }

    #[test]
    fn invalid_inputs_fail_before_native_process_creation() {
        assert_eq!(
            spawn_suspended(options(&[], "cmd.exe"))
                .err()
                .expect("empty SID")
                .kind(),
            AppContainerProcessErrorKind::InvalidInput
        );
        assert_eq!(
            spawn_suspended(options(&[1], "bad\0command"))
                .err()
                .expect("NUL command")
                .kind(),
            AppContainerProcessErrorKind::InvalidInput
        );
    }

    #[cfg(not(windows))]
    #[test]
    fn non_windows_hosts_report_typed_unsupported() {
        assert_eq!(
            spawn_suspended(options(&[1], "cmd.exe"))
                .err()
                .expect("unsupported host")
                .kind(),
            AppContainerProcessErrorKind::Unsupported
        );
    }
}

#[cfg(all(test, windows))]
mod real_windows_tests {
    use super::*;
    use std::os::windows::io::{AsHandle as _, AsRawHandle as _, FromRawHandle as _};

    use crate::adapters::windows::app_container;

    struct ProfileCleanup(String);

    impl Drop for ProfileCleanup {
        fn drop(&mut self) {
            let _ = app_container::delete_profile(&self.0);
        }
    }

    fn unique_name(label: &str) -> String {
        format!(
            "agenterm.platform.process.{}.{}.{}",
            std::process::id(),
            label,
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system clock")
                .as_nanos()
        )
    }

    fn fixture() -> (
        String,
        crate::adapters::windows::app_container::OwnedAppContainerSid,
    ) {
        let name = unique_name("suspended");
        let sid = app_container::create_profile(
            &name,
            &name,
            "agenterm-platform suspended process test",
            &[],
        )
        .expect("create AppContainer profile");
        (name, sid)
    }

    fn command_fixture() -> (String, std::path::PathBuf, Vec<(String, String)>) {
        let system_root = std::env::var("SystemRoot").expect("SystemRoot");
        let executable = std::path::Path::new(&system_root)
            .join("System32")
            .join("hostname.exe");
        assert!(executable.is_file(), "hostname.exe fixture is missing");
        let command = format!(r#""{}""#, executable.display());
        let environment = [
            "SystemRoot",
            "PATH",
            "COMSPEC",
            "PATHEXT",
            "TEMP",
            "TMP",
            "USERPROFILE",
            "APPDATA",
            "LOCALAPPDATA",
        ]
        .into_iter()
        .filter_map(|key| std::env::var(key).ok().map(|value| (key.to_owned(), value)))
        .collect();
        (
            command,
            executable.parent().expect("hostname parent").into(),
            environment,
        )
    }

    #[test]
    fn suspended_process_resumes_and_preserves_exact_exit_code() {
        let (name, sid) = fixture();
        let _cleanup = ProfileCleanup(name);
        let (command, directory, environment) = command_fixture();
        let suspended = spawn_suspended(AppContainerProcessOptions {
            app_container_sid: sid.as_bytes(),
            capabilities: &[],
            command_line: &command,
            working_directory: &directory,
            environment: &environment,
            invalid_environment_entries: InvalidEnvironmentEntryPolicy::Reject,
        })
        .expect("spawn suspended AppContainer process");
        assert!(suspended.process().is_alive().expect("suspended liveness"));
        let process = suspended.resume().expect("resume AppContainer process");
        assert_eq!(
            crate::process_reference::wait_handle(process.as_handle(), None)
                .expect("wait resumed process"),
            crate::process_reference::ProcessWait::Exited
        );
        assert_eq!(
            crate::process_reference::exit_code_handle(process.as_handle())
                .expect("read resumed exit code"),
            0
        );
    }

    #[test]
    fn dropping_unresumed_process_terminates_and_waits_for_the_exact_object() {
        let (name, sid) = fixture();
        let _cleanup = ProfileCleanup(name);
        let (command, directory, environment) = command_fixture();
        let suspended = spawn_suspended(AppContainerProcessOptions {
            app_container_sid: sid.as_bytes(),
            capabilities: &[],
            command_line: &command,
            working_directory: &directory,
            environment: &environment,
            invalid_environment_entries: InvalidEnvironmentEntryPolicy::Reject,
        })
        .expect("spawn process for drop guard");
        let retained = ProcessReference::duplicate_from(suspended.process().as_handle())
            .expect("retain exact process before guard drop");
        drop(suspended);
        assert_eq!(
            crate::process_reference::wait_handle(
                retained.as_handle(),
                Some(std::time::Duration::from_secs(2))
            )
            .expect("wait dropped suspended process"),
            crate::process_reference::ProcessWait::Exited
        );
    }

    #[test]
    fn explicit_handle_allowlist_restores_source_inheritance_flag() {
        use windows_sys::Win32::{
            Foundation::{GetHandleInformation, HANDLE_FLAG_INHERIT},
            System::Threading::CreateEventW,
        };

        let (name, sid) = fixture();
        let _cleanup = ProfileCleanup(name);
        let (command, directory, environment) = command_fixture();
        let event = unsafe {
            std::os::windows::io::OwnedHandle::from_raw_handle(CreateEventW(
                std::ptr::null(),
                1,
                0,
                std::ptr::null(),
            ))
        };
        assert!(!event.as_raw_handle().is_null());
        let suspended = spawn_suspended_with_handles(
            AppContainerProcessOptions {
                app_container_sid: sid.as_bytes(),
                capabilities: &[],
                command_line: &command,
                working_directory: &directory,
                environment: &environment,
                invalid_environment_entries: InvalidEnvironmentEntryPolicy::Reject,
            },
            &[event.as_handle()],
        )
        .expect("spawn with explicit inherited event");
        let mut flags = 0;
        assert_ne!(
            unsafe { GetHandleInformation(event.as_raw_handle(), &raw mut flags) },
            0
        );
        assert_eq!(flags & HANDLE_FLAG_INHERIT, 0);
        let process = suspended.resume().expect("resume inherited-handle probe");
        assert_eq!(
            crate::process_reference::wait_handle(process.as_handle(), None)
                .expect("wait inherited-handle probe"),
            crate::process_reference::ProcessWait::Exited
        );
    }
}
