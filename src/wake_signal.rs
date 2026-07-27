use std::sync::atomic::{AtomicBool, Ordering};

/// Coalesces producer notifications into at most one outstanding GUI wake.
///
/// Producers enqueue work before calling `request`. The GUI handler calls
/// `begin_drain` before draining queues, allowing work that arrives during the
/// drain to publish a fresh wake. A bounded drain calls `rearm_if` when it
/// consumes its entire budget so already-queued work cannot be stranded.
#[derive(Debug, Default)]
pub(crate) struct WakeSignal {
    pending: AtomicBool,
}

impl WakeSignal {
    pub(crate) const fn new() -> Self {
        Self {
            pending: AtomicBool::new(false),
        }
    }

    /// Returns true only for the producer that transitions idle to pending.
    pub(crate) fn request(&self) -> bool {
        self.pending
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
    }

    /// Marks the posted wake as being handled before the consumer drains work.
    pub(crate) fn begin_drain(&self) {
        self.pending.store(false, Ordering::Release);
    }

    /// Returns true when a bounded consumer must publish another GUI wake.
    pub(crate) fn rearm_if(&self, more_work_may_remain: bool) -> bool {
        more_work_may_remain && self.request()
    }

    #[cfg(test)]
    fn is_pending(&self) -> bool {
        self.pending.load(Ordering::Acquire)
    }
}

#[cfg(test)]
mod tests {
    use std::{
        sync::{Arc, mpsc},
        thread,
        time::Duration,
    };

    use super::*;

    #[test]
    fn producers_coalesce_until_the_handler_begins_draining() {
        let signal = WakeSignal::new();
        assert!(signal.request());
        assert!(!signal.request());
        assert!(signal.is_pending());

        signal.begin_drain();
        assert!(!signal.is_pending());
        assert!(signal.request());
    }

    #[test]
    fn rearm_does_not_duplicate_a_wake_posted_during_drain() {
        let signal = WakeSignal::new();
        assert!(signal.request());
        signal.begin_drain();

        assert!(signal.request());
        assert!(!signal.rearm_if(true));
        assert!(signal.is_pending());
    }

    #[test]
    fn concurrent_bounded_drains_do_not_lose_wakes() {
        const PRODUCERS: usize = 8;
        const ITEMS_PER_PRODUCER: usize = 500;
        const DRAIN_BUDGET: usize = 7;

        let signal = Arc::new(WakeSignal::new());
        let (work_sender, work_receiver) = mpsc::channel();
        let (wake_sender, wake_receiver) = mpsc::channel();
        let consumer_wake_sender = wake_sender.clone();
        let mut producers = Vec::new();
        for producer in 0..PRODUCERS {
            let signal = Arc::clone(&signal);
            let work_sender = work_sender.clone();
            let wake_sender = wake_sender.clone();
            producers.push(thread::spawn(move || {
                for item in 0..ITEMS_PER_PRODUCER {
                    work_sender.send((producer, item)).unwrap();
                    if signal.request() {
                        wake_sender.send(()).unwrap();
                    }
                    if item % 17 == 0 {
                        thread::yield_now();
                    }
                }
            }));
        }
        drop(work_sender);
        drop(wake_sender);

        let expected = PRODUCERS * ITEMS_PER_PRODUCER;
        let mut consumed = 0;
        while consumed < expected {
            wake_receiver
                .recv_timeout(Duration::from_secs(5))
                .expect("queued work lost its wake");
            signal.begin_drain();

            let mut drained = 0;
            while drained < DRAIN_BUDGET {
                match work_receiver.try_recv() {
                    Ok(_) => {
                        consumed += 1;
                        drained += 1;
                    }
                    Err(mpsc::TryRecvError::Empty) => break,
                    Err(mpsc::TryRecvError::Disconnected) => break,
                }
            }
            if signal.rearm_if(drained == DRAIN_BUDGET) {
                // This models the single PostMessage performed by the GUI
                // consumer when a bounded drain may have left queued work.
                consumer_wake_sender.send(()).unwrap();
            }
        }

        for producer in producers {
            producer.join().unwrap();
        }
        assert_eq!(consumed, expected);
    }
}
