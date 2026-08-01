//! POSIX SIGINT adapter using an async-signal-safe self-pipe.

use std::sync::{
    OnceLock,
    atomic::{AtomicBool, AtomicI32, Ordering},
};

use crate::contract::console_interrupt::ConsoleInterruptError;

static ACTIVE: AtomicBool = AtomicBool::new(false);
static NOTIFY_FD: AtomicI32 = AtomicI32::new(-1);
static SELF_PIPE: OnceLock<Result<SelfPipe, ConsoleInterruptError>> = OnceLock::new();

struct SelfPipe {
    read_descriptor: libc::c_int,
    write_descriptor: libc::c_int,
}

extern "C" fn notify_sigint(signal: libc::c_int) {
    if signal != libc::SIGINT {
        return;
    }
    let descriptor = NOTIFY_FD.load(Ordering::Relaxed);
    if descriptor >= 0 {
        let byte = 1_u8;
        // SAFETY: write(2) is async-signal-safe. The descriptor belongs to the
        // process-lifetime self-pipe and is never closed or reused.
        let _ = unsafe { libc::write(descriptor, (&byte as *const u8).cast(), 1) };
    }
}

fn native_failure(code: &'static str, error: std::io::Error) -> ConsoleInterruptError {
    ConsoleInterruptError::failed(code, error.raw_os_error().map(i64::from), error.to_string())
}

fn claim() -> Result<(), ConsoleInterruptError> {
    ACTIVE
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .map(|_| ())
        .map_err(|_| {
            ConsoleInterruptError::failed(
                "already-installed",
                None,
                "a console interrupt observer or ignore guard is already active",
            )
        })
}

fn release() {
    ACTIVE.store(false, Ordering::Release);
}

fn set_descriptor_flags(descriptor: libc::c_int) -> std::io::Result<()> {
    let status_flags = unsafe { libc::fcntl(descriptor, libc::F_GETFL) };
    if status_flags < 0
        || unsafe { libc::fcntl(descriptor, libc::F_SETFL, status_flags | libc::O_NONBLOCK) } < 0
    {
        return Err(std::io::Error::last_os_error());
    }
    let descriptor_flags = unsafe { libc::fcntl(descriptor, libc::F_GETFD) };
    if descriptor_flags < 0
        || unsafe {
            libc::fcntl(
                descriptor,
                libc::F_SETFD,
                descriptor_flags | libc::FD_CLOEXEC,
            )
        } < 0
    {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}

fn sigint_action(handler: libc::sighandler_t) -> std::io::Result<libc::sigaction> {
    let mut action = unsafe { std::mem::zeroed::<libc::sigaction>() };
    action.sa_sigaction = handler;
    if unsafe { libc::sigemptyset(&mut action.sa_mask) } != 0 {
        return Err(std::io::Error::last_os_error());
    }
    action.sa_flags = 0;
    let mut previous = unsafe { std::mem::zeroed::<libc::sigaction>() };
    if unsafe { libc::sigaction(libc::SIGINT, &action, &mut previous) } != 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(previous)
}

fn restore(previous: &libc::sigaction) {
    let _ = unsafe { libc::sigaction(libc::SIGINT, previous, std::ptr::null_mut()) };
}

fn close(descriptor: libc::c_int) {
    if descriptor >= 0 {
        let _ = unsafe { libc::close(descriptor) };
    }
}

fn create_self_pipe() -> Result<SelfPipe, ConsoleInterruptError> {
    let mut descriptors = [-1; 2];
    if unsafe { libc::pipe(descriptors.as_mut_ptr()) } != 0 {
        return Err(native_failure(
            "create-self-pipe",
            std::io::Error::last_os_error(),
        ));
    }
    if let Err(error) =
        set_descriptor_flags(descriptors[0]).and_then(|()| set_descriptor_flags(descriptors[1]))
    {
        close(descriptors[0]);
        close(descriptors[1]);
        return Err(native_failure("configure-self-pipe", error));
    }
    Ok(SelfPipe {
        read_descriptor: descriptors[0],
        write_descriptor: descriptors[1],
    })
}

fn self_pipe() -> Result<&'static SelfPipe, ConsoleInterruptError> {
    match SELF_PIPE.get_or_init(create_self_pipe) {
        Ok(pipe) => Ok(pipe),
        Err(error) => Err(error.clone()),
    }
}

fn drain_self_pipe(pipe: &SelfPipe) -> Result<bool, ConsoleInterruptError> {
    let mut observed = false;
    let mut buffer = [0_u8; 64];
    loop {
        let read = unsafe {
            libc::read(
                pipe.read_descriptor,
                buffer.as_mut_ptr().cast(),
                buffer.len(),
            )
        };
        if read > 0 {
            observed = true;
            continue;
        }
        if read == 0 {
            return Ok(observed);
        }
        let error = std::io::Error::last_os_error();
        if error.kind() == std::io::ErrorKind::Interrupted {
            continue;
        }
        if error.kind() == std::io::ErrorKind::WouldBlock {
            return Ok(observed);
        }
        return Err(native_failure("read-self-pipe", error));
    }
}

pub(crate) struct Observer {
    previous: libc::sigaction,
}

impl Observer {
    pub(crate) fn install() -> Result<Self, ConsoleInterruptError> {
        claim()?;
        let pipe = match self_pipe().and_then(|pipe| {
            drain_self_pipe(pipe)?;
            Ok(pipe)
        }) {
            Ok(pipe) => pipe,
            Err(error) => {
                release();
                return Err(error);
            }
        };
        NOTIFY_FD.store(pipe.write_descriptor, Ordering::Release);
        let previous = match sigint_action(notify_sigint as *const () as libc::sighandler_t) {
            Ok(previous) => previous,
            Err(error) => {
                NOTIFY_FD.store(-1, Ordering::Release);
                release();
                return Err(native_failure("install-sigaction", error));
            }
        };
        Ok(Self { previous })
    }

    pub(crate) fn take_pending(&self) -> Result<bool, ConsoleInterruptError> {
        drain_self_pipe(self_pipe()?)
    }
}

impl Drop for Observer {
    fn drop(&mut self) {
        restore(&self.previous);
        NOTIFY_FD.store(-1, Ordering::Release);
        release();
    }
}

pub(crate) struct IgnoreGuard {
    previous: libc::sigaction,
}

impl IgnoreGuard {
    pub(crate) fn install() -> Result<Self, ConsoleInterruptError> {
        claim()?;
        match sigint_action(libc::SIG_IGN) {
            Ok(previous) => Ok(Self { previous }),
            Err(error) => {
                release();
                Err(native_failure("ignore-sigaction", error))
            }
        }
    }
}

impl Drop for IgnoreGuard {
    fn drop(&mut self) {
        restore(&self.previous);
        release();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn self_pipe_is_initialized_once_and_kept_for_process_lifetime() {
        let first = self_pipe().unwrap();
        let second = self_pipe().unwrap();
        assert!(std::ptr::eq(first, second));
        assert!(first.read_descriptor >= 0);
        assert!(first.write_descriptor >= 0);
    }
}
