use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const SCRIPT_ENVELOPE_VERSION: u32 = 1;
pub const SCRIPT_API_VERSION: u32 = 1;

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
        }
    }
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
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ScriptResult {
    pub envelope_version: u32,
    pub invocation_id: String,
    pub api_version: u32,
    pub ok: bool,
    pub exit_class: String,
    pub stdout: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failure: Option<ScriptFailure>,
    pub duration_ms: u64,
}
