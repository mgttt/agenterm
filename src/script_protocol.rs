use std::{
    collections::HashSet,
    error::Error,
    fmt,
    io::{self, Read, Write},
};

use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const SCRIPT_ENVELOPE_VERSION: u32 = 2;
pub const SCRIPT_API_VERSION: u32 = 1;
pub const SCRIPT_INVOCATION_MAX_BYTES: u64 = 2 * 1024 * 1024;
pub const SCRIPT_FRAME_VERSION: u32 = 1;
pub const SCRIPT_FRAME_MAX_BYTES: u32 = 2 * 1024 * 1024;

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ScriptProfile {
    Pure,
    Observe,
    Local,
}

impl ScriptProfile {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Pure => "pure",
            Self::Observe => "observe",
            Self::Local => "local",
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ScriptOperation {
    Api,
    Check,
    Eval,
    Run,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ScriptBudgets {
    pub source_bytes: usize,
    pub operations: u64,
    pub call_depth: usize,
    pub expression_depth: usize,
    pub collection_items: usize,
    pub string_bytes: usize,
    pub output_bytes: usize,
    pub wall_time_ms: u64,
    #[serde(default = "default_broker_requests")]
    pub broker_requests: usize,
    #[serde(default = "default_broker_return_bytes")]
    pub broker_return_bytes: usize,
    #[serde(default = "default_capture_bytes")]
    pub capture_bytes: usize,
    #[serde(default = "default_event_items")]
    pub event_items: usize,
    #[serde(default = "default_wait_time_ms")]
    pub wait_time_ms: u64,
}

const fn default_broker_requests() -> usize {
    64
}
const fn default_broker_return_bytes() -> usize {
    256 * 1024
}
const fn default_capture_bytes() -> usize {
    64 * 1024
}
const fn default_event_items() -> usize {
    256
}
const fn default_wait_time_ms() -> u64 {
    2_000
}

impl Default for ScriptBudgets {
    fn default() -> Self {
        Self {
            source_bytes: 256 * 1024,
            operations: 1_000_000,
            call_depth: 64,
            expression_depth: 64,
            collection_items: 10_000,
            string_bytes: 256 * 1024,
            output_bytes: 64 * 1024,
            wall_time_ms: 2_000,
            broker_requests: default_broker_requests(),
            broker_return_bytes: default_broker_return_bytes(),
            capture_bytes: default_capture_bytes(),
            event_items: default_event_items(),
            wait_time_ms: default_wait_time_ms(),
        }
    }
}

impl ScriptBudgets {
    pub fn hard_limits() -> Self {
        Self {
            source_bytes: 256 * 1024,
            operations: 10_000_000,
            call_depth: 128,
            expression_depth: 128,
            collection_items: 100_000,
            string_bytes: 1024 * 1024,
            output_bytes: 1024 * 1024,
            wall_time_ms: 10_000,
            broker_requests: 256,
            broker_return_bytes: 1024 * 1024,
            capture_bytes: 256 * 1024,
            event_items: 1024,
            wait_time_ms: 10_000,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ScriptBrokerRequest {
    pub operation: String,
    #[serde(default)]
    pub arguments: Value,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ScriptBrokerResponse {
    pub ok: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<ScriptBrokerError>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ScriptBrokerError {
    pub code: String,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub details: Option<Value>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ScriptInvocation {
    pub envelope_version: u32,
    pub invocation_id: String,
    pub api_version: u32,
    pub operation: ScriptOperation,
    pub profile: ScriptProfile,
    pub source_label: String,
    pub source: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_root: Option<String>,
    pub arguments: Vec<String>,
    pub budgets: ScriptBudgets,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observation: Option<Value>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ScriptFailure {
    pub code: String,
    pub message: String,
    pub category: ScriptFailureCategory,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ScriptFailureCategory {
    Configuration,
    Limit,
    Script,
    Protocol,
    Host,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ScriptExitClass {
    Success,
    Configuration,
    Limit,
    Script,
    Protocol,
    Host,
}

impl ScriptExitClass {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Success => "success",
            Self::Configuration => "configuration",
            Self::Limit => "limit",
            Self::Script => "script",
            Self::Protocol => "protocol",
            Self::Host => "host",
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ScriptResult {
    pub envelope_version: u32,
    pub invocation_id: String,
    pub api_version: u32,
    pub ok: bool,
    pub exit_class: ScriptExitClass,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub operation: Option<ScriptOperation>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile: Option<ScriptProfile>,
    pub stdout: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failure: Option<ScriptFailure>,
    pub duration_ms: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ScriptFrame {
    pub frame_version: u32,
    pub frame_id: String,
    #[serde(flatten)]
    pub payload: ScriptFramePayload,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "kind", content = "payload", rename_all = "snake_case")]
pub enum ScriptFramePayload {
    Invoke(ScriptInvocation),
    Cancel {
        invocation_id: String,
    },
    Result(ScriptResult),
    BrokerRequest {
        invocation_id: String,
        request_id: String,
        request: ScriptBrokerRequest,
    },
    BrokerResponse {
        invocation_id: String,
        request_id: String,
        response: ScriptBrokerResponse,
    },
}

#[derive(Debug)]
pub enum ScriptFrameEncodeError {
    Serialize(serde_json::Error),
    TooLarge { encoded_bytes: usize },
}

impl fmt::Display for ScriptFrameEncodeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Serialize(error) => write!(formatter, "failed to encode script frame: {error}"),
            Self::TooLarge { encoded_bytes } => write!(
                formatter,
                "encoded script frame is {encoded_bytes} bytes, exceeding the {SCRIPT_FRAME_MAX_BYTES} byte limit"
            ),
        }
    }
}

impl Error for ScriptFrameEncodeError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Serialize(error) => Some(error),
            Self::TooLarge { .. } => None,
        }
    }
}

#[derive(Debug)]
pub enum ScriptFrameWriteError {
    Encode(ScriptFrameEncodeError),
    Io(io::Error),
}

impl fmt::Display for ScriptFrameWriteError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Encode(error) => error.fmt(formatter),
            Self::Io(error) => write!(formatter, "failed to write script frame: {error}"),
        }
    }
}

impl Error for ScriptFrameWriteError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Encode(error) => Some(error),
            Self::Io(error) => Some(error),
        }
    }
}

impl From<ScriptFrameEncodeError> for ScriptFrameWriteError {
    fn from(error: ScriptFrameEncodeError) -> Self {
        Self::Encode(error)
    }
}

impl From<io::Error> for ScriptFrameWriteError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

pub fn encode_script_frame(frame: &ScriptFrame) -> Result<Vec<u8>, ScriptFrameEncodeError> {
    let bytes = serde_json::to_vec(frame).map_err(ScriptFrameEncodeError::Serialize)?;
    if bytes.len() > SCRIPT_FRAME_MAX_BYTES as usize {
        return Err(ScriptFrameEncodeError::TooLarge {
            encoded_bytes: bytes.len(),
        });
    }
    Ok(bytes)
}

pub fn write_encoded_script_frame(
    output: &mut impl Write,
    bytes: &[u8],
) -> Result<(), ScriptFrameWriteError> {
    if bytes.len() > SCRIPT_FRAME_MAX_BYTES as usize {
        return Err(ScriptFrameEncodeError::TooLarge {
            encoded_bytes: bytes.len(),
        }
        .into());
    }
    output.write_all(&(bytes.len() as u32).to_be_bytes())?;
    output.write_all(bytes)?;
    output.flush()?;
    Ok(())
}

pub fn write_script_frame(
    output: &mut impl Write,
    frame: &ScriptFrame,
) -> Result<(), ScriptFrameWriteError> {
    let bytes = encode_script_frame(frame)?;
    write_encoded_script_frame(output, &bytes)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScriptFrameRejection {
    pub frame_id: String,
    pub invocation_id: String,
    pub code: &'static str,
    pub message: String,
    pub recoverable: bool,
}

impl ScriptFrameRejection {
    fn transport(code: &'static str, message: String, recoverable: bool) -> Self {
        Self {
            frame_id: "unknown".to_owned(),
            invocation_id: "unknown".to_owned(),
            code,
            message,
            recoverable,
        }
    }

    fn admission(
        frame_id: String,
        invocation_id: impl Into<String>,
        code: &'static str,
        message: impl Into<String>,
    ) -> Self {
        Self {
            frame_id,
            invocation_id: invocation_id.into(),
            code,
            message: message.into(),
            recoverable: true,
        }
    }
}

#[derive(Debug)]
pub enum ScriptFrameRead {
    Eof,
    Frame(Box<ScriptFrame>),
    Rejected(ScriptFrameRejection),
}

pub fn read_script_frame(input: &mut impl Read) -> io::Result<ScriptFrameRead> {
    let mut header = [0_u8; 4];
    match input.read(&mut header[..1])? {
        0 => return Ok(ScriptFrameRead::Eof),
        1 => {}
        _ => unreachable!("one-byte read returned more than one byte"),
    }
    if let Err(error) = input.read_exact(&mut header[1..]) {
        return Ok(ScriptFrameRead::Rejected(ScriptFrameRejection::transport(
            "protocol_truncated_header",
            format!("frame header ended before four bytes: {error}"),
            false,
        )));
    }
    let length = u32::from_be_bytes(header);
    if length > SCRIPT_FRAME_MAX_BYTES {
        let mut limited = input.take(u64::from(length));
        let copied = io::copy(&mut limited, &mut io::sink())?;
        return Ok(ScriptFrameRead::Rejected(ScriptFrameRejection::transport(
            "protocol_frame_too_large",
            format!("frame length {length} exceeds the {SCRIPT_FRAME_MAX_BYTES} byte limit"),
            copied == u64::from(length),
        )));
    }
    let mut bytes = vec![0_u8; length as usize];
    if let Err(error) = input.read_exact(&mut bytes) {
        return Ok(ScriptFrameRead::Rejected(ScriptFrameRejection::transport(
            "protocol_truncated_frame",
            format!("frame payload ended before {length} bytes: {error}"),
            false,
        )));
    }
    match serde_json::from_slice(&bytes) {
        Ok(frame) => Ok(ScriptFrameRead::Frame(Box::new(frame))),
        Err(error) => Ok(ScriptFrameRead::Rejected(ScriptFrameRejection::transport(
            "protocol_malformed_frame",
            error.to_string(),
            true,
        ))),
    }
}

#[derive(Default)]
pub struct ScriptFrameTracker {
    seen_frames: HashSet<String>,
    known_invocations: HashSet<String>,
}

impl ScriptFrameTracker {
    pub fn admit(&mut self, frame: ScriptFrame) -> Result<ScriptFrame, ScriptFrameRejection> {
        let frame_id = frame.frame_id.clone();
        if frame_id.is_empty() || frame_id.len() > 128 {
            return Err(ScriptFrameRejection::admission(
                frame_id,
                "unknown",
                "protocol_invalid_frame_id",
                "frame_id must contain from 1 to 128 bytes",
            ));
        }
        if !self.seen_frames.insert(frame_id.clone()) {
            return Err(ScriptFrameRejection::admission(
                frame_id,
                "unknown",
                "protocol_duplicate_frame",
                "frame_id has already been processed",
            ));
        }
        if frame.frame_version != SCRIPT_FRAME_VERSION {
            return Err(ScriptFrameRejection::admission(
                frame_id,
                "unknown",
                "protocol_unsupported_frame_version",
                format!(
                    "worker supports frame version {SCRIPT_FRAME_VERSION}, requested {}",
                    frame.frame_version
                ),
            ));
        }
        if let ScriptFramePayload::Invoke(invocation) = &frame.payload {
            let invocation_id = invocation.invocation_id.clone();
            if invocation_id.is_empty() || invocation_id.len() > 128 {
                return Err(ScriptFrameRejection::admission(
                    frame_id,
                    invocation_id,
                    "protocol_invalid_invocation_id",
                    "invocation_id must contain from 1 to 128 bytes",
                ));
            }
            if !self.known_invocations.insert(invocation_id.clone()) {
                return Err(ScriptFrameRejection::admission(
                    frame_id,
                    invocation_id,
                    "protocol_duplicate_invocation",
                    "invocation_id has already been processed",
                ));
            }
        }
        Ok(frame)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScriptCancelDisposition {
    Requested,
    TooLate,
    Unknown,
}

impl ScriptCancelDisposition {
    pub fn classify(
        requested_invocation_id: &str,
        active_invocation_id: Option<&str>,
        completed: bool,
    ) -> Self {
        if active_invocation_id == Some(requested_invocation_id) {
            Self::Requested
        } else if completed {
            Self::TooLate
        } else {
            Self::Unknown
        }
    }

    pub const fn rejection(self) -> Option<(&'static str, &'static str)> {
        match self {
            Self::Requested => None,
            Self::TooLate => Some((
                "protocol_cancel_too_late",
                "invocation has already completed",
            )),
            Self::Unknown => Some((
                "protocol_cancel_unknown",
                "no active invocation has this invocation_id",
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::*;

    fn cancel_frame(frame_id: &str, invocation_id: &str) -> ScriptFrame {
        ScriptFrame {
            frame_version: SCRIPT_FRAME_VERSION,
            frame_id: frame_id.to_owned(),
            payload: ScriptFramePayload::Cancel {
                invocation_id: invocation_id.to_owned(),
            },
        }
    }

    #[test]
    fn codec_recovers_after_malformed_and_oversized_frames() {
        let valid = cancel_frame("valid", "invocation");
        let mut input = Vec::new();
        input.extend(1_u32.to_be_bytes());
        input.push(b'{');
        let oversized = SCRIPT_FRAME_MAX_BYTES + 1;
        input.extend(oversized.to_be_bytes());
        input.resize(input.len() + oversized as usize, b'x');
        write_script_frame(&mut input, &valid).unwrap();

        let mut input = Cursor::new(input);
        let malformed = read_script_frame(&mut input).unwrap();
        let oversized = read_script_frame(&mut input).unwrap();
        let recovered = read_script_frame(&mut input).unwrap();

        assert!(matches!(
            malformed,
            ScriptFrameRead::Rejected(ScriptFrameRejection {
                code: "protocol_malformed_frame",
                recoverable: true,
                ..
            })
        ));
        assert!(matches!(
            oversized,
            ScriptFrameRead::Rejected(ScriptFrameRejection {
                code: "protocol_frame_too_large",
                recoverable: true,
                ..
            })
        ));
        assert!(matches!(
            recovered,
            ScriptFrameRead::Frame(frame) if frame.frame_id == "valid"
        ));
    }

    #[test]
    fn tracker_rejects_unsupported_versions_without_poisoning_recovery() {
        let mut tracker = ScriptFrameTracker::default();
        let mut unsupported = cancel_frame("unsupported", "invocation");
        unsupported.frame_version += 1;
        let rejection = tracker.admit(unsupported).unwrap_err();
        assert_eq!(rejection.code, "protocol_unsupported_frame_version");

        assert!(
            tracker
                .admit(cancel_frame("supported", "invocation"))
                .is_ok()
        );
    }

    #[test]
    fn cancellation_classification_covers_active_completed_and_unknown() {
        assert_eq!(
            ScriptCancelDisposition::classify("invocation", Some("invocation"), false),
            ScriptCancelDisposition::Requested
        );
        assert_eq!(
            ScriptCancelDisposition::classify("invocation", None, true),
            ScriptCancelDisposition::TooLate
        );
        assert_eq!(
            ScriptCancelDisposition::classify("invocation", Some("other"), false),
            ScriptCancelDisposition::Unknown
        );
    }
}
