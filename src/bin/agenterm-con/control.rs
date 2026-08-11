//! Fixed, GUI-lifetime control grammar for `agenterm-con`.
//!
//! This deliberately models only direct terminal interaction.  It is not a
//! scripting language, mux protocol, workspace store, or background service.

use std::io::{BufRead as _, BufReader, Read as _, Write as _};
use std::str::FromStr as _;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};
use std::thread;
use std::time::Duration;

use agenterm_platform::ipc::{IpcEndpoint, IpcTransportErrorCode, NativeListener, NativeStream};

use super::{
    json::{self, JsonValue},
    workspace::TabId,
};

const REQUEST_MAX_BYTES: u64 = 1024 * 1024;
const RESPONSE_MAX_BYTES: u64 = 2 * 1024 * 1024;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(3);
const GUI_RESPONSE_TIMEOUT: Duration = Duration::from_secs(125);
const ACCEPT_POLL: Duration = Duration::from_millis(200);

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CliRequest {
    pub control: String,
    pub command: CliCommand,
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

const DEFAULT_CAPTURE_BYTES: usize = 256 * 1024;
const MAX_CAPTURE_BYTES: usize = 1024 * 1024;

pub fn parse_cli(args: &[String]) -> Result<CliRequest, String> {
    let mut cursor = Cursor::new(args);
    cursor.require("cli")?;
    let control = cursor.required_value("--control")?;
    let verb = cursor.next().ok_or_else(usage)?;

    let command = match verb.as_str() {
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
            let output = cursor.required_value("--output")?;
            cursor.finish()?;
            CliCommand::ScreenshotPane { target, output }
        }
        "send-text" => {
            let target = cursor.optional_target()?;
            let text = cursor
                .next()
                .ok_or_else(|| "send-text requires TEXT".to_owned())?;
            cursor.finish()?;
            CliCommand::SendText { target, text }
        }
        "send-keys" => {
            let target = cursor.optional_target()?;
            let mut keys = Vec::new();
            while let Some(key) = cursor.next() {
                keys.push(key);
            }
            if keys.is_empty() {
                return Err("send-keys requires at least one KEY".to_owned());
            }
            CliCommand::SendKeys { target, keys }
        }
        "send-mouse" => {
            let target = cursor.optional_target()?;
            let action = parse_mouse_action(&cursor.required_value("--action")?)?;
            let button = parse_mouse_button(&cursor.required_value("--button")?)?;
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
                .ok_or_else(|| "wait-text requires TEXT".to_owned())?;
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
    "usage: agenterm-con cli --control ENDPOINT <list-tabs|perf-stats|reset-perf-stats|new-tab|select-tab|close-tab|capture-pane|screenshot-pane|send-text|send-keys|send-mouse|send-wheel|wait-text> ...".to_owned()
}

struct Cursor<'a> {
    args: &'a [String],
    position: usize,
}

impl<'a> Cursor<'a> {
    fn new(args: &'a [String]) -> Self {
        Self { args, position: 0 }
    }

    fn next(&mut self) -> Option<String> {
        let value = self.args.get(self.position)?.clone();
        self.position += 1;
        Some(value)
    }

    fn require(&mut self, expected: &str) -> Result<(), String> {
        match self.next().as_deref() {
            Some(value) if value == expected => Ok(()),
            _ => Err(usage()),
        }
    }

    fn required_value(&mut self, flag: &str) -> Result<String, String> {
        match self.next().as_deref() {
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
        let id = value
            .strip_prefix('@')
            .and_then(|value| value.parse::<u64>().ok())
            .filter(|id| *id != 0)
            .ok_or_else(|| format!("invalid tab target {value:?}; expected @TAB_ID"))?;
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
        value
            .parse::<usize>()
            .map(Some)
            .map_err(|_| format!("{flag} must be an unsigned integer"))
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
        value
            .parse::<u64>()
            .map(Some)
            .map_err(|_| format!("{flag} must be an unsigned integer"))
    }

    fn required_u16(&mut self, flag: &str) -> Result<u16, String> {
        self.required_value(flag)?
            .parse::<u16>()
            .map_err(|_| format!("{flag} must be an unsigned 16-bit integer"))
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
pub type ReplySender = mpsc::Sender<Reply>;

pub struct IncomingRequest {
    pub command: CliCommand,
    pub reply: ReplySender,
}

pub struct ControlServer {
    requests: mpsc::Receiver<IncomingRequest>,
    alive: Arc<AtomicBool>,
}

impl ControlServer {
    pub fn bind(endpoint: &str, wake: impl Fn() + Send + Sync + 'static) -> Result<Self, String> {
        let endpoint = parse_native_endpoint(endpoint)?;
        let mut listener = NativeListener::bind(&endpoint).map_err(|error| error.to_string())?;
        let (request_tx, requests) = mpsc::channel();
        let alive = Arc::new(AtomicBool::new(true));
        let worker_alive = Arc::clone(&alive);
        let wake = Arc::new(wake);
        thread::Builder::new()
            .name("agenterm-con-control".to_owned())
            .spawn(move || {
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
                    let _ = thread::Builder::new()
                        .name("agenterm-con-control-request".to_owned())
                        .spawn(move || serve_one(stream, request_tx, wake));
                }
            })
            .map_err(|error| format!("control listener thread: {error}"))?;
        Ok(Self { requests, alive })
    }

    pub fn try_recv(&self) -> Option<IncomingRequest> {
        self.requests.try_recv().ok()
    }
}

impl Drop for ControlServer {
    fn drop(&mut self) {
        self.alive.store(false, Ordering::Release);
    }
}

pub fn run_cli(args: &[String]) -> Result<String, String> {
    let request = parse_cli(args)?;
    let endpoint = parse_native_endpoint(&request.control)?;
    let wire = WireRequest::from(request.command);
    let mut stream = NativeStream::connect(&endpoint, CONNECT_TIMEOUT)
        .map_err(|error| format!("connect {}: {error}", endpoint))?;
    stream
        .set_io_timeout(GUI_RESPONSE_TIMEOUT)
        .map_err(|error| error.to_string())?;
    let mut bytes = json::to_vec(&wire.into_json());
    bytes.push(b'\n');
    stream
        .write_all(&bytes)
        .map_err(|error| error.to_string())?;
    stream.flush().map_err(|error| error.to_string())?;

    let mut reader = BufReader::new(stream);
    let mut response = Vec::new();
    let read = (&mut reader)
        .take(RESPONSE_MAX_BYTES + 1)
        .read_until(b'\n', &mut response)
        .map_err(|error| error.to_string())?;
    if read == 0 || read as u64 > RESPONSE_MAX_BYTES || !response.ends_with(b"\n") {
        return Err("invalid or oversized control response".to_owned());
    }
    let response = WireResponse::from_json(
        json::parse(&response).map_err(|error| format!("control response: {error}"))?,
    )?;
    if response.ok {
        Ok(match response.result {
            Some(JsonValue::String(text)) => text,
            Some(value) => String::from_utf8(json::to_vec_pretty(&value))
                .map_err(|_| "JSON writer emitted invalid UTF-8".to_owned())?,
            None => String::new(),
        })
    } else {
        Err(response
            .error
            .unwrap_or_else(|| "control request failed".to_owned()))
    }
}

fn serve_one(
    mut stream: NativeStream,
    request_tx: mpsc::Sender<IncomingRequest>,
    wake: Arc<dyn Fn() + Send + Sync>,
) {
    let _ = stream.set_io_timeout(GUI_RESPONSE_TIMEOUT);
    let response = read_wire_request(&mut stream).and_then(|command| {
        let (reply, response_rx) = mpsc::channel();
        request_tx
            .send(IncomingRequest { command, reply })
            .map_err(|_| "terminal window is closing".to_owned())?;
        wake();
        response_rx
            .recv_timeout(GUI_RESPONSE_TIMEOUT)
            .map_err(|_| "terminal GUI did not respond before timeout".to_owned())?
    });
    let wire = match response {
        Ok(result) => WireResponse {
            ok: true,
            result: Some(result),
            error: None,
        },
        Err(error) => WireResponse {
            ok: false,
            result: None,
            error: Some(error),
        },
    };
    let mut bytes = json::to_vec(&wire.into_json());
    bytes.push(b'\n');
    let _ = stream.write_all(&bytes);
    let _ = stream.flush();
}

fn read_wire_request(stream: &mut NativeStream) -> Result<CliCommand, String> {
    let mut reader = BufReader::new(stream);
    let mut bytes = Vec::new();
    let read = (&mut reader)
        .take(REQUEST_MAX_BYTES + 1)
        .read_until(b'\n', &mut bytes)
        .map_err(|error| error.to_string())?;
    if read == 0 || read as u64 > REQUEST_MAX_BYTES || !bytes.ends_with(b"\n") {
        return Err("invalid or oversized control request".to_owned());
    }
    let request = WireRequest::from_json(
        json::parse(&bytes).map_err(|error| format!("control request: {error}"))?,
    )?;
    request.try_into()
}

fn parse_native_endpoint(value: &str) -> Result<IpcEndpoint, String> {
    let endpoint = IpcEndpoint::from_str(value).map_err(|error| error.to_string())?;
    endpoint
        .validate_local()
        .map_err(|error| error.to_string())?;
    if !matches!(
        endpoint,
        IpcEndpoint::NamedPipe(_) | IpcEndpoint::UnixSocket(_)
    ) {
        return Err("agenterm-con control requires pipe:<name> or unix:<absolute-path>".to_owned());
    }
    Ok(endpoint)
}

struct WireRequest {
    command: String,
    target: Option<u64>,
    parent: Option<u64>,
    text: Option<String>,
    keys: Option<Vec<String>>,
    output: Option<String>,
    max_bytes: Option<usize>,
    action: Option<String>,
    button: Option<String>,
    column: Option<u16>,
    row: Option<u16>,
    timeout_ms: Option<u64>,
    notches: Option<i16>,
    ctrl: Option<bool>,
}

struct WireResponse {
    ok: bool,
    result: Option<JsonValue>,
    error: Option<String>,
}

impl WireRequest {
    fn into_json(self) -> JsonValue {
        json::object([
            ("command", self.command.into()),
            ("target", json::nullable(self.target)),
            ("parent", json::nullable(self.parent)),
            ("text", json::nullable(self.text)),
            (
                "keys",
                json::nullable(
                    self.keys
                        .map(|keys| JsonValue::Array(keys.into_iter().map(Into::into).collect())),
                ),
            ),
            ("output", json::nullable(self.output)),
            ("max_bytes", json::nullable(self.max_bytes)),
            ("action", json::nullable(self.action)),
            ("button", json::nullable(self.button)),
            ("column", json::nullable(self.column)),
            ("row", json::nullable(self.row)),
            ("timeout_ms", json::nullable(self.timeout_ms)),
            ("notches", json::nullable(self.notches)),
            ("ctrl", json::nullable(self.ctrl)),
        ])
    }

    fn from_json(value: JsonValue) -> Result<Self, String> {
        let mut fields = value.into_object("control request")?;
        let request = Self {
            command: json::take_string(&mut fields, "command")?
                .ok_or_else(|| "missing command".to_owned())?,
            target: json::take_u64(&mut fields, "target")?,
            parent: json::take_u64(&mut fields, "parent")?,
            text: json::take_string(&mut fields, "text")?,
            keys: json::take_string_array(&mut fields, "keys")?,
            output: json::take_string(&mut fields, "output")?,
            max_bytes: json::take_usize(&mut fields, "max_bytes")?,
            action: json::take_string(&mut fields, "action")?,
            button: json::take_string(&mut fields, "button")?,
            column: json::take_u16(&mut fields, "column")?,
            row: json::take_u16(&mut fields, "row")?,
            timeout_ms: json::take_u64(&mut fields, "timeout_ms")?,
            notches: json::take_i16(&mut fields, "notches")?,
            ctrl: json::take_bool(&mut fields, "ctrl")?,
        };
        json::reject_unknown(fields, "control request")?;
        Ok(request)
    }
}

impl WireResponse {
    fn into_json(self) -> JsonValue {
        json::object([
            ("ok", self.ok.into()),
            ("result", self.result.unwrap_or(JsonValue::Null)),
            ("error", json::nullable(self.error)),
        ])
    }

    fn from_json(value: JsonValue) -> Result<Self, String> {
        let mut fields = value.into_object("control response")?;
        let ok = json::take_bool(&mut fields, "ok")?
            .ok_or_else(|| "control response missing ok".to_owned())?;
        let result = json::take(&mut fields, "result").filter(|value| !value.is_null());
        let error = json::take_string(&mut fields, "error")?;
        json::reject_unknown(fields, "control response")?;
        Ok(Self { ok, result, error })
    }
}

impl From<CliCommand> for WireRequest {
    fn from(command: CliCommand) -> Self {
        let mut wire = Self {
            command: String::new(),
            target: None,
            parent: None,
            text: None,
            keys: None,
            output: None,
            max_bytes: None,
            action: None,
            button: None,
            column: None,
            row: None,
            timeout_ms: None,
            notches: None,
            ctrl: None,
        };
        match command {
            CliCommand::ListTabs => wire.command = "list-tabs".to_owned(),
            CliCommand::PerfStats => wire.command = "perf-stats".to_owned(),
            CliCommand::ResetPerfStats => wire.command = "reset-perf-stats".to_owned(),
            CliCommand::NewTab { parent } => {
                wire.command = "new-tab".to_owned();
                wire.parent = parent.map(TabId::get);
            }
            CliCommand::SelectTab { target } => {
                wire.command = "select-tab".to_owned();
                wire.target = Some(target.get());
            }
            CliCommand::CloseTab { target } => {
                wire.command = "close-tab".to_owned();
                wire.target = Some(target.get());
            }
            CliCommand::CapturePane { target, max_bytes } => {
                wire.command = "capture-pane".to_owned();
                wire.target = target.map(TabId::get);
                wire.max_bytes = Some(max_bytes);
            }
            CliCommand::ScreenshotPane { target, output } => {
                wire.command = "screenshot-pane".to_owned();
                wire.target = target.map(TabId::get);
                wire.output = Some(output);
            }
            CliCommand::SendText { target, text } => {
                wire.command = "send-text".to_owned();
                wire.target = target.map(TabId::get);
                wire.text = Some(text);
            }
            CliCommand::SendKeys { target, keys } => {
                wire.command = "send-keys".to_owned();
                wire.target = target.map(TabId::get);
                wire.keys = Some(keys);
            }
            CliCommand::SendMouse {
                target,
                action,
                button,
                column,
                row,
            } => {
                wire.command = "send-mouse".to_owned();
                wire.target = target.map(TabId::get);
                wire.action = Some(format!("{action:?}").to_ascii_lowercase());
                wire.button = Some(format!("{button:?}").to_ascii_lowercase());
                wire.column = Some(column);
                wire.row = Some(row);
            }
            CliCommand::SendWheel {
                target,
                column,
                row,
                notches,
                ctrl,
            } => {
                wire.command = "send-wheel".to_owned();
                wire.target = target.map(TabId::get);
                wire.column = Some(column);
                wire.row = Some(row);
                wire.notches = Some(notches);
                wire.ctrl = Some(ctrl);
            }
            CliCommand::WaitText {
                target,
                text,
                timeout_ms,
            } => {
                wire.command = "wait-text".to_owned();
                wire.target = target.map(TabId::get);
                wire.text = Some(text);
                wire.timeout_ms = Some(timeout_ms);
            }
        }
        wire
    }
}

impl TryFrom<WireRequest> for CliCommand {
    type Error = String;

    fn try_from(wire: WireRequest) -> Result<Self, Self::Error> {
        let target = wire.target.map(TabId::new);
        match wire.command.as_str() {
            "list-tabs" => Ok(Self::ListTabs),
            "perf-stats" => Ok(Self::PerfStats),
            "reset-perf-stats" => Ok(Self::ResetPerfStats),
            "new-tab" => Ok(Self::NewTab {
                parent: wire.parent.map(TabId::new),
            }),
            "select-tab" => Ok(Self::SelectTab {
                target: wire.target.map(TabId::new).ok_or("missing target")?,
            }),
            "close-tab" => Ok(Self::CloseTab {
                target: wire.target.map(TabId::new).ok_or("missing target")?,
            }),
            "capture-pane" => Ok(Self::CapturePane {
                target,
                max_bytes: wire.max_bytes.unwrap_or(DEFAULT_CAPTURE_BYTES),
            }),
            "screenshot-pane" => Ok(Self::ScreenshotPane {
                target,
                output: wire.output.ok_or("missing output")?,
            }),
            "send-text" => Ok(Self::SendText {
                target,
                text: wire.text.ok_or("missing text")?,
            }),
            "send-keys" => Ok(Self::SendKeys {
                target,
                keys: wire.keys.ok_or("missing keys")?,
            }),
            "send-mouse" => Ok(Self::SendMouse {
                target,
                action: parse_mouse_action(wire.action.as_deref().ok_or("missing action")?)?,
                button: parse_mouse_button(wire.button.as_deref().ok_or("missing button")?)?,
                column: wire.column.ok_or("missing column")?,
                row: wire.row.ok_or("missing row")?,
            }),
            "send-wheel" => Ok(Self::SendWheel {
                target,
                column: wire.column.ok_or("missing column")?,
                row: wire.row.ok_or("missing row")?,
                notches: wire.notches.ok_or("missing notches")?,
                ctrl: wire.ctrl.unwrap_or(false),
            }),
            "wait-text" => Ok(Self::WaitText {
                target,
                text: wire.text.ok_or("missing text")?,
                timeout_ms: wire.timeout_ms.unwrap_or(10_000),
            }),
            _ => Err("unknown control command".to_owned()),
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
            let bytes = json::to_vec(&WireRequest::from(command.clone()).into_json());
            let decoded = CliCommand::try_from(
                WireRequest::from_json(json::parse(&bytes).expect("wire JSON parses"))
                    .expect("wire schema decodes"),
            )
            .unwrap();
            assert_eq!(decoded, command);
        }
    }
}
