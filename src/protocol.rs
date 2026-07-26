use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct IpcRequest {
    pub(crate) args: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct IpcResponse {
    pub(crate) ok: bool,
    pub(crate) output: String,
    pub(crate) error: String,
}

impl IpcResponse {
    pub(crate) fn success(output: impl Into<String>) -> Self {
        Self {
            ok: true,
            output: output.into(),
            error: String::new(),
        }
    }

    pub(crate) fn failure(error: impl Into<String>) -> Self {
        Self {
            ok: false,
            output: String::new(),
            error: error.into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_and_response_are_newline_protocol_safe() {
        let request = IpcRequest {
            args: vec!["set-composer".to_owned(), "hello\nworld".to_owned()],
        };
        let encoded = serde_json::to_string(&request).unwrap();
        assert!(!encoded.contains('\n'));

        let response = IpcResponse::success("ok");
        let roundtrip: IpcResponse =
            serde_json::from_str(&serde_json::to_string(&response).unwrap()).unwrap();
        assert!(roundtrip.ok);
        assert_eq!(roundtrip.output, "ok");
    }
}
