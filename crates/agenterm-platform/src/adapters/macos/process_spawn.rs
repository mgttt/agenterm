use std::process::{Child, Command, ExitStatus};

use crate::contract::process_spawn::ProcessExit;

pub(crate) fn classify_exit_status(status: &ExitStatus) -> ProcessExit {
    use std::os::unix::process::ExitStatusExt as _;

    if let Some(code) = status.code() {
        return ProcessExit::Code(code);
    }
    status
        .signal()
        .and_then(|signal| u32::try_from(signal).ok())
        .map(ProcessExit::Signal)
        .unwrap_or(ProcessExit::Unavailable)
}

pub(crate) fn configure_detached_command(command: &mut Command) -> Result<(), String> {
    use std::os::unix::process::CommandExt;
    unsafe {
        command.pre_exec(|| {
            if libc::setsid() == -1 {
                Err(std::io::Error::last_os_error())
            } else {
                Ok(())
            }
        });
    }
    Ok(())
}

pub(crate) fn configure_owned_headless_command(_command: &mut Command) -> Result<(), String> {
    Ok(())
}

pub(crate) fn configure_breakaway_visible_command(command: &mut Command) -> Result<(), String> {
    use std::os::unix::process::CommandExt;
    unsafe {
        command.pre_exec(|| {
            if libc::setpgid(0, 0) == 0 {
                Ok(())
            } else {
                Err(std::io::Error::last_os_error())
            }
        });
    }
    Ok(())
}

pub(crate) fn is_breakaway_denied(_error: &std::io::Error) -> bool {
    false
}

pub(crate) fn configure_caller_job_fallback(_command: &mut Command) -> Result<(), String> {
    Err("caller-job fallback exists only on Windows".to_owned())
}

pub(crate) fn configure_visible_in_caller_job(_command: &mut Command) -> Result<(), String> {
    // Breakaway denial is Windows-only; leave the command unchanged.
    Ok(())
}

pub(crate) fn spawn(command: &mut Command) -> std::io::Result<Child> {
    command.spawn()
}
