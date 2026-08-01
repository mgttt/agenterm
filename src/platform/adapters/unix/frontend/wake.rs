use std::sync::{Arc, Mutex, OnceLock};

use crate::wake_signal::WakeSignal;

/// User-event payload used by the selected Unix window host.
pub type UnixWake = ();

type GuiWake = Arc<dyn Fn() + Send + Sync>;

static UNIX_WAKE: OnceLock<Mutex<Option<GuiWake>>> = OnceLock::new();

fn unix_wake() -> &'static Mutex<Option<GuiWake>> {
    UNIX_WAKE.get_or_init(|| Mutex::new(None))
}

/// Registers a neutral callback for the selected frontend event-loop waker.
pub fn install_unix_wake(wake: impl Fn() + Send + Sync + 'static) {
    *unix_wake().lock().expect("unix wake mailbox lock poisoned") = Some(Arc::new(wake));
}

pub(crate) fn request_gui_wake(_wake_window: isize, wake_signal: &WakeSignal) {
    if !wake_signal.request() {
        return;
    }
    let guard = unix_wake().lock().expect("unix wake mailbox lock poisoned");
    if let Some(wake) = guard.as_ref() {
        wake();
    }
}
