use serde::{Deserialize, Serialize};

use crate::control_contract::{ControlReceipt, ControlRequest};

const IPC_ERROR_CODE_COMMAND_FAILED: &str = "ipc.command_failed";
const IPC_ERROR_CATEGORY_COMMAND: &str = "command";

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct IpcRequest {
    pub(crate) args: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) control: Option<ControlRequest>,
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct IpcResponse {
    pub(crate) ok: bool,
    pub(crate) output: String,
    pub(crate) error: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub(crate) error_code: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub(crate) error_category: String,
    #[serde(default)]
    pub(crate) retryable: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) receipt: Option<ControlReceipt>,
}

impl IpcResponse {
    pub(crate) fn success(output: impl Into<String>) -> Self {
        Self {
            ok: true,
            output: output.into(),
            error: String::new(),
            error_code: String::new(),
            error_category: String::new(),
            retryable: false,
            receipt: None,
        }
    }

    pub(crate) fn failure(error: impl Into<String>) -> Self {
        Self {
            ok: false,
            output: String::new(),
            error: error.into(),
            // Existing callers provide only human-readable text. Keep their
            // first structured classification deliberately broad until each
            // command path can opt into a narrower stable code.
            error_code: IPC_ERROR_CODE_COMMAND_FAILED.to_owned(),
            error_category: IPC_ERROR_CATEGORY_COMMAND.to_owned(),
            retryable: false,
            receipt: None,
        }
    }

    pub(crate) fn typed_failure(
        error: impl Into<String>,
        code: impl Into<String>,
        category: impl Into<String>,
        retryable: bool,
    ) -> Self {
        Self {
            ok: false,
            output: String::new(),
            error: error.into(),
            error_code: code.into(),
            error_category: category.into(),
            retryable,
            receipt: None,
        }
    }

    pub(crate) fn with_receipt(mut self, receipt: ControlReceipt) -> Self {
        self.receipt = Some(receipt);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_and_response_are_newline_protocol_safe() {
        let request = IpcRequest {
            args: vec!["set-composer".to_owned(), "hello\nworld".to_owned()],
            control: None,
        };
        let encoded = serde_json::to_string(&request).unwrap();
        assert!(!encoded.contains('\n'));

        let response = IpcResponse::success("ok");
        let roundtrip: IpcResponse =
            serde_json::from_str(&serde_json::to_string(&response).unwrap()).unwrap();
        assert!(roundtrip.ok);
        assert_eq!(roundtrip.output, "ok");
    }

    #[test]
    fn old_response_payloads_default_missing_structured_error_fields() {
        let response: IpcResponse =
            serde_json::from_str(r#"{"ok":false,"output":"","error":"legacy failure"}"#).unwrap();

        assert!(!response.ok);
        assert_eq!(response.error, "legacy failure");
        assert_eq!(response.error_code, "");
        assert_eq!(response.error_category, "");
        assert!(!response.retryable);
    }

    #[test]
    fn success_response_preserves_legacy_fields_and_is_not_retryable() {
        let encoded = serde_json::to_value(IpcResponse::success("done")).unwrap();

        assert_eq!(
            encoded,
            serde_json::json!({
                "ok": true,
                "output": "done",
                "error": "",
                "retryable": false
            })
        );
    }

    #[test]
    fn failures_expose_stable_machine_readable_fallback_fields() {
        let encoded = serde_json::to_value(IpcResponse::failure("bad command")).unwrap();

        assert_eq!(encoded["ok"], false);
        assert_eq!(encoded["error"], "bad command");
        assert_eq!(encoded["error_code"], IPC_ERROR_CODE_COMMAND_FAILED);
        assert_eq!(encoded["error_category"], IPC_ERROR_CATEGORY_COMMAND);
        assert_eq!(encoded["retryable"], false);
    }

    #[test]
    fn legacy_clients_ignore_new_structured_error_fields() {
        #[derive(Deserialize)]
        struct LegacyIpcResponse {
            ok: bool,
            output: String,
            error: String,
        }

        let encoded = serde_json::to_string(&IpcResponse::failure("bad command")).unwrap();
        let legacy: LegacyIpcResponse = serde_json::from_str(&encoded).unwrap();

        assert!(!legacy.ok);
        assert_eq!(legacy.output, "");
        assert_eq!(legacy.error, "bad command");
    }
}
