use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const SCRIPT_ENVELOPE_VERSION: u32 = 1;
pub const SCRIPT_API_VERSION: u32 = 1;
pub const SCRIPT_INVOCATION_MAX_BYTES: u64 = 2 * 1024 * 1024;
pub const SCRIPT_FRAME_VERSION: u32 = 1;
pub const SCRIPT_FRAME_MAX_BYTES: u32 = 2 * 1024 * 1024;

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ScriptProfile {
    Pure,
    Observe,
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
