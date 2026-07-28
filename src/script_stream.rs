use std::{
    collections::VecDeque,
    io::{self, Read},
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

use rhai::{Engine, EvalAltResult};

use crate::{script_process::ScriptDuration, script_stdlib::ScriptBytes};

pub const STREAM_BUFFER_BYTES: usize = 64 * 1024;
pub const STREAM_READ_MAX_BYTES: usize = 64 * 1024;
const STREAM_PUMP_CHUNK_BYTES: usize = 8 * 1024;
static NEXT_STREAM_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum StreamStatus {
    Open,
    Closed,
    Failed,
    Cancelled,
}

#[derive(Debug)]
struct StreamState {
    status: StreamStatus,
    queue: VecDeque<u8>,
    archive: Vec<u8>,
    capture_limit: usize,
    truncated: bool,
    capture_truncated: bool,
}

#[derive(Debug)]
struct StreamInner {
    id: u64,
    kind: &'static str,
    state: Mutex<StreamState>,
    changed: Condvar,
    space: Condvar,
}

#[derive(Clone, Debug)]
pub struct ScriptStream(Arc<StreamInner>);

#[derive(Debug)]
pub(crate) struct CapturedStream {
    pub(crate) bytes: Vec<u8>,
    pub(crate) truncated: bool,
}

pub(crate) fn register(engine: &mut Engine) {
    engine.register_type_with_name::<ScriptStream>("Stream");
    engine.register_get("id", stream_id);
    engine.register_get("kind", stream_kind);
    engine.register_get("state", stream_state);
    engine.register_get("buffered_bytes", stream_buffered_bytes);
    engine.register_get("truncated", stream_truncated);
    engine.register_get("complete", stream_complete);
    engine.register_fn("read", stream_read);
    engine.register_fn("read", stream_read_for);
    engine.register_fn("collect", stream_collect);
    engine.register_fn("collect", stream_collect_for);
    engine.register_fn("close", stream_close);
}

pub(crate) fn from_reader(
    mut reader: impl Read + Send + 'static,
    kind: &'static str,
    capture_limit: usize,
) -> ScriptStream {
    let inner = Arc::new(StreamInner {
        id: NEXT_STREAM_ID.fetch_add(1, Ordering::Relaxed),
        kind,
        state: Mutex::new(StreamState {
            status: StreamStatus::Open,
            queue: VecDeque::with_capacity(STREAM_BUFFER_BYTES),
            archive: Vec::with_capacity(capture_limit.min(STREAM_PUMP_CHUNK_BYTES)),
            capture_limit,
            truncated: false,
            capture_truncated: false,
        }),
        changed: Condvar::new(),
        space: Condvar::new(),
    });
    let pump = Arc::clone(&inner);
    thread::spawn(move || {
        let mut buffer = [0_u8; STREAM_PUMP_CHUNK_BYTES];
        loop {
            match reader.read(&mut buffer) {
                Ok(0) => {
                    finish_pump(&pump, StreamStatus::Closed);
                    return;
                }
                Ok(read) => {
                    if !publish_bytes(&pump, &buffer[..read]) {
                        return;
                    }
                }
                Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                Err(_) => {
                    finish_pump(&pump, StreamStatus::Failed);
                    return;
                }
            }
        }
    });
    ScriptStream(inner)
}

fn publish_bytes(stream: &Arc<StreamInner>, bytes: &[u8]) -> bool {
    let mut state = match stream.state.lock() {
        Ok(state) => state,
        Err(_) => return false,
    };
    if state.status != StreamStatus::Open {
        return false;
    }

    let retained = bytes
        .len()
        .min(state.capture_limit.saturating_sub(state.archive.len()));
    state.archive.extend_from_slice(&bytes[..retained]);
    state.capture_truncated |= retained < bytes.len();

    let mut offset = 0;
    while offset < bytes.len() {
        state = match stream.space.wait_while(state, |state| {
            state.status == StreamStatus::Open && state.queue.len() >= STREAM_BUFFER_BYTES
        }) {
            Ok(state) => state,
            Err(_) => return false,
        };
        if state.status != StreamStatus::Open {
            return false;
        }
        let available = STREAM_BUFFER_BYTES.saturating_sub(state.queue.len());
        let count = available.min(bytes.len() - offset);
        state.queue.extend(&bytes[offset..offset + count]);
        offset += count;
        stream.changed.notify_all();
    }
    true
}

fn finish_pump(stream: &Arc<StreamInner>, status: StreamStatus) {
    let Ok(mut state) = stream.state.lock() else {
        return;
    };
    if state.status == StreamStatus::Open {
        state.status = status;
    }
    stream.changed.notify_all();
    stream.space.notify_all();
}

fn stream_id(stream: &mut ScriptStream) -> Result<rhai::INT, Box<EvalAltResult>> {
    rhai::INT::try_from(stream.0.id).map_err(|_| "stream_id_overflow".into())
}

fn stream_kind(stream: &mut ScriptStream) -> String {
    stream.0.kind.to_owned()
}

fn stream_state(stream: &mut ScriptStream) -> Result<String, Box<EvalAltResult>> {
    let state = stream.0.state.lock().map_err(|_| "stream_state_poisoned")?;
    Ok(status_name(&state).to_owned())
}

fn stream_buffered_bytes(stream: &mut ScriptStream) -> Result<rhai::INT, Box<EvalAltResult>> {
    let count = stream
        .0
        .state
        .lock()
        .map_err(|_| "stream_state_poisoned")?
        .queue
        .len();
    rhai::INT::try_from(count).map_err(|_| "stream_buffer_overflow".into())
}

fn stream_truncated(stream: &mut ScriptStream) -> Result<bool, Box<EvalAltResult>> {
    Ok(stream
        .0
        .state
        .lock()
        .map_err(|_| "stream_state_poisoned")?
        .truncated)
}

fn stream_complete(stream: &mut ScriptStream) -> Result<bool, Box<EvalAltResult>> {
    let state = stream.0.state.lock().map_err(|_| "stream_state_poisoned")?;
    Ok(state.status == StreamStatus::Closed && !state.truncated)
}

fn stream_read(
    stream: &mut ScriptStream,
    maximum: rhai::INT,
) -> Result<ScriptBytes, Box<EvalAltResult>> {
    read_inner(stream, bounded_read(maximum)?, None)
}

fn stream_read_for(
    stream: &mut ScriptStream,
    maximum: rhai::INT,
    timeout: ScriptDuration,
) -> Result<ScriptBytes, Box<EvalAltResult>> {
    read_inner(stream, bounded_read(maximum)?, Some(timeout.0))
}

fn read_inner(
    stream: &ScriptStream,
    maximum: usize,
    timeout: Option<Duration>,
) -> Result<ScriptBytes, Box<EvalAltResult>> {
    let deadline = timeout.map(|timeout| Instant::now() + timeout);
    let mut state = stream.0.state.lock().map_err(|_| "stream_state_poisoned")?;
    loop {
        if !state.queue.is_empty() {
            let count = maximum.min(state.queue.len());
            let bytes = state.queue.drain(..count).collect();
            stream.0.space.notify_all();
            return Ok(ScriptBytes(bytes));
        }
        match state.status {
            StreamStatus::Closed => return Ok(ScriptBytes(Vec::new())),
            StreamStatus::Failed => {
                return Err("stream_read_failed: producer could not read the source".into());
            }
            StreamStatus::Cancelled => {
                return Err("stream_closed: stream was closed by the consumer".into());
            }
            StreamStatus::Open => {}
        }
        state = if let Some(deadline) = deadline {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err("stream_read_timeout: stream remained unreadable".into());
            }
            let (state, timed) = stream
                .0
                .changed
                .wait_timeout(state, remaining)
                .map_err(|_| "stream_state_poisoned")?;
            if timed.timed_out() && state.queue.is_empty() && state.status == StreamStatus::Open {
                return Err("stream_read_timeout: stream remained unreadable".into());
            }
            state
        } else {
            stream
                .0
                .changed
                .wait(state)
                .map_err(|_| "stream_state_poisoned")?
        };
    }
}

fn stream_collect(
    stream: &mut ScriptStream,
    maximum: rhai::INT,
) -> Result<ScriptBytes, Box<EvalAltResult>> {
    collect_inner(stream, bounded_collect(maximum)?, None)
}

fn stream_collect_for(
    stream: &mut ScriptStream,
    maximum: rhai::INT,
    timeout: ScriptDuration,
) -> Result<ScriptBytes, Box<EvalAltResult>> {
    collect_inner(stream, bounded_collect(maximum)?, Some(timeout.0))
}

fn collect_inner(
    stream: &ScriptStream,
    maximum: usize,
    timeout: Option<Duration>,
) -> Result<ScriptBytes, Box<EvalAltResult>> {
    let deadline = timeout.map(|timeout| Instant::now() + timeout);
    let mut output = Vec::with_capacity(maximum.min(STREAM_PUMP_CHUNK_BYTES));
    loop {
        let remaining = deadline.map(|deadline| deadline.saturating_duration_since(Instant::now()));
        let chunk = read_inner(stream, STREAM_READ_MAX_BYTES, remaining)?;
        if chunk.0.is_empty() {
            return Ok(ScriptBytes(output));
        }
        if chunk.0.len() > maximum.saturating_sub(output.len()) {
            mark_truncated(stream)?;
            return Err(format!("stream_collect_limit: maximum is {maximum} bytes").into());
        }
        output.extend_from_slice(&chunk.0);
    }
}

fn stream_close(stream: &mut ScriptStream) -> Result<bool, Box<EvalAltResult>> {
    let mut state = stream.0.state.lock().map_err(|_| "stream_state_poisoned")?;
    if state.status != StreamStatus::Open {
        return Ok(false);
    }
    state.status = StreamStatus::Cancelled;
    state.truncated = true;
    state.queue.clear();
    stream.0.changed.notify_all();
    stream.0.space.notify_all();
    Ok(true)
}

fn mark_truncated(stream: &ScriptStream) -> Result<(), Box<EvalAltResult>> {
    stream
        .0
        .state
        .lock()
        .map_err(|_| "stream_state_poisoned")?
        .truncated = true;
    Ok(())
}

fn bounded_read(value: rhai::INT) -> Result<usize, Box<EvalAltResult>> {
    let value =
        usize::try_from(value).map_err(|_| "stream_read_size: value must be a positive integer")?;
    if value == 0 || value > STREAM_READ_MAX_BYTES {
        return Err(format!(
            "stream_read_size: value must be between 1 and {STREAM_READ_MAX_BYTES}"
        )
        .into());
    }
    Ok(value)
}

fn bounded_collect(value: rhai::INT) -> Result<usize, Box<EvalAltResult>> {
    let value = usize::try_from(value)
        .map_err(|_| "stream_collect_size: value must be a positive integer")?;
    if value == 0 || value > crate::script_protocol::ScriptBudgets::hard_limits().capture_bytes {
        return Err(format!(
            "stream_collect_size: value must be between 1 and {}",
            crate::script_protocol::ScriptBudgets::hard_limits().capture_bytes
        )
        .into());
    }
    Ok(value)
}

fn status_name(state: &StreamState) -> &'static str {
    if !state.queue.is_empty() {
        return "readable";
    }
    match state.status {
        StreamStatus::Open => "pending",
        StreamStatus::Closed => "closed",
        StreamStatus::Failed => "failed",
        StreamStatus::Cancelled => "cancelled",
    }
}

pub(crate) fn discard_buffered(stream: &ScriptStream) -> Result<(), Box<EvalAltResult>> {
    let mut state = stream.0.state.lock().map_err(|_| "stream_state_poisoned")?;
    if !state.queue.is_empty() {
        state.queue.clear();
        stream.0.space.notify_all();
    }
    Ok(())
}

pub(crate) fn capture_after_close(
    stream: &ScriptStream,
    deadline: Instant,
) -> Result<CapturedStream, Box<EvalAltResult>> {
    let mut state = stream.0.state.lock().map_err(|_| "stream_state_poisoned")?;
    loop {
        if !state.queue.is_empty() {
            state.queue.clear();
            stream.0.space.notify_all();
        }
        match state.status {
            StreamStatus::Closed => {
                return Ok(CapturedStream {
                    bytes: state.archive.clone(),
                    truncated: state.truncated || state.capture_truncated,
                });
            }
            StreamStatus::Failed => {
                return Err("stream_read_failed: producer could not read the source".into());
            }
            StreamStatus::Cancelled => {
                return Ok(CapturedStream {
                    bytes: state.archive.clone(),
                    truncated: true,
                });
            }
            StreamStatus::Open => {}
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err("process_timeout: output stream remained open".into());
        }
        let (next, timed) = stream
            .0
            .changed
            .wait_timeout(state, remaining.min(Duration::from_millis(5)))
            .map_err(|_| "stream_state_poisoned")?;
        state = next;
        if timed.timed_out() {
            continue;
        }
    }
}

pub(crate) fn cancel(stream: &ScriptStream) {
    let Ok(mut state) = stream.0.state.lock() else {
        return;
    };
    if state.status == StreamStatus::Open {
        state.status = StreamStatus::Cancelled;
        state.truncated = true;
        state.queue.clear();
    }
    stream.0.changed.notify_all();
    stream.0.space.notify_all();
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn stream_reads_bounded_chunks_and_reports_clean_close() {
        let mut stream = from_reader(Cursor::new(b"abcdef".to_vec()), "bytes", 32);
        assert_eq!(stream_read(&mut stream, 2).unwrap().0, b"ab");
        assert_eq!(stream_collect(&mut stream, 8).unwrap().0, b"cdef");
        assert_eq!(stream_state(&mut stream).unwrap(), "closed");
        assert!(stream_complete(&mut stream).unwrap());
        assert!(!stream_truncated(&mut stream).unwrap());
    }

    #[test]
    fn final_capture_truncation_does_not_truncate_live_delivery() {
        let mut stream = from_reader(Cursor::new(vec![b'x'; 32]), "bytes", 4);
        let delivered = stream_collect(&mut stream, 64).unwrap();
        assert_eq!(delivered.0, vec![b'x'; 32]);
        assert!(stream_complete(&mut stream).unwrap());
        let capture =
            capture_after_close(&stream, Instant::now() + Duration::from_secs(1)).unwrap();
        assert_eq!(capture.bytes, b"xxxx");
        assert!(capture.truncated);
        assert!(stream_complete(&mut stream).unwrap());
    }

    #[test]
    fn close_wakes_a_backpressured_producer() {
        let mut stream = from_reader(Cursor::new(vec![b'x'; STREAM_BUFFER_BYTES * 2]), "bytes", 8);
        let deadline = Instant::now() + Duration::from_secs(1);
        while stream_buffered_bytes(&mut stream).unwrap() < STREAM_BUFFER_BYTES as i64 {
            assert!(Instant::now() < deadline);
            thread::yield_now();
        }
        assert!(stream_close(&mut stream).unwrap());
        assert_eq!(stream_state(&mut stream).unwrap(), "cancelled");
        assert!(stream_truncated(&mut stream).unwrap());
    }

    #[test]
    fn read_timeout_does_not_silently_close_the_stream() {
        struct DelayedEof;

        impl Read for DelayedEof {
            fn read(&mut self, _buffer: &mut [u8]) -> io::Result<usize> {
                thread::sleep(Duration::from_millis(25));
                Ok(0)
            }
        }

        let mut stream = from_reader(DelayedEof, "bytes", 8);
        let error = stream_read_for(&mut stream, 1, ScriptDuration(Duration::from_millis(1)))
            .unwrap_err()
            .to_string();
        assert!(error.contains("stream_read_timeout"));
        assert_eq!(stream_state(&mut stream).unwrap(), "pending");
        assert!(stream_close(&mut stream).unwrap());
    }

    #[test]
    fn collect_limit_is_typed_and_marks_the_result_incomplete() {
        let mut stream = from_reader(Cursor::new(vec![b'x'; 32]), "bytes", 32);
        let error = stream_collect(&mut stream, 4).unwrap_err().to_string();
        assert!(error.contains("stream_collect_limit"));
        assert!(stream_truncated(&mut stream).unwrap());
        assert!(!stream_complete(&mut stream).unwrap());
    }
}
