use std::{
    collections::HashSet,
    error::Error,
    fmt,
    io::{self, Read, Write},
};

use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const SCRIPT_ENVELOPE_VERSION: u32 = 2;
pub const SCRIPT_API_VERSION: u32 = 2;
pub const RH_EVAL_VALUE_MARKER: &str = "__AGENTERM_RH_EVAL_VALUE_V1__";
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
            // Every other field here keeps real headroom above its
            // `Default` (operations: 100x, string_bytes: 32x, output_bytes:
            // 16x, wall_time_ms: 1800x, ...) so an opt-in CLI override can
            // actually raise the budget. `source_bytes` used to be pinned
            // at exactly the default (256 KiB == 256 KiB), which is why
            // `agenterm cli script run` could never accept a compiled
            // `.wasm` guest even with a `--max-*` override: there was no
            // ceiling headroom to override into. 16 MiB restores that
            // headroom (64x default, same order of magnitude as the other
            // fields' multipliers) without raising the *default* itself —
            // `Default::default().source_bytes` below is untouched, so a
            // script invocation that omits the override keeps the exact
            // 256 KiB behavior it always had.
            source_bytes: 16 * 1024 * 1024,
            operations: 100_000_000,
            call_depth: 128,
            expression_depth: 128,
            collection_items: 100_000,
            string_bytes: 8 * 1024 * 1024,
            output_bytes: 1024 * 1024,
            wall_time_ms: 3_600_000,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub invocation_temp_root: Option<String>,
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
    Child,
    Cancelled,
    Fleet,
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
    Child,
    Cancelled,
    Fleet,
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
            Self::Child => "child",
            Self::Cancelled => "cancelled",
            Self::Fleet => "fleet",
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
    ReplRequest(ReplSessionRequest),
    ReplResponse(ReplSessionResponse),
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

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ReplSessionRequest {
    pub session_id: String,
    pub generation: u64,
    pub sequence: u64,
    #[serde(flatten)]
    pub command: ReplSessionCommand,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "command", content = "payload", rename_all = "snake_case")]
pub enum ReplSessionCommand {
    Open { config: ReplSessionWireConfig },
    Evaluate { cell_id: String, source: String },
    Inspect { source: String },
    Query { query: ReplSessionQuery },
    Reset,
    Cancel { cell_id: String },
    Close,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ReplSessionWireConfig {
    pub budgets: ScriptBudgets,
    pub arguments: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_root: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub invocation_temp_root: Option<String>,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ReplSessionQuery {
    State,
    History,
    Variables,
    Functions,
    Limits,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ReplSessionResponse {
    pub session_id: String,
    pub generation: u64,
    pub sequence: u64,
    #[serde(flatten)]
    pub event: ReplSessionEvent,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "event", content = "payload", rename_all = "snake_case")]
pub enum ReplSessionEvent {
    Ready {
        worker_pid: u32,
    },
    InputState {
        state: ReplWireInputState,
    },
    CellStarted {
        cell_id: String,
    },
    CellResult {
        cell_id: String,
        result: ReplWireCellResult,
    },
    QueryResult {
        query: ReplSessionQuery,
        result: ReplWireQueryResult,
    },
    ResetDone,
    Closed,
    Failure {
        failure: ReplWireFailure,
    },
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "state", content = "failure", rename_all = "snake_case")]
pub enum ReplWireInputState {
    Complete,
    Incomplete,
    Invalid(ReplWireFailure),
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ReplWireCellResult {
    pub cell_sequence: u64,
    pub ok: bool,
    pub stdout: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<ReplWireValue>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failure: Option<ReplWireFailure>,
    pub state_committed: bool,
    pub duration_ms: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ReplWireValue {
    pub type_name: String,
    pub serializable: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<Value>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ReplWireFailure {
    pub code: String,
    pub category: String,
    pub message: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "result", content = "value", rename_all = "snake_case")]
pub enum ReplWireQueryResult {
    State {
        history: Vec<String>,
        variables: Vec<ReplWireVariable>,
        functions: Vec<String>,
        limits: ScriptBudgets,
    },
    History(Vec<String>),
    Variables(Vec<ReplWireVariable>),
    Functions(Vec<String>),
    Limits(ScriptBudgets),
}

impl ReplWireQueryResult {
    pub const fn query(&self) -> ReplSessionQuery {
        match self {
            Self::State { .. } => ReplSessionQuery::State,
            Self::History(_) => ReplSessionQuery::History,
            Self::Variables(_) => ReplSessionQuery::Variables,
            Self::Functions(_) => ReplSessionQuery::Functions,
            Self::Limits(_) => ReplSessionQuery::Limits,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ReplWireVariable {
    pub name: String,
    pub type_name: String,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ReplSessionPhase {
    #[default]
    Vacant,
    Idle,
    Evaluating,
    Closed,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReplSessionRejection {
    pub code: &'static str,
    pub message: String,
}

#[derive(Default)]
pub struct ReplRequestValidator {
    phase: ReplSessionPhase,
    session_id: Option<String>,
    generation: Option<u64>,
    next_sequence: u64,
    active_cell_id: Option<String>,
}

impl ReplRequestValidator {
    pub const fn phase(&self) -> ReplSessionPhase {
        self.phase
    }

    pub fn admit(&mut self, request: &ReplSessionRequest) -> Result<(), ReplSessionRejection> {
        self.validate_envelope(
            &request.session_id,
            request.generation,
            request.sequence,
            matches!(&request.command, ReplSessionCommand::Open { .. }),
        )?;
        match &request.command {
            ReplSessionCommand::Evaluate { cell_id, .. }
            | ReplSessionCommand::Cancel { cell_id } => validate_repl_cell_id(cell_id)?,
            _ => {}
        }
        if let ReplSessionCommand::Cancel { cell_id } = &request.command
            && self.active_cell_id.as_deref() != Some(cell_id)
        {
            return Err(repl_cell_rejection(cell_id, self.active_cell_id.as_deref()));
        }
        let next_phase = match (&self.phase, &request.command) {
            (ReplSessionPhase::Vacant, ReplSessionCommand::Open { .. }) => ReplSessionPhase::Idle,
            (
                ReplSessionPhase::Idle,
                ReplSessionCommand::Inspect { .. }
                | ReplSessionCommand::Query { .. }
                | ReplSessionCommand::Reset,
            ) => ReplSessionPhase::Idle,
            (ReplSessionPhase::Idle, ReplSessionCommand::Evaluate { .. }) => {
                ReplSessionPhase::Evaluating
            }
            (ReplSessionPhase::Idle, ReplSessionCommand::Close) => ReplSessionPhase::Closed,
            (ReplSessionPhase::Evaluating, ReplSessionCommand::Cancel { .. }) => {
                ReplSessionPhase::Evaluating
            }
            _ => return Err(repl_phase_rejection(self.phase, "request")),
        };
        self.commit_envelope(&request.session_id, request.generation, request.sequence)?;
        self.phase = next_phase;
        if let ReplSessionCommand::Evaluate { cell_id, .. } = &request.command {
            self.active_cell_id = Some(cell_id.clone());
        }
        Ok(())
    }

    pub fn admit_frame(&mut self, frame: &ScriptFrame) -> Result<(), ReplSessionRejection> {
        validate_repl_frame_envelope(frame)?;
        match &frame.payload {
            ScriptFramePayload::ReplRequest(request) => self.admit(request),
            _ => Err(ReplSessionRejection {
                code: "protocol_repl_unexpected_frame",
                message: "REPL request validator received a non-request frame".to_owned(),
            }),
        }
    }

    pub fn complete_evaluation(
        &mut self,
        session_id: &str,
        generation: u64,
    ) -> Result<(), ReplSessionRejection> {
        self.validate_bound_identity(session_id, generation)?;
        if self.phase != ReplSessionPhase::Evaluating {
            return Err(repl_phase_rejection(self.phase, "evaluation completion"));
        }
        self.phase = ReplSessionPhase::Idle;
        self.active_cell_id = None;
        Ok(())
    }

    fn validate_envelope(
        &self,
        session_id: &str,
        generation: u64,
        sequence: u64,
        opening: bool,
    ) -> Result<(), ReplSessionRejection> {
        validate_repl_identity(session_id, generation)?;
        if self.phase == ReplSessionPhase::Vacant {
            if !opening {
                return Err(repl_phase_rejection(self.phase, "request"));
            }
            if sequence != 0 {
                return Err(repl_sequence_rejection(sequence, 0));
            }
        } else {
            self.validate_bound_identity(session_id, generation)?;
            if sequence != self.next_sequence {
                return Err(repl_sequence_rejection(sequence, self.next_sequence));
            }
        }
        if sequence == u64::MAX {
            return Err(ReplSessionRejection {
                code: "protocol_repl_sequence_overflow",
                message: "REPL sequence cannot advance beyond u64::MAX".to_owned(),
            });
        }
        Ok(())
    }

    fn validate_bound_identity(
        &self,
        session_id: &str,
        generation: u64,
    ) -> Result<(), ReplSessionRejection> {
        if self.session_id.as_deref() != Some(session_id) {
            return Err(ReplSessionRejection {
                code: "protocol_repl_session_mismatch",
                message: "REPL frame does not match the admitted session".to_owned(),
            });
        }
        match self.generation {
            Some(expected) if generation < expected => Err(ReplSessionRejection {
                code: "protocol_repl_stale_generation",
                message: format!(
                    "REPL generation {generation} is stale; current generation is {expected}"
                ),
            }),
            Some(expected) if generation != expected => Err(ReplSessionRejection {
                code: "protocol_repl_generation_mismatch",
                message: format!(
                    "REPL generation {generation} does not match current generation {expected}"
                ),
            }),
            Some(_) => Ok(()),
            None => Err(repl_phase_rejection(self.phase, "identity")),
        }
    }

    fn commit_envelope(
        &mut self,
        session_id: &str,
        generation: u64,
        sequence: u64,
    ) -> Result<(), ReplSessionRejection> {
        let next_sequence = sequence.checked_add(1).ok_or(ReplSessionRejection {
            code: "protocol_repl_sequence_overflow",
            message: "REPL sequence cannot advance beyond u64::MAX".to_owned(),
        })?;
        if self.session_id.is_none() {
            self.session_id = Some(session_id.to_owned());
            self.generation = Some(generation);
        }
        self.next_sequence = next_sequence;
        Ok(())
    }
}

#[derive(Default)]
pub struct ReplResponseValidator {
    phase: ReplSessionPhase,
    session_id: Option<String>,
    generation: Option<u64>,
    next_sequence: u64,
    active_cell_id: Option<String>,
}

impl ReplResponseValidator {
    pub const fn phase(&self) -> ReplSessionPhase {
        self.phase
    }

    pub fn admit(&mut self, response: &ReplSessionResponse) -> Result<(), ReplSessionRejection> {
        validate_repl_identity(&response.session_id, response.generation)?;
        if self.phase == ReplSessionPhase::Vacant {
            if response.sequence != 0 {
                return Err(repl_sequence_rejection(response.sequence, 0));
            }
        } else {
            self.validate_bound_identity(&response.session_id, response.generation)?;
            if response.sequence != self.next_sequence {
                return Err(repl_sequence_rejection(
                    response.sequence,
                    self.next_sequence,
                ));
            }
        }
        if response.sequence == u64::MAX {
            return Err(ReplSessionRejection {
                code: "protocol_repl_sequence_overflow",
                message: "REPL sequence cannot advance beyond u64::MAX".to_owned(),
            });
        }
        match &response.event {
            ReplSessionEvent::CellStarted { cell_id }
            | ReplSessionEvent::CellResult { cell_id, .. } => validate_repl_cell_id(cell_id)?,
            ReplSessionEvent::QueryResult { query, result } if *query != result.query() => {
                return Err(ReplSessionRejection {
                    code: "protocol_repl_query_mismatch",
                    message: "REPL query result does not match its declared query".to_owned(),
                });
            }
            _ => {}
        }
        if let ReplSessionEvent::CellResult { cell_id, .. } = &response.event
            && self.active_cell_id.as_deref() != Some(cell_id)
        {
            return Err(repl_cell_rejection(cell_id, self.active_cell_id.as_deref()));
        }
        let next_phase = match (&self.phase, &response.event) {
            (ReplSessionPhase::Vacant, ReplSessionEvent::Ready { .. }) => ReplSessionPhase::Idle,
            (ReplSessionPhase::Vacant, ReplSessionEvent::Failure { .. }) => {
                ReplSessionPhase::Closed
            }
            (
                ReplSessionPhase::Idle,
                ReplSessionEvent::InputState { .. }
                | ReplSessionEvent::QueryResult { .. }
                | ReplSessionEvent::ResetDone
                | ReplSessionEvent::Failure { .. },
            ) => ReplSessionPhase::Idle,
            (ReplSessionPhase::Idle, ReplSessionEvent::CellStarted { .. }) => {
                ReplSessionPhase::Evaluating
            }
            (ReplSessionPhase::Idle, ReplSessionEvent::Closed) => ReplSessionPhase::Closed,
            (ReplSessionPhase::Evaluating, ReplSessionEvent::CellResult { .. }) => {
                ReplSessionPhase::Idle
            }
            (ReplSessionPhase::Evaluating, ReplSessionEvent::Failure { .. }) => {
                ReplSessionPhase::Evaluating
            }
            _ => return Err(repl_phase_rejection(self.phase, "response")),
        };
        let next_sequence = response
            .sequence
            .checked_add(1)
            .ok_or(ReplSessionRejection {
                code: "protocol_repl_sequence_overflow",
                message: "REPL sequence cannot advance beyond u64::MAX".to_owned(),
            })?;
        if self.session_id.is_none() {
            self.session_id = Some(response.session_id.clone());
            self.generation = Some(response.generation);
        }
        self.next_sequence = next_sequence;
        self.phase = next_phase;
        match &response.event {
            ReplSessionEvent::CellStarted { cell_id } => {
                self.active_cell_id = Some(cell_id.clone());
            }
            ReplSessionEvent::CellResult { .. } => self.active_cell_id = None,
            _ => {}
        }
        Ok(())
    }

    pub fn admit_frame(&mut self, frame: &ScriptFrame) -> Result<(), ReplSessionRejection> {
        validate_repl_frame_envelope(frame)?;
        match &frame.payload {
            ScriptFramePayload::ReplResponse(response) => self.admit(response),
            _ => Err(ReplSessionRejection {
                code: "protocol_repl_unexpected_frame",
                message: "REPL response validator received a non-response frame".to_owned(),
            }),
        }
    }

    fn validate_bound_identity(
        &self,
        session_id: &str,
        generation: u64,
    ) -> Result<(), ReplSessionRejection> {
        if self.session_id.as_deref() != Some(session_id) {
            return Err(ReplSessionRejection {
                code: "protocol_repl_session_mismatch",
                message: "REPL frame does not match the admitted session".to_owned(),
            });
        }
        match self.generation {
            Some(expected) if generation < expected => Err(ReplSessionRejection {
                code: "protocol_repl_stale_generation",
                message: format!(
                    "REPL generation {generation} is stale; current generation is {expected}"
                ),
            }),
            Some(expected) if generation != expected => Err(ReplSessionRejection {
                code: "protocol_repl_generation_mismatch",
                message: format!(
                    "REPL generation {generation} does not match current generation {expected}"
                ),
            }),
            Some(_) => Ok(()),
            None => Err(repl_phase_rejection(self.phase, "identity")),
        }
    }
}

fn validate_repl_identity(session_id: &str, generation: u64) -> Result<(), ReplSessionRejection> {
    if session_id.is_empty() || session_id.len() > 128 {
        return Err(ReplSessionRejection {
            code: "protocol_repl_invalid_session_id",
            message: "session_id must contain from 1 to 128 bytes".to_owned(),
        });
    }
    if generation == 0 {
        return Err(ReplSessionRejection {
            code: "protocol_repl_invalid_generation",
            message: "REPL generation must be greater than zero".to_owned(),
        });
    }
    Ok(())
}

fn validate_repl_cell_id(cell_id: &str) -> Result<(), ReplSessionRejection> {
    if cell_id.is_empty() || cell_id.len() > 128 {
        Err(ReplSessionRejection {
            code: "protocol_repl_invalid_cell_id",
            message: "cell_id must contain from 1 to 128 bytes".to_owned(),
        })
    } else {
        Ok(())
    }
}

fn validate_repl_frame_envelope(frame: &ScriptFrame) -> Result<(), ReplSessionRejection> {
    if frame.frame_version != SCRIPT_FRAME_VERSION {
        return Err(ReplSessionRejection {
            code: "protocol_unsupported_frame_version",
            message: format!(
                "REPL frame version {} does not match {SCRIPT_FRAME_VERSION}",
                frame.frame_version
            ),
        });
    }
    if frame.frame_id.is_empty() || frame.frame_id.len() > 128 {
        return Err(ReplSessionRejection {
            code: "protocol_invalid_frame_id",
            message: "frame_id must contain from 1 to 128 bytes".to_owned(),
        });
    }
    Ok(())
}

fn repl_sequence_rejection(sequence: u64, expected: u64) -> ReplSessionRejection {
    if sequence < expected {
        ReplSessionRejection {
            code: "protocol_repl_stale_sequence",
            message: format!("REPL sequence {sequence} is stale; next sequence is {expected}"),
        }
    } else {
        ReplSessionRejection {
            code: "protocol_repl_sequence_mismatch",
            message: format!("REPL sequence {sequence} does not match next sequence {expected}"),
        }
    }
}

fn repl_phase_rejection(phase: ReplSessionPhase, message_kind: &str) -> ReplSessionRejection {
    ReplSessionRejection {
        code: "protocol_repl_phase_mismatch",
        message: format!("REPL {message_kind} is invalid while session phase is {phase:?}"),
    }
}

fn repl_cell_rejection(cell_id: &str, active_cell_id: Option<&str>) -> ReplSessionRejection {
    ReplSessionRejection {
        code: "protocol_repl_cell_mismatch",
        message: format!(
            "REPL cell {cell_id} does not match active cell {}",
            active_cell_id.unwrap_or("<none>")
        ),
    }
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
        if matches!(
            &frame.payload,
            ScriptFramePayload::ReplRequest(_) | ScriptFramePayload::ReplResponse(_)
        ) {
            return Err(ScriptFrameRejection::admission(
                frame_id,
                "unknown",
                "protocol_repl_requires_session_validator",
                "REPL frames require the constant-space session validator",
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

    fn repl_request(sequence: u64, command: ReplSessionCommand) -> ReplSessionRequest {
        ReplSessionRequest {
            session_id: "session-a".to_owned(),
            generation: 7,
            sequence,
            command,
        }
    }

    fn repl_response(sequence: u64, event: ReplSessionEvent) -> ReplSessionResponse {
        ReplSessionResponse {
            session_id: "session-a".to_owned(),
            generation: 7,
            sequence,
            event,
        }
    }

    #[test]
    fn repl_wire_round_trips_every_command_and_event() {
        let commands = vec![
            ReplSessionCommand::Open {
                config: ReplSessionWireConfig {
                    budgets: ScriptBudgets::default(),
                    arguments: vec!["argument".to_owned()],
                    project_root: Some("project".to_owned()),
                    invocation_temp_root: Some("temp".to_owned()),
                },
            },
            ReplSessionCommand::Evaluate {
                cell_id: "cell-1".to_owned(),
                source: "40 + 2".to_owned(),
            },
            ReplSessionCommand::Inspect {
                source: "fn add(x) {".to_owned(),
            },
            ReplSessionCommand::Query {
                query: ReplSessionQuery::State,
            },
            ReplSessionCommand::Reset,
            ReplSessionCommand::Cancel {
                cell_id: "cell-1".to_owned(),
            },
            ReplSessionCommand::Close,
        ];
        for (sequence, command) in commands.into_iter().enumerate() {
            let frame = ScriptFrame {
                frame_version: SCRIPT_FRAME_VERSION,
                frame_id: format!("request-{sequence}"),
                payload: ScriptFramePayload::ReplRequest(repl_request(sequence as u64, command)),
            };
            let mut encoded = Vec::new();
            write_script_frame(&mut encoded, &frame).expect("encode REPL request");
            let mut encoded = Cursor::new(encoded);
            let decoded = read_script_frame(&mut encoded).expect("decode REPL request");
            let ScriptFrameRead::Frame(decoded) = decoded else {
                panic!("encoded REPL request was rejected");
            };
            assert!(matches!(
                &decoded.payload,
                ScriptFramePayload::ReplRequest(_)
            ));
        }

        let failure = || ReplWireFailure {
            code: "script_runtime".to_owned(),
            category: "script".to_owned(),
            message: "failure".to_owned(),
        };
        let events = vec![
            ReplSessionEvent::Ready { worker_pid: 42 },
            ReplSessionEvent::InputState {
                state: ReplWireInputState::Invalid(failure()),
            },
            ReplSessionEvent::CellStarted {
                cell_id: "cell-1".to_owned(),
            },
            ReplSessionEvent::CellResult {
                cell_id: "cell-1".to_owned(),
                result: ReplWireCellResult {
                    cell_sequence: 1,
                    ok: false,
                    stdout: String::new(),
                    value: None,
                    failure: Some(failure()),
                    state_committed: false,
                    duration_ms: 3,
                },
            },
            ReplSessionEvent::QueryResult {
                query: ReplSessionQuery::Variables,
                result: ReplWireQueryResult::Variables(vec![ReplWireVariable {
                    name: "x".to_owned(),
                    type_name: "i64".to_owned(),
                }]),
            },
            ReplSessionEvent::ResetDone,
            ReplSessionEvent::Closed,
            ReplSessionEvent::Failure { failure: failure() },
        ];
        for (sequence, event) in events.into_iter().enumerate() {
            let frame = ScriptFrame {
                frame_version: SCRIPT_FRAME_VERSION,
                frame_id: format!("response-{sequence}"),
                payload: ScriptFramePayload::ReplResponse(repl_response(sequence as u64, event)),
            };
            let mut encoded = Vec::new();
            write_script_frame(&mut encoded, &frame).expect("encode REPL response");
            let mut encoded = Cursor::new(encoded);
            let decoded = read_script_frame(&mut encoded).expect("decode REPL response");
            let ScriptFrameRead::Frame(decoded) = decoded else {
                panic!("encoded REPL response was rejected");
            };
            assert!(matches!(
                &decoded.payload,
                ScriptFramePayload::ReplResponse(_)
            ));
        }
    }

    #[test]
    fn repl_request_validator_tracks_phase_with_constant_state() {
        let mut validator = ReplRequestValidator::default();
        let open = ScriptFrame {
            frame_version: SCRIPT_FRAME_VERSION,
            frame_id: "repl-open".to_owned(),
            payload: ScriptFramePayload::ReplRequest(repl_request(
                0,
                ReplSessionCommand::Open {
                    config: ReplSessionWireConfig {
                        budgets: ScriptBudgets::default(),
                        arguments: Vec::new(),
                        project_root: None,
                        invocation_temp_root: None,
                    },
                },
            )),
        };
        validator.admit_frame(&open).unwrap();
        assert_eq!(validator.phase(), ReplSessionPhase::Idle);
        validator
            .admit(&repl_request(
                1,
                ReplSessionCommand::Inspect {
                    source: "40 + 2".to_owned(),
                },
            ))
            .unwrap();
        validator
            .admit(&repl_request(
                2,
                ReplSessionCommand::Evaluate {
                    cell_id: "cell-1".to_owned(),
                    source: "while true {}".to_owned(),
                },
            ))
            .unwrap();
        assert_eq!(validator.phase(), ReplSessionPhase::Evaluating);
        validator
            .admit(&repl_request(
                3,
                ReplSessionCommand::Cancel {
                    cell_id: "cell-1".to_owned(),
                },
            ))
            .unwrap();
        validator.complete_evaluation("session-a", 7).unwrap();
        validator
            .admit(&repl_request(
                4,
                ReplSessionCommand::Query {
                    query: ReplSessionQuery::State,
                },
            ))
            .unwrap();
        validator
            .admit(&repl_request(5, ReplSessionCommand::Reset))
            .unwrap();
        validator
            .admit(&repl_request(6, ReplSessionCommand::Close))
            .unwrap();
        assert_eq!(validator.phase(), ReplSessionPhase::Closed);
        assert_eq!(
            validator
                .admit(&repl_request(
                    7,
                    ReplSessionCommand::Query {
                        query: ReplSessionQuery::Limits,
                    },
                ))
                .unwrap_err()
                .code,
            "protocol_repl_phase_mismatch"
        );
    }

    #[test]
    fn repl_request_validator_rejects_stale_mismatched_and_overflowing_envelopes() {
        let mut validator = ReplRequestValidator::default();
        validator
            .admit(&repl_request(
                0,
                ReplSessionCommand::Open {
                    config: ReplSessionWireConfig {
                        budgets: ScriptBudgets::default(),
                        arguments: Vec::new(),
                        project_root: None,
                        invocation_temp_root: None,
                    },
                },
            ))
            .unwrap();

        let query = || ReplSessionCommand::Query {
            query: ReplSessionQuery::History,
        };
        assert_eq!(
            validator.admit(&repl_request(0, query())).unwrap_err().code,
            "protocol_repl_stale_sequence"
        );
        assert_eq!(
            validator.admit(&repl_request(2, query())).unwrap_err().code,
            "protocol_repl_sequence_mismatch"
        );
        let mut wrong_session = repl_request(1, query());
        wrong_session.session_id = "session-b".to_owned();
        assert_eq!(
            validator.admit(&wrong_session).unwrap_err().code,
            "protocol_repl_session_mismatch"
        );
        let mut stale_generation = repl_request(1, query());
        stale_generation.generation = 6;
        assert_eq!(
            validator.admit(&stale_generation).unwrap_err().code,
            "protocol_repl_stale_generation"
        );
        let mut future_generation = repl_request(1, query());
        future_generation.generation = 8;
        assert_eq!(
            validator.admit(&future_generation).unwrap_err().code,
            "protocol_repl_generation_mismatch"
        );

        validator.next_sequence = u64::MAX;
        let mut overflow = repl_request(u64::MAX, query());
        overflow.generation = 7;
        assert_eq!(
            validator.admit(&overflow).unwrap_err().code,
            "protocol_repl_sequence_overflow"
        );
        assert_eq!(validator.next_sequence, u64::MAX);
    }

    #[test]
    fn repl_response_validator_rejects_out_of_phase_and_stale_responses() {
        let mut validator = ReplResponseValidator::default();
        validator
            .admit(&repl_response(
                0,
                ReplSessionEvent::Ready { worker_pid: 42 },
            ))
            .unwrap();
        validator
            .admit(&repl_response(
                1,
                ReplSessionEvent::CellStarted {
                    cell_id: "cell-1".to_owned(),
                },
            ))
            .unwrap();
        assert_eq!(validator.phase(), ReplSessionPhase::Evaluating);
        assert_eq!(
            validator
                .admit(&repl_response(1, ReplSessionEvent::ResetDone))
                .unwrap_err()
                .code,
            "protocol_repl_stale_sequence"
        );
        assert_eq!(
            validator
                .admit(&repl_response(2, ReplSessionEvent::ResetDone))
                .unwrap_err()
                .code,
            "protocol_repl_phase_mismatch"
        );
        validator
            .admit(&repl_response(
                2,
                ReplSessionEvent::CellResult {
                    cell_id: "cell-1".to_owned(),
                    result: ReplWireCellResult {
                        cell_sequence: 1,
                        ok: true,
                        stdout: String::new(),
                        value: None,
                        failure: None,
                        state_committed: true,
                        duration_ms: 1,
                    },
                },
            ))
            .unwrap();
        validator
            .admit(&repl_response(3, ReplSessionEvent::Closed))
            .unwrap();
        assert_eq!(validator.phase(), ReplSessionPhase::Closed);
    }

    #[test]
    fn repl_request_cancel_mismatch_does_not_advance_validator_state() {
        let mut validator = ReplRequestValidator::default();
        validator
            .admit(&repl_request(
                0,
                ReplSessionCommand::Open {
                    config: ReplSessionWireConfig {
                        budgets: ScriptBudgets::default(),
                        arguments: Vec::new(),
                        project_root: None,
                        invocation_temp_root: None,
                    },
                },
            ))
            .unwrap();
        validator
            .admit(&repl_request(
                1,
                ReplSessionCommand::Evaluate {
                    cell_id: "cell-1".to_owned(),
                    source: "while true {}".to_owned(),
                },
            ))
            .unwrap();

        let rejection = validator
            .admit(&repl_request(
                2,
                ReplSessionCommand::Cancel {
                    cell_id: "cell-2".to_owned(),
                },
            ))
            .unwrap_err();
        assert_eq!(rejection.code, "protocol_repl_cell_mismatch");
        assert_eq!(validator.phase(), ReplSessionPhase::Evaluating);
        assert_eq!(validator.next_sequence, 2);
        assert_eq!(validator.active_cell_id.as_deref(), Some("cell-1"));

        validator
            .admit(&repl_request(
                2,
                ReplSessionCommand::Cancel {
                    cell_id: "cell-1".to_owned(),
                },
            ))
            .unwrap();
    }

    #[test]
    fn repl_response_cell_result_mismatch_does_not_advance_validator_state() {
        let mut validator = ReplResponseValidator::default();
        validator
            .admit(&repl_response(
                0,
                ReplSessionEvent::Ready { worker_pid: 42 },
            ))
            .unwrap();
        validator
            .admit(&repl_response(
                1,
                ReplSessionEvent::CellStarted {
                    cell_id: "cell-1".to_owned(),
                },
            ))
            .unwrap();
        let result = || ReplWireCellResult {
            cell_sequence: 1,
            ok: true,
            stdout: String::new(),
            value: None,
            failure: None,
            state_committed: true,
            duration_ms: 1,
        };

        let rejection = validator
            .admit(&repl_response(
                2,
                ReplSessionEvent::CellResult {
                    cell_id: "cell-2".to_owned(),
                    result: result(),
                },
            ))
            .unwrap_err();
        assert_eq!(rejection.code, "protocol_repl_cell_mismatch");
        assert_eq!(validator.phase(), ReplSessionPhase::Evaluating);
        assert_eq!(validator.next_sequence, 2);
        assert_eq!(validator.active_cell_id.as_deref(), Some("cell-1"));

        validator
            .admit(&repl_response(
                2,
                ReplSessionEvent::Failure {
                    failure: ReplWireFailure {
                        code: "protocol_repl_cell_mismatch".to_owned(),
                        category: "protocol".to_owned(),
                        message: "cancel addressed a different cell".to_owned(),
                    },
                },
            ))
            .unwrap();
        assert_eq!(validator.phase(), ReplSessionPhase::Evaluating);
        assert_eq!(validator.next_sequence, 3);
        assert_eq!(validator.active_cell_id.as_deref(), Some("cell-1"));

        validator
            .admit(&repl_response(
                3,
                ReplSessionEvent::CellResult {
                    cell_id: "cell-1".to_owned(),
                    result: result(),
                },
            ))
            .unwrap();
        assert_eq!(validator.phase(), ReplSessionPhase::Idle);
        assert!(validator.active_cell_id.is_none());
    }

    #[test]
    fn repl_frames_cannot_enter_the_hash_set_tracker() {
        let frame_id = "constant-space-frame";
        let repl = ScriptFrame {
            frame_version: SCRIPT_FRAME_VERSION,
            frame_id: frame_id.to_owned(),
            payload: ScriptFramePayload::ReplRequest(repl_request(
                0,
                ReplSessionCommand::Open {
                    config: ReplSessionWireConfig {
                        budgets: ScriptBudgets::default(),
                        arguments: Vec::new(),
                        project_root: None,
                        invocation_temp_root: None,
                    },
                },
            )),
        };
        let mut tracker = ScriptFrameTracker::default();
        assert_eq!(
            tracker.admit(repl).unwrap_err().code,
            "protocol_repl_requires_session_validator"
        );
        assert!(tracker.admit(cancel_frame(frame_id, "invocation")).is_ok());
    }
}
