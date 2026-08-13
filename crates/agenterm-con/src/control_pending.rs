//! Pending control-request ownership and completion.
//!
//! Session inspection and frame capture stay in `ConApp`; this module owns
//! queue bounds, deadlines, cancellation, and exactly-once reply transfer.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use crate::control::ReplySender;
use crate::json::JsonValue;
use crate::workspace::TabId;

const MAX_WAITS: usize = 32;
const MAX_WAIT_TIMEOUT_MS: u64 = 10 * 60 * 1000;

pub(super) enum WaitKind {
    Text(String),
    TabExit,
}

pub(super) enum WaitProbe {
    Completed(JsonValue),
    Missing(String),
    Pending,
}

struct PendingWait {
    target: TabId,
    kind: WaitKind,
    deadline: Instant,
    reply: ReplySender,
}

pub(super) struct ScreenshotWork {
    pub(super) target: TabId,
    pub(super) path: PathBuf,
    pub(super) reply: ReplySender,
    pub(super) restore_active: Option<TabId>,
}

struct InflightScreenshot {
    target: TabId,
    reply: Arc<Mutex<Option<ReplySender>>>,
    done: Arc<AtomicBool>,
}

#[derive(Default)]
pub(super) struct PendingControl {
    waits: Vec<PendingWait>,
    screenshot: Option<ScreenshotWork>,
    inflight_screenshot: Option<InflightScreenshot>,
}

impl PendingControl {
    pub(super) fn wait_count(&self) -> usize {
        self.waits.len()
    }

    pub(super) fn screenshot_count(&self) -> usize {
        usize::from(self.screenshot.is_some()) + usize::from(self.inflight_screenshot.is_some())
    }

    pub(super) fn has_pending_screenshot(&self) -> bool {
        self.screenshot.is_some()
    }

    pub(super) fn enqueue_wait(
        &mut self,
        target: TabId,
        kind: WaitKind,
        timeout_ms: u64,
        reply: &mut Option<ReplySender>,
        capacity_error: &'static str,
    ) -> Result<(), String> {
        if self.waits.len() >= MAX_WAITS {
            return Err(capacity_error.to_owned());
        }
        if timeout_ms > MAX_WAIT_TIMEOUT_MS {
            return Err(format!(
                "wait timeout exceeds the {MAX_WAIT_TIMEOUT_MS} ms limit"
            ));
        }
        let deadline = Instant::now() + Duration::from_millis(timeout_ms);
        self.waits.push(PendingWait {
            target,
            kind,
            deadline,
            reply: reply.take().expect("control reply available"),
        });
        Ok(())
    }

    pub(super) fn poll_waits(
        &mut self,
        now: Instant,
        mut probe: impl FnMut(TabId, &WaitKind) -> WaitProbe,
    ) -> Option<Instant> {
        let mut pending = Vec::new();
        let mut next = None;
        for wait in std::mem::take(&mut self.waits) {
            match probe(wait.target, &wait.kind) {
                WaitProbe::Completed(response) => {
                    let _ = wait.reply.send(Ok(response));
                }
                WaitProbe::Missing(error) => {
                    let _ = wait.reply.send(Err(error));
                }
                WaitProbe::Pending if now >= wait.deadline => {
                    let error = match &wait.kind {
                        WaitKind::Text(text) => {
                            format!("wait-text timed out waiting for {text:?}")
                        }
                        WaitKind::TabExit => {
                            format!("wait-tab-exit timed out waiting for @{}", wait.target.get())
                        }
                    };
                    let _ = wait.reply.send(Err(error));
                }
                WaitProbe::Pending => {
                    next = Some(
                        next.map_or(wait.deadline, |current: Instant| current.min(wait.deadline)),
                    );
                    pending.push(wait);
                }
            }
        }
        self.waits = pending;
        next
    }

    pub(super) fn enqueue_screenshot(
        &mut self,
        target: TabId,
        path: PathBuf,
        reply: &mut Option<ReplySender>,
    ) -> Result<(), String> {
        if self.screenshot.is_some() || self.inflight_screenshot.is_some() {
            return Err("a screenshot is already pending".to_owned());
        }
        self.screenshot = Some(ScreenshotWork {
            target,
            path,
            reply: reply.take().expect("control reply available"),
            restore_active: None,
        });
        Ok(())
    }

    pub(super) fn prepare_screenshot(&mut self, active: Option<TabId>) -> Option<TabId> {
        let screenshot = self.screenshot.as_mut()?;
        screenshot.restore_active = (active != Some(screenshot.target))
            .then_some(active)
            .flatten();
        Some(screenshot.target)
    }

    pub(super) fn take_screenshot(&mut self) -> Option<ScreenshotWork> {
        self.screenshot.take()
    }

    pub(super) fn start_screenshot(
        &mut self,
        target: TabId,
        reply: Arc<Mutex<Option<ReplySender>>>,
        done: Arc<AtomicBool>,
    ) {
        self.inflight_screenshot = Some(InflightScreenshot {
            target,
            reply,
            done,
        });
    }

    pub(super) fn reap_finished_screenshot(&mut self) {
        if self
            .inflight_screenshot
            .as_ref()
            .is_some_and(|screenshot| screenshot.done.load(Ordering::Acquire))
        {
            self.inflight_screenshot = None;
        }
    }

    pub(super) fn cancel_for_tab(&mut self, id: TabId, reason: &str) {
        let mut retained = Vec::with_capacity(self.waits.len());
        for wait in std::mem::take(&mut self.waits) {
            if wait.target == id {
                let _ = wait.reply.send(Err(reason.to_owned()));
            } else {
                retained.push(wait);
            }
        }
        self.waits = retained;
        if self
            .screenshot
            .as_ref()
            .is_some_and(|shot| shot.target == id)
            && let Some(screenshot) = self.screenshot.take()
        {
            let _ = screenshot.reply.send(Err(reason.to_owned()));
        }
        if self
            .inflight_screenshot
            .as_ref()
            .is_some_and(|shot| shot.target == id)
            && let Some(screenshot) = self.inflight_screenshot.take()
            && let Some(reply) = screenshot
                .reply
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .take()
        {
            let _ = reply.send(Err(reason.to_owned()));
        }
    }

    pub(super) fn cancel_all(&mut self, reason: &str) {
        for wait in std::mem::take(&mut self.waits) {
            let _ = wait.reply.send(Err(reason.to_owned()));
        }
        if let Some(screenshot) = self.screenshot.take() {
            let _ = screenshot.reply.send(Err(reason.to_owned()));
        }
        if let Some(screenshot) = self.inflight_screenshot.take()
            && let Some(reply) = screenshot
                .reply
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .take()
        {
            let _ = reply.send(Err(reason.to_owned()));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{control, json};

    fn reply() -> (ReplySender, control::ReplyReceiver) {
        control::reply_channel()
    }

    #[test]
    fn wait_capacity_fails_without_stealing_the_extra_reply() {
        let mut pending = PendingControl::default();
        for _ in 0..MAX_WAITS {
            let (sender, _receiver) = reply();
            let mut sender = Some(sender);
            pending
                .enqueue_wait(
                    TabId::new(1),
                    WaitKind::TabExit,
                    1000,
                    &mut sender,
                    "capacity",
                )
                .expect("within capacity");
            assert!(sender.is_none());
        }
        let (sender, _receiver) = reply();
        let mut sender = Some(sender);
        assert_eq!(
            pending.enqueue_wait(
                TabId::new(1),
                WaitKind::TabExit,
                1000,
                &mut sender,
                "capacity",
            ),
            Err("capacity".to_owned())
        );
        assert!(sender.is_some());
    }

    #[test]
    fn oversized_wait_timeout_fails_without_stealing_the_reply() {
        let mut pending = PendingControl::default();
        let (sender, _receiver) = reply();
        let mut sender = Some(sender);
        assert_eq!(
            pending.enqueue_wait(
                TabId::new(1),
                WaitKind::Text("ready".to_owned()),
                u64::MAX,
                &mut sender,
                "capacity",
            ),
            Err(format!(
                "wait timeout exceeds the {MAX_WAIT_TIMEOUT_MS} ms limit"
            ))
        );
        assert!(sender.is_some());
        assert_eq!(pending.wait_count(), 0);
    }

    #[test]
    fn screenshot_busy_rejection_preserves_the_callers_reply() {
        let mut pending = PendingControl::default();
        let (accepted, _accepted_receiver) = reply();
        let mut accepted = Some(accepted);
        pending
            .enqueue_screenshot(TabId::new(1), PathBuf::from("first.png"), &mut accepted)
            .expect("first screenshot");
        assert!(accepted.is_none());

        let (rejected, _rejected_receiver) = reply();
        let mut rejected = Some(rejected);
        assert_eq!(
            pending.enqueue_screenshot(TabId::new(2), PathBuf::from("second.png"), &mut rejected,),
            Err("a screenshot is already pending".to_owned())
        );
        assert!(rejected.is_some());
    }

    #[test]
    fn cancellation_completes_each_owned_reply_once() {
        let mut pending = PendingControl::default();
        let (sender, receiver) = reply();
        let mut sender = Some(sender);
        pending
            .enqueue_wait(
                TabId::new(7),
                WaitKind::Text("ready".to_owned()),
                1000,
                &mut sender,
                "capacity",
            )
            .expect("enqueue");
        pending.cancel_for_tab(TabId::new(7), "closed");
        assert_eq!(
            receiver
                .recv_timeout(Duration::from_secs(1))
                .expect("reply"),
            Err("closed".to_owned())
        );
        pending.cancel_all("window closed");
        assert!(receiver.recv_timeout(Duration::ZERO).is_err());
    }

    #[test]
    fn poll_reports_completion_and_nearest_deadline() {
        let mut pending = PendingControl::default();
        let (done_sender, done_receiver) = reply();
        let (wait_sender, _wait_receiver) = reply();
        let mut done_sender = Some(done_sender);
        let mut wait_sender = Some(wait_sender);
        pending
            .enqueue_wait(
                TabId::new(1),
                WaitKind::TabExit,
                0,
                &mut done_sender,
                "capacity",
            )
            .expect("enqueue done");
        pending
            .enqueue_wait(
                TabId::new(2),
                WaitKind::TabExit,
                1000,
                &mut wait_sender,
                "capacity",
            )
            .expect("enqueue pending");
        let now = Instant::now();
        let next = pending.poll_waits(now, |target, _| {
            if target == TabId::new(1) {
                WaitProbe::Completed(json::object(vec![("done", true.into())]))
            } else {
                WaitProbe::Pending
            }
        });
        assert!(
            done_receiver
                .recv_timeout(Duration::from_secs(1))
                .expect("reply")
                .is_ok()
        );
        assert!(next.is_some_and(|deadline| deadline > now));
        assert_eq!(pending.wait_count(), 1);
    }
}
