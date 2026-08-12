//! Preallocated bounded handoff from a blocking PTY reader to its consumer.

use std::sync::{Condvar, Mutex, MutexGuard};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OutputPushError {
    Closed,
    InputTooLarge { length: usize, capacity: usize },
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct OutputDrain {
    pub bytes: usize,
    pub backlog: bool,
    pub closed: bool,
}

struct State {
    storage: Box<[u8]>,
    read: usize,
    write: usize,
    length: usize,
    closed: bool,
}

/// Fixed-capacity byte ring with blocking producer backpressure and
/// allocation-free consumer slices.
///
/// A push is committed atomically as one read chunk. The consumer may receive
/// two slices when data wraps around the end of the ring, but byte order is
/// stable. `close` wakes a blocked producer and does not discard queued bytes.
pub struct BoundedOutputPipe {
    state: Mutex<State>,
    space_available: Condvar,
}

impl BoundedOutputPipe {
    pub fn new(capacity: usize) -> Self {
        let capacity = capacity.max(1);
        Self {
            state: Mutex::new(State {
                storage: vec![0; capacity].into_boxed_slice(),
                read: 0,
                write: 0,
                length: 0,
                closed: false,
            }),
            space_available: Condvar::new(),
        }
    }

    pub fn capacity(&self) -> usize {
        self.lock().storage.len()
    }

    pub fn close(&self) {
        let mut state = self.lock();
        state.closed = true;
        drop(state);
        self.space_available.notify_all();
    }

    pub fn push_blocking(&self, input: &[u8]) -> Result<(), OutputPushError> {
        if input.is_empty() {
            return Ok(());
        }
        let mut state = self.lock();
        let capacity = state.storage.len();
        if input.len() > capacity {
            return Err(OutputPushError::InputTooLarge {
                length: input.len(),
                capacity,
            });
        }
        while capacity - state.length < input.len() && !state.closed {
            state = self
                .space_available
                .wait(state)
                .unwrap_or_else(std::sync::PoisonError::into_inner);
        }
        if state.closed {
            return Err(OutputPushError::Closed);
        }

        let write = state.write;
        let first = input.len().min(capacity - write);
        state.storage[write..write + first].copy_from_slice(&input[..first]);
        let second = input.len() - first;
        if second != 0 {
            state.storage[..second].copy_from_slice(&input[first..]);
        }
        state.write = (write + input.len()) % capacity;
        state.length += input.len();
        Ok(())
    }

    pub fn drain(&self, budget: usize, mut consume: impl FnMut(&[u8])) -> OutputDrain {
        let mut state = self.lock();
        let mut drained = 0usize;
        while drained < budget && state.length != 0 {
            let available = state.length.min(state.storage.len() - state.read);
            let take = available.min(budget - drained);
            let read = state.read;
            consume(&state.storage[read..read + take]);
            state.read = (read + take) % state.storage.len();
            state.length -= take;
            drained += take;
        }
        let report = OutputDrain {
            bytes: drained,
            backlog: state.length != 0,
            closed: state.closed,
        };
        drop(state);
        if drained != 0 {
            self.space_available.notify_one();
        }
        report
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, mpsc};
    use std::time::Duration;

    use super::*;

    #[test]
    fn wraparound_drains_in_byte_order_without_reallocation() {
        let pipe = BoundedOutputPipe::new(8);
        assert_eq!(pipe.capacity(), 8);
        pipe.push_blocking(b"abcdef").unwrap();
        let mut first = Vec::new();
        assert_eq!(
            pipe.drain(5, |bytes| first.extend_from_slice(bytes)).bytes,
            5
        );
        pipe.push_blocking(b"WXYZ").unwrap();
        let mut rest = Vec::new();
        let report = pipe.drain(8, |bytes| rest.extend_from_slice(bytes));
        assert_eq!(rest, b"fWXYZ");
        assert_eq!(
            report,
            OutputDrain {
                bytes: 5,
                backlog: false,
                closed: false
            }
        );
    }

    #[test]
    fn budget_reports_backlog_and_close_preserves_queued_bytes() {
        let pipe = BoundedOutputPipe::new(8);
        pipe.push_blocking(b"abcdef").unwrap();
        pipe.close();
        let mut bytes = Vec::new();
        let first = pipe.drain(4, |chunk| bytes.extend_from_slice(chunk));
        assert_eq!(
            first,
            OutputDrain {
                bytes: 4,
                backlog: true,
                closed: true
            }
        );
        let second = pipe.drain(4, |chunk| bytes.extend_from_slice(chunk));
        assert_eq!(
            second,
            OutputDrain {
                bytes: 2,
                backlog: false,
                closed: true
            }
        );
        assert_eq!(bytes, b"abcdef");
        assert_eq!(pipe.push_blocking(b"x"), Err(OutputPushError::Closed));
    }

    #[test]
    fn draining_space_releases_a_blocked_producer() {
        let pipe = Arc::new(BoundedOutputPipe::new(4));
        pipe.push_blocking(b"full").unwrap();
        let producer = Arc::clone(&pipe);
        let (done_tx, done_rx) = mpsc::channel();
        let thread = std::thread::spawn(move || {
            let result = producer.push_blocking(b"next");
            done_tx.send(result).unwrap();
        });
        assert!(done_rx.try_recv().is_err());
        pipe.drain(4, |_| {});
        assert_eq!(
            done_rx.recv_timeout(Duration::from_secs(1)).unwrap(),
            Ok(())
        );
        thread.join().unwrap();
    }

    #[test]
    fn close_releases_a_blocked_producer() {
        let pipe = Arc::new(BoundedOutputPipe::new(4));
        pipe.push_blocking(b"full").unwrap();
        let producer = Arc::clone(&pipe);
        let thread = std::thread::spawn(move || producer.push_blocking(b"next"));
        pipe.close();
        assert_eq!(thread.join().unwrap(), Err(OutputPushError::Closed));
    }

    #[test]
    fn oversized_push_fails_without_partial_commit() {
        let pipe = BoundedOutputPipe::new(4);
        assert_eq!(
            pipe.push_blocking(b"12345"),
            Err(OutputPushError::InputTooLarge {
                length: 5,
                capacity: 4
            })
        );
        assert_eq!(pipe.drain(4, |_| {}).bytes, 0);
    }
}
