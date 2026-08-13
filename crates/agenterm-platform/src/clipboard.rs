//! Platform-neutral Unicode clipboard facade.
//!
//! This source is intentionally independent of terminal, editor, or product
//! paste policy. Every read receives its maximum retained UTF-8 byte count
//! from the caller.

use std::{
    sync::mpsc::{self, Receiver, SyncSender, TryRecvError},
    time::Duration,
};

use crate::{
    contract::clipboard::{ClipboardError, ClipboardResult},
    selected, threading,
};

const DEFAULT_OPEN_TIMEOUT: Duration = Duration::from_millis(500);

/// Publish Unicode text through the selected native clipboard adapter.
pub fn set_text(text: &str) -> ClipboardResult<()> {
    set_text_with_timeout(text, DEFAULT_OPEN_TIMEOUT)
}

pub fn set_text_with_timeout(text: &str, open_timeout: Duration) -> ClipboardResult<()> {
    selected::clipboard::set_text(text, open_timeout).map_err(selected::clipboard::map_error)
}

/// Read Unicode text while retaining no more than `max_read_bytes` UTF-8
/// bytes. Exceeding the caller's bound is a typed `Failed` result.
pub fn get_text(max_read_bytes: usize) -> ClipboardResult<String> {
    get_text_with_timeout(max_read_bytes, DEFAULT_OPEN_TIMEOUT)
}

pub fn get_text_with_timeout(
    max_read_bytes: usize,
    open_timeout: Duration,
) -> ClipboardResult<String> {
    selected::clipboard::get_text(max_read_bytes, open_timeout)
        .map_err(selected::clipboard::map_error)
}

pub enum ClipboardTextReadPoll {
    Pending,
    Ready(ClipboardResult<String>),
}

pub struct ClipboardTextRead {
    receiver: Receiver<ClipboardResult<String>>,
}

impl ClipboardTextRead {
    pub fn try_poll(&self) -> ClipboardTextReadPoll {
        match self.receiver.try_recv() {
            Ok(result) => ClipboardTextReadPoll::Ready(result),
            Err(TryRecvError::Empty) => ClipboardTextReadPoll::Pending,
            Err(TryRecvError::Disconnected) => {
                ClipboardTextReadPoll::Ready(Err(ClipboardError::failed(
                    "clipboard_worker_disconnected",
                    "clipboard read worker stopped without a result",
                )))
            }
        }
    }
}

struct ClipboardReadCompletion {
    sender: Option<SyncSender<ClipboardResult<String>>>,
    wake: Option<Box<dyn FnOnce() + Send + 'static>>,
}

impl ClipboardReadCompletion {
    fn complete(mut self, result: ClipboardResult<String>) {
        if let Some(sender) = self.sender.take() {
            let _ = sender.send(result);
        }
        if let Some(wake) = self.wake.take() {
            wake();
        }
    }
}

impl Drop for ClipboardReadCompletion {
    fn drop(&mut self) {
        if let Some(sender) = self.sender.take() {
            let _ = sender.send(Err(ClipboardError::failed(
                "clipboard_worker_panicked",
                "clipboard read worker panicked",
            )));
        }
        if let Some(wake) = self.wake.take() {
            wake();
        }
    }
}

/// Starts one bounded clipboard read outside the caller's thread. Completion,
/// adapter failure, panic, and disconnect are all observable through
/// [`ClipboardTextRead::try_poll`]; `wake` is invoked once when the worker
/// reaches a terminal state.
pub fn read_text_async(
    max_read_bytes: usize,
    wake: impl FnOnce() + Send + 'static,
) -> ClipboardResult<ClipboardTextRead> {
    let (sender, receiver) = mpsc::sync_channel(1);
    threading::spawn_named_detached(
        "agenterm-clipboard-read",
        Box::new(move || {
            let completion = ClipboardReadCompletion {
                sender: Some(sender),
                wake: Some(Box::new(wake)),
            };
            completion.complete(get_text(max_read_bytes));
        }),
    )
    .map_err(|error| {
        ClipboardError::failed(
            "clipboard_worker_start",
            format!("could not start clipboard read worker: {error}"),
        )
    })?;
    Ok(ClipboardTextRead { receiver })
}

/// Probe whether Unicode text is presently available without requiring a full
/// payload read where the selected adapter can provide a cheaper probe.
pub fn has_unicode_text() -> bool {
    selected::clipboard::has_unicode_text()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_read_poll_distinguishes_pending_ready_and_disconnect() {
        let (sender, receiver) = mpsc::sync_channel(1);
        let read = ClipboardTextRead { receiver };
        assert!(matches!(read.try_poll(), ClipboardTextReadPoll::Pending));
        sender.send(Ok("ready".to_owned())).unwrap();
        assert!(matches!(
            read.try_poll(),
            ClipboardTextReadPoll::Ready(Ok(text)) if text == "ready"
        ));

        let (sender, receiver) = mpsc::sync_channel(1);
        let disconnected = ClipboardTextRead { receiver };
        drop(sender);
        assert!(matches!(
            disconnected.try_poll(),
            ClipboardTextReadPoll::Ready(Err(ClipboardError::Failed { code, .. }))
                if code == "clipboard_worker_disconnected"
        ));
    }

    #[test]
    fn completion_guard_reports_panic_shape_and_wakes_once() {
        let (sender, receiver) = mpsc::sync_channel(1);
        let wakes = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let wake_count = std::sync::Arc::clone(&wakes);
        drop(ClipboardReadCompletion {
            sender: Some(sender),
            wake: Some(Box::new(move || {
                wake_count.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            })),
        });
        let read = ClipboardTextRead { receiver };
        assert!(matches!(
            read.try_poll(),
            ClipboardTextReadPoll::Ready(Err(ClipboardError::Failed { code, .. }))
                if code == "clipboard_worker_panicked"
        ));
        assert_eq!(wakes.load(std::sync::atomic::Ordering::Relaxed), 1);
    }
}
