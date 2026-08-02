use std::sync::{Arc, Mutex, OnceLock};

use crate::wake_signal::WakeSignal;
use crate::frontend::GuiWakeResult;

/// User-event payload used by the selected Unix window host.
pub type UnixWake = ();

type GuiWake = Arc<dyn Fn() + Send + Sync>;

static UNIX_WAKE: OnceLock<Mutex<Option<GuiWake>>> = OnceLock::new();

fn unix_wake() -> &'static Mutex<Option<GuiWake>> {
    UNIX_WAKE.get_or_init(|| Mutex::new(None))
}

/// Registers a neutral callback for the selected frontend event-loop waker.
pub fn install_unix_wake(wake: impl Fn() + Send + Sync + 'static) {
    if let Ok(mut installed) = unix_wake().lock() {
        *installed = Some(Arc::new(wake));
    }
}

pub(crate) fn request_gui_wake(_wake_window: isize, wake_signal: &WakeSignal) -> GuiWakeResult {
    if !wake_signal.request() {
        return GuiWakeResult::Throttled;
    }
    let guard = match unix_wake().lock() {
        Ok(guard) => guard,
        Err(error) => {
            return GuiWakeResult::Failed(format!("unix wake mailbox lock poisoned: {error}"));
        }
    };
    if let Some(wake) = guard.as_ref() {
        wake();
        return GuiWakeResult::Woke;
    }
    GuiWakeResult::NoTarget
}
