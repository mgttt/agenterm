//! Static API surface validation for script sources (shared by Rhai and rh check paths).

use crate::shipped_surfaces::SHIPPED_SURFACE_PATHS;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApiValidateError {
    pub code: &'static str,
    pub message: String,
}

fn script_error(code: &'static str, message: impl Into<String>) -> ApiValidateError {
    ApiValidateError {
        code,
        message: message.into(),
    }
}

fn is_shipped_surface(path: &str) -> bool {
    SHIPPED_SURFACE_PATHS.contains(&path)
        || path == "std::fs::try_remove_file"
        || path == "std::fs::try_copy"
        || path == "std::fs::try_create_dir_all"
        || path == "std::fs::try_rename"
}

pub fn validate_available_apis(source: &str) -> Result<(), ApiValidateError> {
    for surface_path in qualified_function_calls(source) {
        if !is_shipped_surface(&surface_path) {
            return Err(script_error(
                "script_api_unknown",
                format!("unknown shipped scripting API: {surface_path}"),
            ));
        }
    }
    if let Some(method) = agent_method_calls(source).into_iter().next() {
        let replacement = match method.as_str() {
            "workspace" => "fleet.workspace.info()",
            "tabs" => "fleet.tabs.list()",
            "active_tab" => "fleet.tabs.active()",
            "ui_snapshot" => "fleet.ui.snapshot()",
            "capture" => "fleet.terminal(tab).capture(max_bytes)",
            "events_read" => "fleet.events.read(epoch, after, limit)",
            "events_wait" => "fleet.events.wait(epoch, after, kind, timeout_ms)",
            _ => "the canonical fleet object",
        };
        return Err(script_error(
            "script_api_migrated",
            format!("agent.{method} was removed in Script API v2; use {replacement}"),
        ));
    }
    for surface_path in fleet_method_calls(source) {
        if matches!(surface_path.as_str(), "fleet.terminal" | "fleet.operations") {
            continue;
        }
        if !is_shipped_surface(&surface_path) {
            return Err(script_error(
                "script_api_unknown",
                format!("unknown shipped scripting API: {surface_path}"),
            ));
        }
    }
    for call in external_function_calls(source) {
        match call.as_str() {
            "print" | "debug" | "type_of" | "is_def_var" | "is_shared" | "eval" | "to_string"
            | "to_debug" | "require" => {
                // language-level builtins lowered by native codegen
            }
            "new_tab" => {
                return Err(script_error(
                    "script_api_unavailable",
                    "API new_tab is not shipped",
                ));
            }
            _ => {
                return Err(script_error(
                    "script_api_unknown",
                    format!("unknown shipped scripting API: {call}"),
                ));
            }
        }
    }
    Ok(())
}

pub fn qualified_function_calls(source: &str) -> Vec<String> {
    let bytes = source.as_bytes();
    let mut calls = Vec::new();
    let mut index = 0;
    while index < bytes.len() {
        if matches!(bytes[index], b'"' | b'\'' | b'`') {
            let quote = bytes[index];
            index += 1;
            while index < bytes.len() {
                if bytes[index] == b'\\' {
                    index = (index + 2).min(bytes.len());
                } else if bytes[index] == quote {
                    index += 1;
                    break;
                } else {
                    index += 1;
                }
            }
            continue;
        }
        if bytes[index] == b'/' && bytes.get(index + 1) == Some(&b'/') {
            index += 2;
            while index < bytes.len() && bytes[index] != b'\n' {
                index += 1;
            }
            continue;
        }
        if bytes[index] == b'/' && bytes.get(index + 1) == Some(&b'*') {
            index += 2;
            while index + 1 < bytes.len() && &bytes[index..index + 2] != b"*/" {
                index += 1;
            }
            index = (index + 2).min(bytes.len());
            continue;
        }
        if source[index..].starts_with("std::")
            || source[index..].starts_with("rh::")
            || source[index..].starts_with("rhai::")
        {
            let start = index;
            while index < bytes.len()
                && (bytes[index] == b'_'
                    || bytes[index] == b':'
                    || bytes[index].is_ascii_alphanumeric())
            {
                index += 1;
            }
            let mut next = index;
            while next < bytes.len() && bytes[next].is_ascii_whitespace() {
                next += 1;
            }
            if bytes.get(next) == Some(&b'(') {
                calls.push(source[start..index].to_owned());
            }
        } else {
            index += 1;
        }
    }
    calls
}

pub fn agent_method_calls(source: &str) -> Vec<String> {
    let bytes = source.as_bytes();
    let mut methods = Vec::new();
    let mut index = 0;
    while index + 6 < bytes.len() {
        if matches!(bytes[index], b'"' | b'\'' | b'`') {
            let quote = bytes[index];
            index += 1;
            while index < bytes.len() {
                if bytes[index] == b'\\' {
                    index = (index + 2).min(bytes.len());
                } else if bytes[index] == quote {
                    index += 1;
                    break;
                } else {
                    index += 1;
                }
            }
            continue;
        }
        if bytes[index] == b'/' && bytes.get(index + 1) == Some(&b'/') {
            index += 2;
            while index < bytes.len() && bytes[index] != b'\n' {
                index += 1;
            }
            continue;
        }
        if bytes[index] == b'/' && bytes.get(index + 1) == Some(&b'*') {
            index += 2;
            while index + 1 < bytes.len() && &bytes[index..index + 2] != b"*/" {
                index += 1;
            }
            index = (index + 2).min(bytes.len());
            continue;
        }
        if source[index..].starts_with("agent.") {
            let start = index + 6;
            let mut end = start;
            while end < bytes.len() && (bytes[end] == b'_' || bytes[end].is_ascii_alphanumeric()) {
                end += 1;
            }
            let mut next = end;
            while next < bytes.len() && bytes[next].is_ascii_whitespace() {
                next += 1;
            }
            if next < bytes.len() && bytes[next] == b'(' {
                methods.push(source[start..end].to_owned());
            }
            index = end.max(index + 1);
        } else {
            index += 1;
        }
    }
    methods
}

pub fn fleet_method_calls(source: &str) -> Vec<String> {
    let bytes = source.as_bytes();
    let mut paths = Vec::new();
    let mut index = 0;
    while index < bytes.len() {
        if matches!(bytes[index], b'"' | b'\'' | b'`') {
            let quote = bytes[index];
            index += 1;
            while index < bytes.len() {
                if bytes[index] == b'\\' {
                    index = (index + 2).min(bytes.len());
                } else if bytes[index] == quote {
                    index += 1;
                    break;
                } else {
                    index += 1;
                }
            }
            continue;
        }
        if bytes[index] == b'/' && bytes.get(index + 1) == Some(&b'/') {
            index += 2;
            while index < bytes.len() && bytes[index] != b'\n' {
                index += 1;
            }
            continue;
        }
        if bytes[index] == b'/' && bytes.get(index + 1) == Some(&b'*') {
            index += 2;
            while index + 1 < bytes.len() && &bytes[index..index + 2] != b"*/" {
                index += 1;
            }
            index = (index + 2).min(bytes.len());
            continue;
        }
        if source[index..].starts_with("fleet.") {
            let start = index;
            index += "fleet.".len();
            while index < bytes.len()
                && (bytes[index] == b'_'
                    || bytes[index] == b'.'
                    || bytes[index].is_ascii_alphanumeric())
            {
                index += 1;
            }
            let mut next = index;
            while next < bytes.len() && bytes[next].is_ascii_whitespace() {
                next += 1;
            }
            if bytes.get(next) == Some(&b'(') {
                paths.push(source[start..index].to_owned());
            }
        } else {
            index += 1;
        }
    }
    paths
}

pub fn external_function_calls(source: &str) -> Vec<String> {
    let bytes = source.as_bytes();
    let mut calls = Vec::new();
    let mut declared = Vec::new();
    let mut index = 0;
    let mut previous_identifier = None::<String>;
    while index < bytes.len() {
        match bytes[index] {
            b'"' | b'\'' | b'`' => {
                let quote = bytes[index];
                index += 1;
                while index < bytes.len() {
                    if bytes[index] == b'\\' {
                        index = (index + 2).min(bytes.len());
                    } else if bytes[index] == quote {
                        index += 1;
                        break;
                    } else {
                        index += 1;
                    }
                }
            }
            b'/' if bytes.get(index + 1) == Some(&b'/') => {
                index += 2;
                while index < bytes.len() && bytes[index] != b'\n' {
                    index += 1;
                }
            }
            b'/' if bytes.get(index + 1) == Some(&b'*') => {
                index += 2;
                while index + 1 < bytes.len() && !(bytes[index] == b'*' && bytes[index + 1] == b'/')
                {
                    index += 1;
                }
                index = (index + 2).min(bytes.len());
            }
            byte if byte == b'_' || byte.is_ascii_alphabetic() => {
                let start = index;
                index += 1;
                while index < bytes.len()
                    && (bytes[index] == b'_' || bytes[index].is_ascii_alphanumeric())
                {
                    index += 1;
                }
                let identifier = &source[start..index];
                let mut next = index;
                while next < bytes.len() && bytes[next].is_ascii_whitespace() {
                    next += 1;
                }
                if previous_identifier.as_deref() == Some("fn") {
                    declared.push(identifier.to_owned());
                } else if bytes.get(next) == Some(&b'(') {
                    let mut previous = start;
                    while previous > 0 && bytes[previous - 1].is_ascii_whitespace() {
                        previous -= 1;
                    }
                    let is_method_or_qualified =
                        previous > 0 && matches!(bytes[previous - 1], b'.' | b':');
                    let is_keyword = matches!(
                        identifier,
                        "if" | "for" | "while" | "loop" | "switch" | "catch" | "throw"
                    );
                    if !is_method_or_qualified
                        && !is_keyword
                        && !declared.iter().any(|name| name == identifier)
                    {
                        calls.push(identifier.to_owned());
                    }
                }
                previous_identifier = Some(identifier.to_owned());
            }
            byte if byte.is_ascii_whitespace() => index += 1,
            _ => {
                previous_identifier = None;
                index += 1;
            }
        }
    }
    calls.retain(|call| !declared.iter().any(|name| name == call));
    calls
}

#[cfg(test)]
mod tests {
    use super::{qualified_function_calls, validate_available_apis};

    #[test]
    fn qualified_function_calls_ignore_strings_and_comments() {
        assert!(
            qualified_function_calls(r#""std::fs::hidden()"; // rhai::json::hidden()"#).is_empty()
        );
    }

    #[test]
    fn accepts_try_remove_file_api() {
        validate_available_apis("fn entry() { std::fs::try_remove_file(`x`) }").expect("shipped");
    }

    #[test]
    fn accepts_try_copy_and_rename_apis() {
        validate_available_apis(
            "fn entry() { std::fs::try_copy(`a`, `b`); std::fs::try_create_dir_all(`d`); std::fs::try_rename(`a`, `c`) }",
        )
        .expect("shipped");
    }

    #[test]
    fn rejects_unknown_api_call() {
        let error = validate_available_apis("fn entry() { std::fs::not_shipped(`x`) }")
            .expect_err("unknown API");
        assert_eq!(error.code, "script_api_unknown");
    }

    #[test]
    fn rejects_legacy_agent_methods() {
        let error = validate_available_apis("fn entry() { agent.workspace() }").expect_err("agent");
        assert_eq!(error.code, "script_api_migrated");
    }
}
