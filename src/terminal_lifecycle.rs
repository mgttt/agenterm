use std::{
    collections::VecDeque,
    time::{Duration, Instant},
};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct TerminalLifecycle {
    reader_closed: bool,
    parser_drained: bool,
}

impl TerminalLifecycle {
    pub(crate) fn close_reader_after_parser_drain(&mut self) {
        self.reader_closed = true;
        self.parser_drained = true;
    }

    pub(crate) fn reader_closed(self) -> bool {
        self.reader_closed
    }

    pub(crate) fn parser_drained(self) -> bool {
        self.parser_drained
    }

    pub(crate) fn finalized(self, process_finished: bool) -> bool {
        process_finished && self.reader_closed && self.parser_drained
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) enum SubmissionState {
    #[default]
    Idle,
    Pending {
        due_at: Instant,
    },
}

impl SubmissionState {
    pub(crate) fn schedule(&mut self, now: Instant, delay: Duration) -> bool {
        if self.is_pending() {
            return false;
        }
        *self = Self::Pending {
            due_at: now + delay,
        };
        true
    }

    pub(crate) fn is_pending(self) -> bool {
        matches!(self, Self::Pending { .. })
    }

    pub(crate) fn take_if_due(&mut self, now: Instant) -> bool {
        if matches!(self, Self::Pending { due_at } if now >= *due_at) {
            *self = Self::Idle;
            true
        } else {
            false
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct BoundedByteRing {
    bytes: VecDeque<u8>,
    capacity: usize,
}

impl BoundedByteRing {
    pub(crate) fn new(capacity: usize) -> Self {
        Self {
            bytes: VecDeque::with_capacity(capacity),
            capacity,
        }
    }

    pub(crate) fn extend(&mut self, incoming: &[u8]) {
        if self.capacity == 0 {
            return;
        }
        if incoming.len() >= self.capacity {
            self.bytes.clear();
            self.bytes
                .extend(incoming[incoming.len() - self.capacity..].iter().copied());
            return;
        }
        let overflow = self
            .bytes
            .len()
            .saturating_add(incoming.len())
            .saturating_sub(self.capacity);
        self.bytes.drain(..overflow);
        self.bytes.extend(incoming.iter().copied());
    }

    pub(crate) fn to_vec(&self) -> Vec<u8> {
        self.bytes.iter().copied().collect()
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.bytes.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terminal_is_final_only_after_process_and_reader_parser_completion() {
        let mut lifecycle = TerminalLifecycle::default();
        assert!(!lifecycle.finalized(false));
        assert!(!lifecycle.finalized(true));

        lifecycle.close_reader_after_parser_drain();
        assert!(lifecycle.reader_closed());
        assert!(lifecycle.parser_drained());
        assert!(!lifecycle.finalized(false));
        assert!(lifecycle.finalized(true));
    }

    #[test]
    fn submission_transitions_once_when_its_deadline_arrives() {
        let now = Instant::now();
        let mut submission = SubmissionState::default();
        assert!(submission.schedule(now, Duration::from_millis(10)));
        assert!(!submission.schedule(now, Duration::from_millis(10)));
        assert!(!submission.take_if_due(now + Duration::from_millis(9)));
        assert!(submission.take_if_due(now + Duration::from_millis(10)));
        assert!(!submission.take_if_due(now + Duration::from_millis(11)));
        assert!(!submission.is_pending());
    }

    #[test]
    fn byte_ring_keeps_only_the_newest_bounded_bytes() {
        let mut ring = BoundedByteRing::new(5);
        ring.extend(b"abc");
        ring.extend(b"def");
        assert_eq!(ring.len(), 5);
        assert_eq!(ring.to_vec(), b"bcdef");

        ring.extend(b"0123456789");
        assert_eq!(ring.len(), 5);
        assert_eq!(ring.to_vec(), b"56789");
    }

    #[test]
    fn zero_capacity_ring_never_retains_input() {
        let mut ring = BoundedByteRing::new(0);
        ring.extend(b"secret");
        assert_eq!(ring.len(), 0);
        assert!(ring.to_vec().is_empty());
    }
}
