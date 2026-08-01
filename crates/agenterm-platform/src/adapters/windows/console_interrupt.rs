//! Windows Ctrl-C-only console handler.

use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};

use windows_sys::Win32::{
    Foundation::{FALSE, TRUE},
    System::Console::{CTRL_C_EVENT, SetConsoleCtrlHandler},
};

use crate::contract::console_interrupt::ConsoleInterruptError;

const MODE_NONE: u8 = 0;
const MODE_OBSERVE: u8 = 1;
const MODE_IGNORE: u8 = 2;

static MODE: AtomicU8 = AtomicU8::new(MODE_NONE);
static PENDING: AtomicBool = AtomicBool::new(false);

unsafe extern "system" fn console_handler(control_type: u32) -> i32 {
    if control_type != CTRL_C_EVENT {
        return FALSE;
    }
    match MODE.load(Ordering::Relaxed) {
        MODE_OBSERVE => {
            PENDING.store(true, Ordering::Release);
            TRUE
        }
        MODE_IGNORE => TRUE,
        _ => FALSE,
    }
}

fn install_mode(mode: u8) -> Result<(), ConsoleInterruptError> {
    MODE.compare_exchange(MODE_NONE, mode, Ordering::AcqRel, Ordering::Acquire)
        .map_err(|_| {
            ConsoleInterruptError::failed(
                "already-installed",
                None,
                "a console interrupt observer or ignore guard is already active",
            )
        })?;
    if unsafe { SetConsoleCtrlHandler(Some(console_handler), TRUE) } == 0 {
        MODE.store(MODE_NONE, Ordering::Release);
        let error = std::io::Error::last_os_error();
        return Err(ConsoleInterruptError::failed(
            "install-handler",
            error.raw_os_error().map(i64::from),
            error.to_string(),
        ));
    }
    Ok(())
}

fn uninstall() {
    // Removing this exact callback restores the previously existing handler
    // chain. Other console control events were never claimed by this handler.
    let _ = unsafe { SetConsoleCtrlHandler(Some(console_handler), FALSE) };
    MODE.store(MODE_NONE, Ordering::Release);
}

pub(crate) struct Observer;

impl Observer {
    pub(crate) fn install() -> Result<Self, ConsoleInterruptError> {
        PENDING.store(false, Ordering::Release);
        install_mode(MODE_OBSERVE)?;
        Ok(Self)
    }

    pub(crate) fn take_pending(&self) -> Result<bool, ConsoleInterruptError> {
        Ok(PENDING.swap(false, Ordering::AcqRel))
    }
}

impl Drop for Observer {
    fn drop(&mut self) {
        uninstall();
        PENDING.store(false, Ordering::Release);
    }
}

pub(crate) struct IgnoreGuard;

impl IgnoreGuard {
    pub(crate) fn install() -> Result<Self, ConsoleInterruptError> {
        install_mode(MODE_IGNORE)?;
        Ok(Self)
    }
}

impl Drop for IgnoreGuard {
    fn drop(&mut self) {
        uninstall();
    }
}
