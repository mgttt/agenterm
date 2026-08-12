//! PTY facade projection; native handles remain adapter-private to selection.

pub use crate::contract::pty::{
    InvalidProcessId, NativeInputOwnership, NativeTerminalKey, ProcessId, PtyError, PtyResult,
    TerminalSize,
};
pub use crate::selected::pty::{ChildCommand, PtyChild, PtyMaster, login_shell_argument};

const PTY_SHUTDOWN_QUEUE_CAPACITY: usize = 64;

struct PtyShutdown {
    master: Option<PtyMaster>,
    child: Option<PtyChild>,
}

impl PtyShutdown {
    fn run(self) {
        if let Some(child) = self.child.as_ref() {
            let _ = child.terminate_forcefully();
            child.close_pseudoconsole();
        }
        drop(self.master);
        drop(self.child);
    }
}

fn shutdown_sender() -> std::io::Result<
    &'static std::sync::mpsc::SyncSender<PtyShutdown>,
> {
    type ReaperInit = Result<
        std::sync::mpsc::SyncSender<PtyShutdown>,
        (std::io::ErrorKind, String),
    >;
    static SENDER: std::sync::OnceLock<ReaperInit> = std::sync::OnceLock::new();
    match SENDER.get_or_init(|| {
        let (sender, receiver) =
            std::sync::mpsc::sync_channel::<PtyShutdown>(PTY_SHUTDOWN_QUEUE_CAPACITY);
        crate::threading::spawn_named_detached(
            "agenterm-pty-reaper",
            Box::new(move || {
                while let Ok(shutdown) = receiver.recv() {
                    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        shutdown.run();
                    }));
                }
            }),
        )
        .map_err(|error| (error.kind(), error.to_string()))?;
        Ok(sender)
    }) {
        Ok(sender) => Ok(sender),
        Err((kind, message)) => Err(std::io::Error::new(*kind, message.clone())),
    }
}

/// Prepare the process-wide PTY teardown owner before opening native sessions.
///
/// GUI hosts should treat failure as a terminal-creation failure. Doing this
/// before native PTY acquisition keeps close paths from discovering thread
/// creation failure while they already own resources that may block on drop.
pub fn initialize_shutdown_reaper() -> std::io::Result<()> {
    shutdown_sender().map(|_| ())
}

/// Relinquish a complete PTY session without running potentially blocking
/// native teardown on the caller's event thread.
///
/// Closing a Windows pseudoconsole may wait for hosted processes and pipe
/// drainage. Unix reader clones can independently retain the master fd. The
/// detached owner therefore performs termination and drops both halves in one
/// place after the product has stopped accepting output.
pub fn shutdown_session_detached(
    master: Option<PtyMaster>,
    child: Option<PtyChild>,
) -> std::io::Result<()> {
    if master.is_none() && child.is_none() {
        return Ok(());
    }
    let shutdown = PtyShutdown { master, child };
    match shutdown_sender()?.try_send(shutdown) {
        Ok(()) => Ok(()),
        Err(std::sync::mpsc::TrySendError::Full(shutdown))
        | Err(std::sync::mpsc::TrySendError::Disconnected(shutdown)) => {
            crate::threading::spawn_named_detached(
                "agenterm-pty-reaper-overflow",
                Box::new(move || shutdown.run()),
            )
        }
    }
}

mod output;
pub use output::{BoundedOutputPipe, OutputDrain, OutputPushError};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn login_shell_argument_is_selected_by_the_platform_adapter() {
        #[cfg(windows)]
        {
            assert_eq!(login_shell_argument(std::path::Path::new("bash"), 0), None);
        }
        #[cfg(unix)]
        {
            assert_eq!(
                login_shell_argument(std::path::Path::new("/bin/zsh"), 0),
                Some("-l")
            );
            assert_eq!(
                login_shell_argument(std::path::Path::new("/bin/zsh"), 1),
                None
            );
            assert_eq!(
                login_shell_argument(std::path::Path::new("/bin/custom-shell"), 0),
                None
            );
        }
    }

    #[test]
    fn detached_shutdown_reuses_one_platform_reaper() {
        initialize_shutdown_reaper().expect("initialize PTY reaper");
        initialize_shutdown_reaper().expect("reinitialize PTY reaper");
        let first = shutdown_sender().expect("start PTY reaper") as *const _;
        let second = shutdown_sender().expect("reuse PTY reaper") as *const _;
        assert_eq!(first, second);
    }
}
