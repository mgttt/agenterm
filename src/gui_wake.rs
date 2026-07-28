use std::sync::{Mutex, OnceLock};

use winit::event_loop::EventLoopProxy;

use crate::wake_signal::WakeSignal;

/// User-event payload used to wake the Unix winit loop from IPC/PTY threads.
pub type UnixWake = ();

static UNIX_WAKE_PROXY: OnceLock<Mutex<Option<EventLoopProxy<UnixWake>>>> = OnceLock::new();

fn unix_wake_proxy() -> &'static Mutex<Option<EventLoopProxy<UnixWake>>> {
    UNIX_WAKE_PROXY.get_or_init(|| Mutex::new(None))
}

/// Registers the EventLoopProxy unix_app uses to wake its winit event loop.
pub fn install_unix_wake(proxy: EventLoopProxy<UnixWake>) {
    *unix_wake_proxy()
        .lock()
        .expect("unix wake mailbox lock poisoned") = Some(proxy);
}

pub(crate) fn request_gui_wake(_wake_window: isize, wake_signal: &WakeSignal) {
    if !wake_signal.request() {
        return;
    }
    let guard = unix_wake_proxy()
        .lock()
        .expect("unix wake mailbox lock poisoned");
    if let Some(proxy) = guard.as_ref() {
        let _ = proxy.send_event(());
    }
}
