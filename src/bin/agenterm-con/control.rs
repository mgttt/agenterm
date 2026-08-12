//! Fixed, GUI-lifetime control grammar for `agenterm-con`.
//!
//! This deliberately models only direct terminal interaction.  It is not a
//! scripting language, mux protocol, workspace store, or background service.

use std::io::{Read as _, Write as _};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use agenterm_platform::ipc::{IpcEndpoint, IpcTransportErrorCode, NativeListener, NativeStream};

use super::{
    json::{self, JsonValue},
    workspace::TabId,
};

const REQUEST_MAX_BYTES: usize = 1024 * 1024;
const RESPONSE_MAX_BYTES: usize = 2 * 1024 * 1024;
const WIRE_MAGIC: [u8; 4] = *b"ATC1";
const CONNECT_TIMEOUT: Duration = Duration::from_secs(3);
const GUI_RESPONSE_TIMEOUT: Duration = Duration::from_secs(125);
const ACCEPT_POLL: Duration = Duration::from_millis(200);

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CliRequest {
    pub control: String,
    pub command: CliCommand,
}

#[cfg(test)]
mod compact_channel_tests {
    use super::*;

    #[test]
    fn dropping_reply_sender_wakes_receiver_without_waiting_for_timeout() {
        let (sender, receiver) = reply_channel();
        drop(sender);
        let started = std::time::Instant::now();
        assert!(receiver.recv_timeout(Duration::from_secs(10)).is_err());
        assert!(started.elapsed() < Duration::from_secs(1));
    }

    #[test]
    fn closed_request_queue_rejects_and_releases_a_waiter() {
        let alive = Arc::new(AtomicBool::new(true));
        let queue = RequestQueue::new(alive);
        queue.close();
        let (reply, receiver) = reply_channel();
        assert!(
            queue
                .push(IncomingRequest {
                    command: CliCommand::ListTabs,
                    reply,
                })
                .is_err()
        );
        assert!(receiver.recv_timeout(Duration::from_secs(1)).is_err());
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CliCommand {
    ListTabs,
    PerfStats,
    ResetPerfStats,
    NewTab {
        parent: Option<TabId>,
    },
    SelectTab {
        target: TabId,
    },
    CloseTab {
        target: TabId,
    },
    CapturePane {
        target: Option<TabId>,
        max_bytes: usize,
    },
    ScreenshotPane {
        target: Option<TabId>,
        output: String,
    },
    SendText {
        target: Option<TabId>,
        text: String,
    },
    SendPaste {
        target: Option<TabId>,
        text: String,
    },
    SendKeys {
        target: Option<TabId>,
        keys: Vec<String>,
    },
    SendMouse {
        target: Option<TabId>,
        action: MouseAction,
        button: MouseButton,
        column: u16,
        row: u16,
    },
    SendWheel {
        target: Option<TabId>,
        column: u16,
        row: u16,
        notches: i16,
        ctrl: bool,
    },
    WaitText {
        target: Option<TabId>,
        text: String,
        timeout_ms: u64,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MouseAction {
    Press,
    Release,
    Move,
    Click,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MouseButton {
    None,
    Left,
    Middle,
    Right,
}

impl MouseAction {
    fn wire_tag(&self) -> u8 {
        match self {
            Self::Press => 0,
            Self::Release => 1,
            Self::Move => 2,
            Self::Click => 3,
        }
    }

    fn from_wire_tag(tag: u8) -> Option<Self> {
        match tag {
            0 => Some(Self::Press),
            1 => Some(Self::Release),
            2 => Some(Self::Move),
            3 => Some(Self::Click),
            _ => None,
        }
    }
}

impl MouseButton {
    fn wire_tag(&self) -> u8 {
        match self {
            Self::None => 0,
            Self::Left => 1,
            Self::Middle => 2,
            Self::Right => 3,
        }
    }

    fn from_wire_tag(tag: u8) -> Option<Self> {
        match tag {
            0 => Some(Self::None),
            1 => Some(Self::Left),
            2 => Some(Self::Middle),
            3 => Some(Self::Right),
            _ => None,
        }
    }
}

const DEFAULT_CAPTURE_BYTES: usize = 256 * 1024;
const MAX_CAPTURE_BYTES: usize = 1024 * 1024;

#[inline(never)]
pub fn parse_cli(args: &[String]) -> Result<CliRequest, String> {
    let mut cursor = Cursor::new(args);
    cursor.require("cli")?;
    let control = cursor.required_value("--control")?.to_owned();
    let verb = cursor.next().ok_or_else(usage)?;

    let command = match verb {
        "list-tabs" => {
            cursor.finish()?;
            CliCommand::ListTabs
        }
        "perf-stats" => {
            cursor.finish()?;
            CliCommand::PerfStats
        }
        "reset-perf-stats" => {
            cursor.finish()?;
            CliCommand::ResetPerfStats
        }
        "new-tab" => {
            let parent = cursor.optional_tab("--parent")?;
            cursor.finish()?;
            CliCommand::NewTab { parent }
        }
        "select-tab" => {
            let target = cursor.required_tab("--target")?;
            cursor.finish()?;
            CliCommand::SelectTab { target }
        }
        "close-tab" => {
            let target = cursor.required_tab("--target")?;
            cursor.finish()?;
            CliCommand::CloseTab { target }
        }
        "capture-pane" => {
            let target = cursor.optional_target()?;
            let max_bytes = cursor
                .optional_usize("--max-bytes")?
                .unwrap_or(DEFAULT_CAPTURE_BYTES);
            if max_bytes == 0 || max_bytes > MAX_CAPTURE_BYTES {
                return Err(format!(
                    "--max-bytes must be between 1 and {MAX_CAPTURE_BYTES}"
                ));
            }
            cursor.finish()?;
            CliCommand::CapturePane { target, max_bytes }
        }
        "screenshot-pane" => {
            let target = cursor.optional_target()?;
            let output = cursor.required_value("--output")?.to_owned();
            cursor.finish()?;
            CliCommand::ScreenshotPane { target, output }
        }
        "send-text" => {
            let target = cursor.optional_target()?;
            let text = cursor
                .next()
                .ok_or_else(|| "send-text requires TEXT".to_owned())?
                .to_owned();
            cursor.finish()?;
            CliCommand::SendText { target, text }
        }
        "send-paste" => {
            let target = cursor.optional_target()?;
            let text = cursor
                .next()
                .ok_or_else(|| "send-paste requires TEXT".to_owned())?
                .to_owned();
            cursor.finish()?;
            CliCommand::SendPaste { target, text }
        }
        "send-keys" => {
            let target = cursor.optional_target()?;
            let mut keys = Vec::new();
            while let Some(key) = cursor.next() {
                keys.push(key.to_owned());
            }
            if keys.is_empty() {
                return Err("send-keys requires at least one KEY".to_owned());
            }
            CliCommand::SendKeys { target, keys }
        }
        "send-mouse" => {
            let target = cursor.optional_target()?;
            let action = parse_mouse_action(cursor.required_value("--action")?)?;
            let button = parse_mouse_button(cursor.required_value("--button")?)?;
            let column = cursor.required_u16("--column")?;
            let row = cursor.required_u16("--row")?;
            cursor.finish()?;
            if (action == MouseAction::Move) != (button == MouseButton::None) {
                return Err(
                    "send-mouse move requires --button none; button actions require a button"
                        .to_owned(),
                );
            }
            CliCommand::SendMouse {
                target,
                action,
                button,
                column,
                row,
            }
        }
        "send-wheel" => {
            let target = cursor.optional_target()?;
            let column = cursor.required_u16("--column")?;
            let row = cursor.required_u16("--row")?;
            let notches = cursor.required_i16("--notches")?;
            let ctrl = cursor.optional_flag("--ctrl");
            cursor.finish()?;
            if notches == 0 {
                return Err("--notches must not be zero".to_owned());
            }
            CliCommand::SendWheel {
                target,
                column,
                row,
                notches,
                ctrl,
            }
        }
        "wait-text" => {
            let target = cursor.optional_target()?;
            let timeout_ms = cursor.optional_u64("--timeout-ms")?.unwrap_or(10_000);
            if timeout_ms == 0 || timeout_ms > 120_000 {
                return Err("--timeout-ms must be between 1 and 120000".to_owned());
            }
            let text = cursor
                .next()
                .ok_or_else(|| "wait-text requires TEXT".to_owned())?
                .to_owned();
            cursor.finish()?;
            CliCommand::WaitText {
                target,
                text,
                timeout_ms,
            }
        }
        _ => {
            return Err(format!(
                "unknown agenterm-con cli command {verb:?}\n{}",
                usage()
            ));
        }
    };

    Ok(CliRequest { control, command })
}

pub fn usage() -> String {
    "usage: agenterm-con cli --control ENDPOINT <list-tabs|perf-stats|reset-perf-stats|new-tab|select-tab|close-tab|capture-pane|screenshot-pane|send-text|send-paste|send-keys|send-mouse|send-wheel|wait-text> ...".to_owned()
}

#[inline(never)]
fn parse_u64_decimal(value: &str) -> Option<u64> {
    let bytes = value.as_bytes();
    let mut index = 0;
    if bytes.first().copied() == Some(b'+') {
        index = 1;
    }
    if index == bytes.len() {
        return None;
    }

    let mut parsed = 0u64;
    while index < bytes.len() {
        let byte = bytes[index];
        if !byte.is_ascii_digit() {
            return None;
        }
        parsed = parsed.checked_mul(10)?;
        parsed = parsed.checked_add(u64::from(byte - b'0'))?;
        index += 1;
    }
    Some(parsed)
}

struct Cursor<'a> {
    args: &'a [String],
    position: usize,
}

impl<'a> Cursor<'a> {
    fn new(args: &'a [String]) -> Self {
        Self { args, position: 0 }
    }

    fn next(&mut self) -> Option<&'a str> {
        let value = self.args.get(self.position)?.as_str();
        self.position += 1;
        Some(value)
    }

    fn require(&mut self, expected: &str) -> Result<(), String> {
        match self.next() {
            Some(value) if value == expected => Ok(()),
            _ => Err(usage()),
        }
    }

    fn required_value(&mut self, flag: &str) -> Result<&'a str, String> {
        match self.next() {
            Some(value) if value == flag => self
                .next()
                .filter(|value| !value.starts_with("--"))
                .ok_or_else(|| format!("{flag} requires a value")),
            Some(value) => Err(format!("expected {flag}, got {value:?}")),
            None => Err(format!("{flag} requires a value")),
        }
    }

    fn optional_target(&mut self) -> Result<Option<TabId>, String> {
        self.optional_tab("--target")
    }

    fn optional_tab(&mut self, flag: &str) -> Result<Option<TabId>, String> {
        if self
            .args
            .get(self.position)
            .is_none_or(|value| value != flag)
        {
            return Ok(None);
        }
        self.position += 1;
        let value = self
            .next()
            .ok_or_else(|| format!("{flag} requires @TAB_ID"))?;
        let digits = match value.strip_prefix('@') {
            Some(digits) => digits,
            None => return Err(format!("invalid tab target {value:?}; expected @TAB_ID")),
        };
        let id = match parse_u64_decimal(digits) {
            Some(id) if id != 0 => id,
            _ => return Err(format!("invalid tab target {value:?}; expected @TAB_ID")),
        };
        Ok(Some(TabId::new(id)))
    }

    fn required_tab(&mut self, flag: &str) -> Result<TabId, String> {
        self.optional_tab(flag)?
            .ok_or_else(|| format!("{flag} requires @TAB_ID"))
    }

    fn optional_usize(&mut self, flag: &str) -> Result<Option<usize>, String> {
        if self
            .args
            .get(self.position)
            .is_none_or(|value| value != flag)
        {
            return Ok(None);
        }
        self.position += 1;
        let value = self
            .next()
            .ok_or_else(|| format!("{flag} requires a value"))?;
        let value = match parse_u64_decimal(value) {
            Some(value) => value,
            None => return Err(format!("{flag} must be an unsigned integer")),
        };
        match usize::try_from(value) {
            Ok(value) => Ok(Some(value)),
            Err(_) => Err(format!("{flag} must be an unsigned integer")),
        }
    }

    fn optional_u64(&mut self, flag: &str) -> Result<Option<u64>, String> {
        if self
            .args
            .get(self.position)
            .is_none_or(|value| value != flag)
        {
            return Ok(None);
        }
        self.position += 1;
        let value = self
            .next()
            .ok_or_else(|| format!("{flag} requires a value"))?;
        match parse_u64_decimal(value) {
            Some(value) => Ok(Some(value)),
            None => Err(format!("{flag} must be an unsigned integer")),
        }
    }

    fn required_u16(&mut self, flag: &str) -> Result<u16, String> {
        let value = self.required_value(flag)?;
        let value = match parse_u64_decimal(value) {
            Some(value) => value,
            None => return Err(format!("{flag} must be an unsigned 16-bit integer")),
        };
        u16::try_from(value).map_err(|_| format!("{flag} must be an unsigned 16-bit integer"))
    }

    fn required_i16(&mut self, flag: &str) -> Result<i16, String> {
        self.required_value(flag)?
            .parse::<i16>()
            .map_err(|_| format!("{flag} must be a signed 16-bit integer"))
    }

    fn optional_flag(&mut self, flag: &str) -> bool {
        if self
            .args
            .get(self.position)
            .is_some_and(|value| value == flag)
        {
            self.position += 1;
            true
        } else {
            false
        }
    }

    fn finish(&self) -> Result<(), String> {
        if self.position == self.args.len() {
            Ok(())
        } else {
            Err(format!(
                "unexpected argument {:?}",
                self.args[self.position]
            ))
        }
    }
}

fn parse_mouse_action(value: &str) -> Result<MouseAction, String> {
    match value {
        "press" => Ok(MouseAction::Press),
        "release" => Ok(MouseAction::Release),
        "move" => Ok(MouseAction::Move),
        "click" => Ok(MouseAction::Click),
        _ => Err(format!(
            "invalid mouse action {value:?}; use press, release, move, or click"
        )),
    }
}

fn parse_mouse_button(value: &str) -> Result<MouseButton, String> {
    match value {
        "none" => Ok(MouseButton::None),
        "left" => Ok(MouseButton::Left),
        "middle" => Ok(MouseButton::Middle),
        "right" => Ok(MouseButton::Right),
        _ => Err(format!(
            "invalid mouse button {value:?}; use none, left, middle, or right"
        )),
    }
}

pub type Reply = Result<JsonValue, String>;
pub struct ReplySender(Arc<ReplySlot>);

struct ReplyReceiver(Arc<ReplySlot>);

struct ReplySlot {
    value: std::sync::Mutex<Option<Reply>>,
    ready: std::sync::Condvar,
    sender_alive: AtomicBool,
}

impl Default for ReplySlot {
    fn default() -> Self {
        Self {
            value: std::sync::Mutex::new(None),
            ready: std::sync::Condvar::new(),
            sender_alive: AtomicBool::new(true),
        }
    }
}

impl ReplySender {
    pub fn send(&self, value: Reply) -> Result<(), Reply> {
        let mut slot = self
            .0
            .value
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if slot.is_some() {
            return Err(value);
        }
        *slot = Some(value);
        self.0.ready.notify_one();
        Ok(())
    }
}

impl ReplyReceiver {
    fn recv_timeout(&self, timeout: Duration) -> Result<Reply, ()> {
        let slot = self
            .0
            .value
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let (mut slot, _) = self
            .0
            .ready
            .wait_timeout_while(slot, timeout, |slot| {
                slot.is_none() && self.0.sender_alive.load(Ordering::Acquire)
            })
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        slot.take().ok_or(())
    }
}

impl Drop for ReplySender {
    fn drop(&mut self) {
        self.0.sender_alive.store(false, Ordering::Release);
        self.0.ready.notify_one();
    }
}

fn reply_channel() -> (ReplySender, ReplyReceiver) {
    let slot = Arc::new(ReplySlot::default());
    (ReplySender(Arc::clone(&slot)), ReplyReceiver(slot))
}

struct RequestQueue {
    items: std::sync::Mutex<std::collections::VecDeque<IncomingRequest>>,
    alive: Arc<AtomicBool>,
}

impl RequestQueue {
    fn new(alive: Arc<AtomicBool>) -> Self {
        Self {
            items: std::sync::Mutex::new(std::collections::VecDeque::new()),
            alive,
        }
    }

    fn push(&self, request: IncomingRequest) -> Result<(), IncomingRequest> {
        let mut items = self
            .items
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if !self.alive.load(Ordering::Acquire) {
            return Err(request);
        }
        items.push_back(request);
        Ok(())
    }

    fn pop(&self) -> Option<IncomingRequest> {
        self.items
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .pop_front()
    }

    fn close(&self) {
        let mut items = self
            .items
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        self.alive.store(false, Ordering::Release);
        items.clear();
    }
}

pub struct IncomingRequest {
    pub command: CliCommand,
    pub reply: ReplySender,
}

pub struct ControlServer {
    requests: Arc<RequestQueue>,
}

impl ControlServer {
    pub fn bind(endpoint: &str, wake: impl Fn() + Send + Sync + 'static) -> Result<Self, String> {
        let endpoint = parse_native_endpoint(endpoint)?;
        let mut listener = NativeListener::bind(&endpoint).map_err(|error| error.to_string())?;
        let alive = Arc::new(AtomicBool::new(true));
        let requests = Arc::new(RequestQueue::new(Arc::clone(&alive)));
        let request_tx = Arc::clone(&requests);
        let worker_alive = Arc::clone(&alive);
        let wake = Arc::new(wake);
        agenterm_platform::threading::spawn_named_detached(
            "agenterm-con-control",
            Box::new(move || {
                while worker_alive.load(Ordering::Acquire) {
                    let stream = match listener.accept(ACCEPT_POLL) {
                        Ok(stream) => stream,
                        Err(error) if error.code == IpcTransportErrorCode::AcceptTimeout => {
                            continue;
                        }
                        Err(_) => break,
                    };
                    let request_tx = request_tx.clone();
                    let wake = Arc::clone(&wake);
                    let _ = agenterm_platform::threading::spawn_named_detached(
                        "agenterm-con-control-request",
                        Box::new(move || serve_one(stream, request_tx, wake)),
                    );
                }
            }),
        )
        .map_err(|error| format!("control listener thread: {error}"))?;
        Ok(Self { requests })
    }

    pub fn try_recv(&self) -> Option<IncomingRequest> {
        self.requests.pop()
    }
}

impl Drop for ControlServer {
    fn drop(&mut self) {
        self.requests.close();
    }
}

#[inline(never)]
pub fn run_cli(args: &[String]) -> Result<String, String> {
    let request = parse_cli(args)?;
    let endpoint = parse_native_endpoint(&request.control)?;
    let mut stream = NativeStream::connect(&endpoint, CONNECT_TIMEOUT)
        .map_err(|error| format!("connect {}: {error}", request.control))?;
    stream
        .set_io_timeout(GUI_RESPONSE_TIMEOUT)
        .map_err(|error| error.to_string())?;
    let payload = encode_request(request.command)?;
    write_frame(&mut stream, &payload, REQUEST_MAX_BYTES)?;
    decode_response(&read_frame(&mut stream, RESPONSE_MAX_BYTES)?)
}

fn serve_one(
    mut stream: NativeStream,
    request_tx: Arc<RequestQueue>,
    wake: Arc<dyn Fn() + Send + Sync>,
) {
    let _ = stream.set_io_timeout(GUI_RESPONSE_TIMEOUT);
    let response = read_wire_request(&mut stream).and_then(|command| {
        let (reply, response_rx) = reply_channel();
        request_tx
            .push(IncomingRequest { command, reply })
            .map_err(|_| "terminal window is closing".to_owned())?;
        wake();
        response_rx
            .recv_timeout(GUI_RESPONSE_TIMEOUT)
            .map_err(|_| "terminal GUI did not respond before timeout".to_owned())?
    });
    let payload = encode_response(response);
    let _ = write_frame(&mut stream, &payload, RESPONSE_MAX_BYTES);
}

fn read_wire_request(stream: &mut NativeStream) -> Result<CliCommand, String> {
    decode_request(&read_frame(stream, REQUEST_MAX_BYTES)?)
}

fn parse_native_endpoint(value: &str) -> Result<IpcEndpoint, String> {
    IpcEndpoint::from_native_address(value)
        .map_err(|_| "agenterm-con control requires pipe:<name> or unix:<absolute-path>".to_owned())
}

#[cfg(test)]
mod native_endpoint_tests {
    use super::*;

    #[test]
    fn con_control_accepts_native_ipc_and_rejects_tcp() {
        assert_eq!(
            parse_native_endpoint("pipe:agenterm-test"),
            Ok(IpcEndpoint::NamedPipe("agenterm-test".to_owned()))
        );
        assert!(parse_native_endpoint("tcp:127.0.0.1:42").is_err());
    }
}

const MAX_WIRE_KEYS: usize = 16_384;

fn write_frame(stream: &mut NativeStream, payload: &[u8], max_bytes: usize) -> Result<(), String> {
    if payload.is_empty() || payload.len() > max_bytes {
        return Err("control frame payload is empty or oversized".to_owned());
    }
    let length =
        u32::try_from(payload.len()).map_err(|_| "control frame payload exceeds u32".to_owned())?;
    let [m0, m1, m2, m3] = WIRE_MAGIC;
    let [l0, l1, l2, l3] = length.to_le_bytes();
    let header = [m0, m1, m2, m3, l0, l1, l2, l3];
    stream
        .write_all(&header)
        .map_err(|error| error.to_string())?;
    stream
        .write_all(payload)
        .map_err(|error| error.to_string())?;
    stream.flush().map_err(|error| error.to_string())
}

fn read_frame(stream: &mut NativeStream, max_bytes: usize) -> Result<Vec<u8>, String> {
    let mut header = [0u8; 8];
    stream
        .read_exact(&mut header)
        .map_err(|error| error.to_string())?;
    if [header[0], header[1], header[2], header[3]] != WIRE_MAGIC {
        return Err("unsupported control frame version".to_owned());
    }
    let length = u32::from_le_bytes([header[4], header[5], header[6], header[7]]) as usize;
    if length == 0 || length > max_bytes {
        return Err("control frame payload is empty or oversized".to_owned());
    }
    let mut payload = vec![0u8; length];
    stream
        .read_exact(&mut payload)
        .map_err(|error| error.to_string())?;
    Ok(payload)
}

fn encode_request(command: CliCommand) -> Result<Vec<u8>, String> {
    let mut wire = WireWriter::new();
    match command {
        CliCommand::ListTabs => wire.byte(0),
        CliCommand::PerfStats => wire.byte(1),
        CliCommand::ResetPerfStats => wire.byte(2),
        CliCommand::NewTab { parent } => {
            wire.byte(3);
            wire.optional_tab(parent);
        }
        CliCommand::SelectTab { target } => {
            wire.byte(4);
            wire.tab(target);
        }
        CliCommand::CloseTab { target } => {
            wire.byte(5);
            wire.tab(target);
        }
        CliCommand::CapturePane { target, max_bytes } => {
            wire.byte(6);
            wire.optional_tab(target);
            wire.u64(max_bytes as u64);
        }
        CliCommand::ScreenshotPane { target, output } => {
            wire.byte(7);
            wire.optional_tab(target);
            wire.string(&output)?;
        }
        CliCommand::SendText { target, text } => {
            wire.byte(8);
            wire.optional_tab(target);
            wire.string(&text)?;
        }
        CliCommand::SendPaste { target, text } => {
            wire.byte(13);
            wire.optional_tab(target);
            wire.string(&text)?;
        }
        CliCommand::SendKeys { target, keys } => {
            wire.byte(9);
            wire.optional_tab(target);
            let count =
                u32::try_from(keys.len()).map_err(|_| "too many control keys".to_owned())?;
            wire.u32(count);
            for key in keys {
                wire.string(&key)?;
            }
        }
        CliCommand::SendMouse {
            target,
            action,
            button,
            column,
            row,
        } => {
            wire.byte(10);
            wire.optional_tab(target);
            wire.byte(action.wire_tag());
            wire.byte(button.wire_tag());
            wire.u16(column);
            wire.u16(row);
        }
        CliCommand::SendWheel {
            target,
            column,
            row,
            notches,
            ctrl,
        } => {
            wire.byte(11);
            wire.optional_tab(target);
            wire.u16(column);
            wire.u16(row);
            wire.i16(notches);
            wire.boolean(ctrl);
        }
        CliCommand::WaitText {
            target,
            text,
            timeout_ms,
        } => {
            wire.byte(12);
            wire.optional_tab(target);
            wire.string(&text)?;
            wire.u64(timeout_ms);
        }
    }
    if wire.bytes.len() > REQUEST_MAX_BYTES {
        return Err("control request is oversized".to_owned());
    }
    Ok(wire.bytes)
}

fn decode_request(bytes: &[u8]) -> Result<CliCommand, String> {
    if bytes.is_empty() || bytes.len() > REQUEST_MAX_BYTES {
        return Err("control request is empty or oversized".to_owned());
    }
    let mut wire = WireReader::new(bytes);
    let command = match wire.byte()? {
        0 => CliCommand::ListTabs,
        1 => CliCommand::PerfStats,
        2 => CliCommand::ResetPerfStats,
        3 => CliCommand::NewTab {
            parent: wire.optional_tab()?,
        },
        4 => CliCommand::SelectTab {
            target: wire.tab()?,
        },
        5 => CliCommand::CloseTab {
            target: wire.tab()?,
        },
        6 => {
            let target = wire.optional_tab()?;
            let max_bytes = usize::try_from(wire.u64()?)
                .map_err(|_| "capture size is outside usize".to_owned())?;
            if max_bytes == 0 || max_bytes > MAX_CAPTURE_BYTES {
                return Err("capture size is outside its allowed range".to_owned());
            }
            CliCommand::CapturePane { target, max_bytes }
        }
        7 => CliCommand::ScreenshotPane {
            target: wire.optional_tab()?,
            output: wire.string()?,
        },
        8 => CliCommand::SendText {
            target: wire.optional_tab()?,
            text: wire.string()?,
        },
        9 => {
            let target = wire.optional_tab()?;
            let count = wire.u32()? as usize;
            if count == 0 || count > MAX_WIRE_KEYS || count > wire.remaining() / 4 {
                return Err("control key count is invalid".to_owned());
            }
            let mut keys = Vec::with_capacity(count);
            for _ in 0..count {
                keys.push(wire.string()?);
            }
            CliCommand::SendKeys { target, keys }
        }
        10 => {
            let target = wire.optional_tab()?;
            let action = match MouseAction::from_wire_tag(wire.byte()?) {
                Some(action) => action,
                None => return Err("invalid control mouse action".to_owned()),
            };
            let button = match MouseButton::from_wire_tag(wire.byte()?) {
                Some(button) => button,
                None => return Err("invalid control mouse button".to_owned()),
            };
            if (action == MouseAction::Move) != (button == MouseButton::None) {
                return Err("invalid control mouse action/button pair".to_owned());
            }
            CliCommand::SendMouse {
                target,
                action,
                button,
                column: wire.u16()?,
                row: wire.u16()?,
            }
        }
        11 => {
            let target = wire.optional_tab()?;
            let column = wire.u16()?;
            let row = wire.u16()?;
            let notches = wire.i16()?;
            if notches == 0 {
                return Err("control wheel notches must not be zero".to_owned());
            }
            CliCommand::SendWheel {
                target,
                column,
                row,
                notches,
                ctrl: wire.boolean()?,
            }
        }
        12 => {
            let target = wire.optional_tab()?;
            let text = wire.string()?;
            let timeout_ms = wire.u64()?;
            if timeout_ms == 0 || timeout_ms > 120_000 {
                return Err("control wait timeout is outside its allowed range".to_owned());
            }
            CliCommand::WaitText {
                target,
                text,
                timeout_ms,
            }
        }
        13 => CliCommand::SendPaste {
            target: wire.optional_tab()?,
            text: wire.string()?,
        },
        _ => return Err("unknown control command opcode".to_owned()),
    };
    wire.finish()?;
    Ok(command)
}

fn encode_response(response: Reply) -> Vec<u8> {
    let mut payload = Vec::new();
    match response {
        Err(error) => {
            payload.push(0);
            payload.extend_from_slice(error.as_bytes());
        }
        Ok(JsonValue::Null) => payload.push(1),
        Ok(JsonValue::String(text)) => {
            payload.push(2);
            payload.extend_from_slice(text.as_bytes());
        }
        Ok(value) => {
            payload.push(3);
            payload.extend_from_slice(&json::to_vec_pretty(&value));
        }
    }
    payload
}

fn decode_response(bytes: &[u8]) -> Result<String, String> {
    let (&tag, payload) = bytes
        .split_first()
        .ok_or_else(|| "empty control response".to_owned())?;
    let text = std::str::from_utf8(payload)
        .map_err(|_| "control response is not valid UTF-8".to_owned())?;
    match tag {
        0 => Err(text.to_owned()),
        1 if payload.is_empty() => Ok(String::new()),
        2 | 3 => Ok(text.to_owned()),
        1 => Err("null control response has trailing bytes".to_owned()),
        _ => Err("unknown control response tag".to_owned()),
    }
}

struct WireWriter {
    bytes: Vec<u8>,
}

impl WireWriter {
    fn new() -> Self {
        Self { bytes: Vec::new() }
    }

    fn byte(&mut self, value: u8) {
        self.bytes.push(value);
    }

    fn boolean(&mut self, value: bool) {
        self.byte(u8::from(value));
    }

    fn u16(&mut self, value: u16) {
        self.bytes.extend_from_slice(&value.to_le_bytes());
    }

    fn i16(&mut self, value: i16) {
        self.bytes.extend_from_slice(&value.to_le_bytes());
    }

    fn u32(&mut self, value: u32) {
        self.bytes.extend_from_slice(&value.to_le_bytes());
    }

    fn u64(&mut self, value: u64) {
        self.bytes.extend_from_slice(&value.to_le_bytes());
    }

    fn tab(&mut self, value: TabId) {
        self.u64(value.get());
    }

    fn optional_tab(&mut self, value: Option<TabId>) {
        self.boolean(value.is_some());
        if let Some(value) = value {
            self.tab(value);
        }
    }

    fn string(&mut self, value: &str) -> Result<(), String> {
        let length =
            u32::try_from(value.len()).map_err(|_| "control string exceeds u32".to_owned())?;
        self.u32(length);
        self.bytes.extend_from_slice(value.as_bytes());
        Ok(())
    }
}

struct WireReader<'a> {
    bytes: &'a [u8],
    position: usize,
}

impl<'a> WireReader<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, position: 0 }
    }

    fn remaining(&self) -> usize {
        self.bytes.len() - self.position
    }

    fn take(&mut self, length: usize) -> Result<&'a [u8], String> {
        let end = self
            .position
            .checked_add(length)
            .filter(|end| *end <= self.bytes.len())
            .ok_or_else(|| "truncated control request".to_owned())?;
        let value = self
            .bytes
            .get(self.position..end)
            .ok_or_else(|| "truncated control request".to_owned())?;
        self.position = end;
        Ok(value)
    }

    fn byte(&mut self) -> Result<u8, String> {
        Ok(self.take(1)?[0])
    }

    fn boolean(&mut self) -> Result<bool, String> {
        match self.byte()? {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err("invalid control boolean tag".to_owned()),
        }
    }

    fn u16(&mut self) -> Result<u16, String> {
        Ok(u16::from_le_bytes(
            self.take(2)?.try_into().expect("two-byte integer"),
        ))
    }

    fn i16(&mut self) -> Result<i16, String> {
        Ok(i16::from_le_bytes(
            self.take(2)?.try_into().expect("two-byte integer"),
        ))
    }

    fn u32(&mut self) -> Result<u32, String> {
        Ok(u32::from_le_bytes(
            self.take(4)?.try_into().expect("four-byte integer"),
        ))
    }

    fn u64(&mut self) -> Result<u64, String> {
        Ok(u64::from_le_bytes(
            self.take(8)?.try_into().expect("eight-byte integer"),
        ))
    }

    fn tab(&mut self) -> Result<TabId, String> {
        let value = self.u64()?;
        if value == 0 {
            return Err("control tab id must not be zero".to_owned());
        }
        Ok(TabId::new(value))
    }

    fn optional_tab(&mut self) -> Result<Option<TabId>, String> {
        if self.boolean()? {
            self.tab().map(Some)
        } else {
            Ok(None)
        }
    }

    fn string(&mut self) -> Result<String, String> {
        let length = self.u32()? as usize;
        let bytes = self.take(length)?;
        std::str::from_utf8(bytes)
            .map(str::to_owned)
            .map_err(|_| "control string is not valid UTF-8".to_owned())
    }

    fn finish(self) -> Result<(), String> {
        if self.position == self.bytes.len() {
            Ok(())
        } else {
            Err("trailing bytes in control request".to_owned())
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_owned()).collect()
    }

    #[test]
    fn capture_pane_uses_stable_target_and_bounded_output() {
        assert_eq!(
            parse_cli(&args(&[
                "cli",
                "--control",
                "local-control",
                "capture-pane",
                "--target",
                "@7",
                "--max-bytes",
                "4096",
            ])),
            Ok(CliRequest {
                control: "local-control".to_owned(),
                command: CliCommand::CapturePane {
                    target: Some(TabId::new(7)),
                    max_bytes: 4096,
                },
            })
        );
    }

    #[test]
    fn send_mouse_rejects_ambiguous_move_button_state() {
        let error = parse_cli(&args(&[
            "cli",
            "--control",
            "local-control",
            "send-mouse",
            "--action",
            "move",
            "--button",
            "left",
            "--column",
            "3",
            "--row",
            "4",
        ]))
        .unwrap_err();
        assert!(error.contains("move requires --button none"));
    }

    #[test]
    fn script_is_not_a_cli_command() {
        let error = parse_cli(&args(&["cli", "--control", "local-control", "script"])).unwrap_err();
        assert!(error.contains("unknown agenterm-con cli command"));
    }

    #[test]
    fn lifecycle_and_wheel_commands_keep_stable_tab_ids() {
        assert_eq!(
            parse_cli(&args(&[
                "cli",
                "--control",
                "pipe:test",
                "new-tab",
                "--parent",
                "@9",
            ])),
            Ok(CliRequest {
                control: "pipe:test".to_owned(),
                command: CliCommand::NewTab {
                    parent: Some(TabId::new(9))
                },
            })
        );
        assert_eq!(
            parse_cli(&args(&[
                "cli",
                "--control",
                "pipe:test",
                "send-wheel",
                "--target",
                "@9",
                "--column",
                "3",
                "--row",
                "4",
                "--notches",
                "-2",
                "--ctrl",
            ])),
            Ok(CliRequest {
                control: "pipe:test".to_owned(),
                command: CliCommand::SendWheel {
                    target: Some(TabId::new(9)),
                    column: 3,
                    row: 4,
                    notches: -2,
                    ctrl: true,
                },
            })
        );
    }

    #[test]
    fn parse_u64_decimal_covers_unsigned_cli_edges() {
        assert_eq!(parse_u64_decimal("0"), Some(0));
        assert_eq!(parse_u64_decimal("00042"), Some(42));
        assert_eq!(parse_u64_decimal("+1"), Some(1));
        assert_eq!(parse_u64_decimal("18446744073709551615"), Some(u64::MAX));
        assert_eq!(parse_u64_decimal("18446744073709551616"), None);
        assert_eq!(parse_u64_decimal("-0"), None);
        assert_eq!(parse_u64_decimal(""), None);
        assert_eq!(parse_u64_decimal("+"), None);
        assert_eq!(parse_u64_decimal("12x"), None);
        assert_eq!(parse_u64_decimal("\u{FF11}"), None);
    }

    #[test]
    fn numeric_cursor_preserves_u16_usize_and_target_bounds() {
        let u16_max_args = vec!["--row".to_owned(), u16::MAX.to_string()];
        let mut u16_max_cursor = Cursor::new(&u16_max_args);
        assert_eq!(u16_max_cursor.required_u16("--row"), Ok(u16::MAX));

        let u16_overflow_args = vec!["--row".to_owned(), "65536".to_owned()];
        let mut u16_overflow_cursor = Cursor::new(&u16_overflow_args);
        assert_eq!(
            u16_overflow_cursor.required_u16("--row"),
            Err("--row must be an unsigned 16-bit integer".to_owned())
        );

        let usize_max_args = vec!["--max-bytes".to_owned(), usize::MAX.to_string()];
        let mut usize_max_cursor = Cursor::new(&usize_max_args);
        assert_eq!(
            usize_max_cursor.optional_usize("--max-bytes"),
            Ok(Some(usize::MAX))
        );

        let mut usize_overflow = usize::MAX.to_string();
        usize_overflow.push('0');
        let usize_overflow_args = vec!["--max-bytes".to_owned(), usize_overflow];
        let mut usize_overflow_cursor = Cursor::new(&usize_overflow_args);
        assert_eq!(
            usize_overflow_cursor.optional_usize("--max-bytes"),
            Err("--max-bytes must be an unsigned integer".to_owned())
        );

        let target_args = vec!["--target".to_owned(), "@+1".to_owned()];
        let mut target_cursor = Cursor::new(&target_args);
        assert_eq!(target_cursor.optional_target(), Ok(Some(TabId::new(1))));
    }

    #[test]
    fn every_control_command_survives_wire_round_trip() {
        let commands = [
            CliCommand::ListTabs,
            CliCommand::PerfStats,
            CliCommand::ResetPerfStats,
            CliCommand::NewTab {
                parent: Some(TabId::new(1)),
            },
            CliCommand::SelectTab {
                target: TabId::new(2),
            },
            CliCommand::CloseTab {
                target: TabId::new(3),
            },
            CliCommand::CapturePane {
                target: Some(TabId::new(4)),
                max_bytes: 4096,
            },
            CliCommand::ScreenshotPane {
                target: None,
                output: "pane.png".to_owned(),
            },
            CliCommand::SendText {
                target: None,
                text: "hello".to_owned(),
            },
            CliCommand::SendPaste {
                target: Some(TabId::new(4)),
                text: "pasted\ntext".to_owned(),
            },
            CliCommand::SendKeys {
                target: None,
                keys: vec!["Ctrl+C".to_owned()],
            },
            CliCommand::SendMouse {
                target: None,
                action: MouseAction::Click,
                button: MouseButton::Left,
                column: 1,
                row: 2,
            },
            CliCommand::SendWheel {
                target: None,
                column: 1,
                row: 2,
                notches: 1,
                ctrl: false,
            },
            CliCommand::WaitText {
                target: None,
                text: "ready".to_owned(),
                timeout_ms: 250,
            },
        ];
        for command in commands {
            let bytes = encode_request(command.clone()).expect("wire command encodes");
            let decoded = decode_request(&bytes).expect("wire command decodes");
            assert_eq!(decoded, command);
        }
    }

    #[test]
    fn typed_wire_rejects_trailing_and_invalid_fields() {
        let mut trailing = encode_request(CliCommand::ListTabs).unwrap();
        trailing.push(0);
        assert!(decode_request(&trailing).is_err());

        assert!(decode_request(&[10, 0, 3, 0, 0, 0, 0, 0]).is_err());
        assert!(decode_response(&[9]).is_err());
    }
}
