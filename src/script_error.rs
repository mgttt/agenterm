use rhai::{Dynamic, EvalAltResult, Map, Position};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ScriptRuntimeErrorFields {
    pub class: String,
    pub code: String,
    pub operation: String,
    pub safe_message: String,
    pub retryable: bool,
    pub target_kind: String,
    pub truncated: bool,
    pub cause_class: Option<String>,
}

#[allow(clippy::too_many_arguments)]
pub fn runtime_error(
    class: &str,
    code: &str,
    operation: &str,
    safe_message: impl Into<String>,
    retryable: bool,
    target_kind: &str,
    truncated: bool,
    cause_class: Option<&str>,
) -> Box<EvalAltResult> {
    let mut fields = Map::new();
    fields.insert("class".into(), class.into());
    fields.insert("code".into(), code.into());
    fields.insert("operation".into(), operation.into());
    fields.insert("safe_message".into(), safe_message.into().into());
    fields.insert("retryable".into(), retryable.into());
    fields.insert("target_kind".into(), target_kind.into());
    fields.insert("truncated".into(), truncated.into());
    if let Some(cause_class) = cause_class {
        fields.insert("cause_class".into(), cause_class.into());
    }
    EvalAltResult::ErrorRuntime(Dynamic::from_map(fields), Position::NONE).into()
}

pub fn runtime_error_fields(error: &EvalAltResult) -> Option<ScriptRuntimeErrorFields> {
    let EvalAltResult::ErrorRuntime(value, _) = error.unwrap_inner() else {
        return None;
    };
    let fields = value.as_map_ref().ok()?;
    Some(ScriptRuntimeErrorFields {
        class: string_field(&fields, "class")?,
        code: string_field(&fields, "code")?,
        operation: string_field(&fields, "operation")?,
        safe_message: string_field(&fields, "safe_message")?,
        retryable: fields.get("retryable")?.as_bool().ok()?,
        target_kind: string_field(&fields, "target_kind")?,
        truncated: fields.get("truncated")?.as_bool().ok()?,
        cause_class: fields
            .get("cause_class")
            .and_then(|value| value.clone().into_string().ok()),
    })
}

fn string_field(fields: &Map, name: &str) -> Option<String> {
    fields.get(name)?.clone().into_string().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typed_runtime_error_round_trips_all_public_fields() {
        let error = runtime_error(
            "child",
            "child_nonzero",
            "std.process.Output.require_success",
            "build-step: required child exited with code 7",
            false,
            "child_process",
            true,
            Some("exit_status"),
        );
        let fields = runtime_error_fields(&error).unwrap();
        assert_eq!(fields.class, "child");
        assert_eq!(fields.code, "child_nonzero");
        assert_eq!(fields.operation, "std.process.Output.require_success");
        assert_eq!(
            fields.safe_message,
            "build-step: required child exited with code 7"
        );
        assert!(!fields.retryable);
        assert_eq!(fields.target_kind, "child_process");
        assert!(fields.truncated);
        assert_eq!(fields.cause_class.as_deref(), Some("exit_status"));
    }
}
