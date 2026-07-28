use std::sync::{Mutex, OnceLock, mpsc::SyncSender};

use crate::wake_signal::WakeSignal;

static UNIX_WAKE_SENDER: OnceLock<Mutex<Option<SyncSender<()>>>> = OnceLock::new();

fn unix_wake_sender() -> &'static Mutex<Option<SyncSender<()>>> {
    UNIX_WAKE_SENDER.get_or_init(|| Mutex::new(None))
}

/// Registers the channel unix_app uses to wake its winit event loop.
pub fn install_unix_wake(sender: SyncSender<()>) {
    *unix_wake_sender()
        .lock()
        .expect("unix wake mailbox lock poisoned") = Some(sender);
}

pub(crate) fn request_gui_wake(_wake_window: isize, wake_signal: &WakeSignal) {
    if !wake_signal.request() {
        return;
    }
    let guard = unix_wake_sender()
        .lock()
        .expect("unix wake mailbox lock poisoned");
    if let Some(sender) = guard.as_ref() {
        let _ = sender.try_send(());
    }
}
