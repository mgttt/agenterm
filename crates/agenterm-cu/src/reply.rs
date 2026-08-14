//! Typed machine-readable replies.

use serde::{Deserialize, Serialize};

use crate::command::Command;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CuError {
    pub code: String,
    pub message: String,
    /// Present on `a11y_node_ambiguous` so callers can see how many showing
    /// nodes matched without parsing the message.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub count: Option<usize>,
}

impl CuError {
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            count: None,
        }
    }

    pub fn with_count(mut self, count: usize) -> Self {
        self.count = Some(count);
        self
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CuReply {
    pub ok: bool,
    pub target: String,
    pub command: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<CuError>,
}

impl CuReply {
    pub fn ok(command: &Command, data: serde_json::Value) -> Self {
        Self {
            ok: true,
            target: command.target().as_str().into(),
            command: command.verb(),
            data: Some(data),
            error: None,
        }
    }

    pub fn err(command: &Command, error: CuError) -> Self {
        Self {
            ok: false,
            target: command.target().as_str().into(),
            command: command.verb(),
            data: None,
            error: Some(error),
        }
    }
}
