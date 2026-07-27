use std::{
    io::{Read, Write},
    process::ExitCode,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Instant,
};

use agenterm::script_protocol::{
    SCRIPT_API_VERSION, SCRIPT_ENVELOPE_VERSION, SCRIPT_INVOCATION_MAX_BYTES, ScriptBudgets,
    ScriptExitClass, ScriptFailure, ScriptFailureCategory, ScriptInvocation, ScriptOperation,
    ScriptProfile, ScriptResult,
};
use rhai::{Dynamic, Engine, Scope};

fn main() -> ExitCode {
    match run() {
        Ok(code) => ExitCode::from(code),
        Err(error) => {
            eprintln!("{error:#}");
            ExitCode::from(2)
        }
    }
}

fn run() -> anyhow::Result<u8> {
    let mut arguments = std::env::args().skip(1);
    match arguments.next().as_deref() {
        Some("--worker") if arguments.next().is_none() => run_worker(),
        Some("--version") if arguments.next().is_none() => {
            println!("agenterm-script {}", env!("CARGO_PKG_VERSION"));
            Ok(0)
        }
        Some("--help") | None => {
            println!(
                "AgenTerm scripting worker\n\
                 Usage: agenterm-script --worker\n\
                 Public scripts are invoked through `agenterm-cli script ...`."
            );
            Ok(0)
        }
        _ => anyhow::bail!(
            "unknown agenterm-script option; use --help or invoke scripts through agenterm-cli"
        ),
    }
}

fn run_worker() -> anyhow::Result<u8> {
    let mut input = Vec::new();
    std::io::stdin()
        .take(SCRIPT_INVOCATION_MAX_BYTES + 1)
        .read_to_end(&mut input)?;
    let result = if input.len() as u64 > SCRIPT_INVOCATION_MAX_BYTES {
        protocol_failure(
            "protocol_invocation_too_large",
            format!("invocation exceeds the {SCRIPT_INVOCATION_MAX_BYTES} byte protocol limit"),
        )
    } else {
        match serde_json::from_slice(&input) {
            Ok(invocation) => execute(invocation),
            Err(error) => protocol_failure("protocol_invalid_invocation", error.to_string()),
        }
    };
    serde_json::to_writer(std::io::stdout().lock(), &result)?;
    std::io::stdout().lock().write_all(b"\n")?;
    Ok(u8::from(!result.ok))
}

fn execute(invocation: ScriptInvocation) -> ScriptResult {
    let started = Instant::now();
    let mut result = ScriptResult {
        envelope_version: SCRIPT_ENVELOPE_VERSION,
        invocation_id: invocation.invocation_id.clone(),
        api_version: SCRIPT_API_VERSION,
        ok: false,
        exit_class: ScriptExitClass::Configuration,
        operation: Some(invocation.operation),
        profile: Some(invocation.profile),
        stdout: String::new(),
        value: None,
        failure: None,
        duration_ms: 0,
    };
    let execution = execute_inner(&invocation);
    match execution {
        Ok((stdout, value)) => {
            result.ok = true;
            result.exit_class = ScriptExitClass::Success;
            result.stdout = stdout;
            result.value = value;
        }
        Err(failure) => {
            result.exit_class = match failure.category {
                ScriptFailureCategory::Configuration => ScriptExitClass::Configuration,
                ScriptFailureCategory::Limit => ScriptExitClass::Limit,
                ScriptFailureCategory::Script => ScriptExitClass::Script,
                ScriptFailureCategory::Protocol => ScriptExitClass::Protocol,
            };
            result.failure = Some(failure);
        }
    }
    result.duration_ms = started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64;
    result
}

fn execute_inner(
    invocation: &ScriptInvocation,
) -> Result<(String, Option<serde_json::Value>), ScriptFailure> {
    if invocation.envelope_version != SCRIPT_ENVELOPE_VERSION {
        return Err(protocol_error(
            "unsupported_envelope",
            format!(
                "worker supports envelope {}, requested {}",
                SCRIPT_ENVELOPE_VERSION, invocation.envelope_version
            ),
        ));
    }
    if invocation.api_version != SCRIPT_API_VERSION {
        return Err(protocol_error(
            "unsupported_api",
            format!(
                "worker supports API {}, requested {}",
                SCRIPT_API_VERSION, invocation.api_version
            ),
        ));
    }
    validate_budgets(&invocation.budgets)?;
    if invocation.source.len() > invocation.budgets.source_bytes {
        return Err(limit_error(
            "limit_source_bytes",
            "script source exceeds its byte budget",
        ));
    }
    if invocation.operation == ScriptOperation::Api {
        return Ok((String::new(), Some(api_catalog())));
    }

    let output = Arc::new(Mutex::new(String::new()));
    let output_exceeded = Arc::new(AtomicBool::new(false));
    let wall_time_exceeded = Arc::new(AtomicBool::new(false));
    let output_for_print = Arc::clone(&output);
    let exceeded_for_print = Arc::clone(&output_exceeded);
    let output_limit = invocation.budgets.output_bytes;
    let deadline = Instant::now()
        .checked_add(std::time::Duration::from_millis(
            invocation.budgets.wall_time_ms,
        ))
        .unwrap_or_else(Instant::now);
    let wall_time_for_progress = Arc::clone(&wall_time_exceeded);
    let mut engine = Engine::new();
    engine.set_max_operations(invocation.budgets.operations);
    engine.set_max_call_levels(invocation.budgets.call_depth);
    engine.set_max_expr_depths(
        invocation.budgets.expression_depth,
        invocation.budgets.expression_depth,
    );
    engine.set_max_array_size(invocation.budgets.collection_items);
    engine.set_max_map_size(invocation.budgets.collection_items);
    engine.set_max_string_size(invocation.budgets.string_bytes);
    engine.on_print(move |text| {
        let mut output = output_for_print
            .lock()
            .expect("script output lock poisoned");
        let remaining = output_limit.saturating_sub(output.len());
        if text.len().saturating_add(1) > remaining {
            exceeded_for_print.store(true, Ordering::Relaxed);
        }
        let mut take = text.len().min(remaining);
        while take > 0 && !text.is_char_boundary(take) {
            take -= 1;
        }
        output.push_str(&text[..take]);
        if output.len() < output_limit {
            output.push('\n');
        }
    });
    engine.on_progress(move |_| {
        if Instant::now() >= deadline {
            wall_time_for_progress.store(true, Ordering::Relaxed);
            Some(Dynamic::from("wall-time budget exceeded"))
        } else {
            None
        }
    });

    let ast = engine
        .compile(&invocation.source)
        .map_err(|error| classify_compile_error(error.to_string()))?;
    if invocation.operation == ScriptOperation::Check {
        validate_profile_apis(&invocation.source, invocation.profile)?;
        return Ok((String::new(), None));
    }

    let mut scope = Scope::new();
    let arguments = rhai::serde::to_dynamic(&invocation.arguments)
        .map_err(|error| configuration_error("configuration_arguments", error.to_string()))?;
    scope.push_dynamic("args", arguments);
    match invocation.profile {
        ScriptProfile::Pure => {}
        ScriptProfile::Observe => {
            let Some(observation) = invocation.observation.as_ref() else {
                return Err(configuration_error(
                    "configuration_observation",
                    "observe profile requires a brokered observation snapshot",
                ));
            };
            let observation = rhai::serde::to_dynamic(observation).map_err(|error| {
                configuration_error("configuration_observation", error.to_string())
            })?;
            scope.push_dynamic("observe", observation);
        }
    }
    let value = engine
        .eval_ast_with_scope::<Dynamic>(&mut scope, &ast)
        .map_err(|error| {
            let message = error.to_string();
            if wall_time_exceeded.load(Ordering::Relaxed) {
                limit_error("limit_wall_time", message)
            } else if message.contains("Too many operations") {
                limit_error("limit_operations", message)
            } else if message.contains("exceeds the maximum")
                || message.contains("Maximum call stack depth")
                || message.contains("Stack overflow")
            {
                classify_engine_limit(message)
            } else if output_exceeded.load(Ordering::Relaxed) {
                limit_error("limit_output_bytes", message)
            } else {
                script_error("script_runtime", message)
            }
        })?;
    let value = if value.is_unit() {
        None
    } else {
        Some(
            rhai::serde::from_dynamic(&value)
                .map_err(|error| script_error("script_result_type", error.to_string()))?,
        )
    };
    let stdout = output
        .lock()
        .map(|output| output.clone())
        .unwrap_or_default();
    if output_exceeded.load(Ordering::Relaxed) {
        return Err(limit_error(
            "limit_output_bytes",
            "script output reached its byte budget",
        ));
    }
    Ok((stdout, value))
}

fn api_catalog() -> serde_json::Value {
    let defaults = ScriptBudgets::default();
    let hard_limits = ScriptBudgets::hard_limits();
    serde_json::json!({
        "schema_version": 1,
        "api_version": SCRIPT_API_VERSION,
        "profiles": {
            "pure": {
                "variables": ["args"],
                "ambient_authority": [],
            },
            "observe": {
                "variables": ["args", "observe"],
                "ambient_authority": [],
            },
        },
        "operations": ["api", "check", "eval", "run"],
        "limits": {
            "defaults": defaults,
            "hard_maximums": hard_limits,
            "invocation_bytes": SCRIPT_INVOCATION_MAX_BYTES,
        },
        "apis": [
            {
                "name": "print",
                "kind": "rhai_builtin",
                "profiles": ["pure", "observe"],
                "available": true,
            },
            {
                "name": "observe",
                "kind": "brokered_variable",
                "profiles": ["observe"],
                "available": true,
            },
            {
                "name": "new_tab",
                "kind": "control",
                "profiles": [],
                "available": false,
                "reason": "control capability is deferred",
            },
        ],
        "failure_categories": ["configuration", "limit", "script", "protocol"],
        "exit_classes": {
            "success": 0,
            "script": 1,
            "protocol": 1,
            "configuration": 2,
            "limit": 3,
        },
        "deferred_capabilities": [
            "control", "fs.read", "fs.write", "env.read", "proc.exec", "network"
        ],
    })
}

fn validate_budgets(budgets: &ScriptBudgets) -> Result<(), ScriptFailure> {
    let maximums = ScriptBudgets::hard_limits();
    macro_rules! validate {
        ($field:ident) => {
            if budgets.$field == 0 || budgets.$field > maximums.$field {
                return Err(configuration_error(
                    concat!("configuration_budget_", stringify!($field)),
                    format!(
                        "{} must be from 1 to {}",
                        stringify!($field),
                        maximums.$field
                    ),
                ));
            }
        };
    }
    validate!(source_bytes);
    validate!(operations);
    validate!(call_depth);
    validate!(expression_depth);
    validate!(collection_items);
    validate!(string_bytes);
    validate!(output_bytes);
    validate!(wall_time_ms);
    Ok(())
}

fn validate_profile_apis(source: &str, profile: ScriptProfile) -> Result<(), ScriptFailure> {
    for call in external_function_calls(source) {
        match call.as_str() {
            "print" | "debug" | "type_of" | "is_def_var" | "is_shared" | "eval" | "to_string"
            | "to_debug" => {}
            "new_tab" => {
                return Err(script_error(
                    "script_api_unavailable",
                    format!("API new_tab is unavailable in the {profile:?} profile"),
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

fn external_function_calls(source: &str) -> Vec<String> {
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
                    let is_method = previous > 0 && bytes[previous - 1] == b'.';
                    let is_keyword = matches!(
                        identifier,
                        "if" | "for" | "while" | "loop" | "switch" | "catch"
                    );
                    if !is_method && !is_keyword && !declared.iter().any(|name| name == identifier)
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

fn classify_engine_limit(message: String) -> ScriptFailure {
    let lowercase = message.to_ascii_lowercase();
    let code = if lowercase.contains("call") || lowercase.contains("stack") {
        "limit_call_depth"
    } else if lowercase.contains("expression") {
        "limit_expression_depth"
    } else if lowercase.contains("string") {
        "limit_string_bytes"
    } else {
        "limit_collection_items"
    };
    limit_error(code, message)
}

fn classify_compile_error(message: String) -> ScriptFailure {
    let lowercase = message.to_ascii_lowercase();
    if lowercase.contains("expression")
        && (lowercase.contains("depth") || lowercase.contains("complexity"))
    {
        limit_error("limit_expression_depth", message)
    } else if (lowercase.contains("array") || lowercase.contains("map"))
        && lowercase.contains("maximum")
    {
        limit_error("limit_collection_items", message)
    } else if lowercase.contains("string") && lowercase.contains("maximum") {
        limit_error("limit_string_bytes", message)
    } else {
        script_error("script_parse", message)
    }
}

fn protocol_failure(code: impl Into<String>, message: impl Into<String>) -> ScriptResult {
    ScriptResult {
        envelope_version: SCRIPT_ENVELOPE_VERSION,
        invocation_id: "unknown".to_owned(),
        api_version: SCRIPT_API_VERSION,
        ok: false,
        exit_class: ScriptExitClass::Protocol,
        operation: None,
        profile: None,
        stdout: String::new(),
        value: None,
        failure: Some(protocol_error(code, message)),
        duration_ms: 0,
    }
}

fn configuration_error(code: impl Into<String>, message: impl Into<String>) -> ScriptFailure {
    failure(code, message, ScriptFailureCategory::Configuration)
}

fn limit_error(code: impl Into<String>, message: impl Into<String>) -> ScriptFailure {
    failure(code, message, ScriptFailureCategory::Limit)
}

fn script_error(code: impl Into<String>, message: impl Into<String>) -> ScriptFailure {
    failure(code, message, ScriptFailureCategory::Script)
}

fn protocol_error(code: impl Into<String>, message: impl Into<String>) -> ScriptFailure {
    failure(code, message, ScriptFailureCategory::Protocol)
}

fn failure(
    code: impl Into<String>,
    message: impl Into<String>,
    category: ScriptFailureCategory,
) -> ScriptFailure {
    ScriptFailure {
        code: code.into(),
        message: message.into(),
        category,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn invocation(operation: ScriptOperation, source: &str) -> ScriptInvocation {
        ScriptInvocation {
            envelope_version: SCRIPT_ENVELOPE_VERSION,
            invocation_id: "unit-invocation".to_owned(),
            api_version: SCRIPT_API_VERSION,
            operation,
            profile: ScriptProfile::Pure,
            source_label: "unit".to_owned(),
            source: source.to_owned(),
            arguments: Vec::new(),
            budgets: ScriptBudgets::default(),
            observation: None,
        }
    }

    fn failure_code(result: &ScriptResult) -> &str {
        result
            .failure
            .as_ref()
            .map(|failure| failure.code.as_str())
            .expect("expected failure")
    }

    #[test]
    fn api_catalog_reports_defaults_maximums_availability_and_exit_classes() {
        let result = execute(invocation(ScriptOperation::Api, ""));
        assert!(result.ok);
        assert_eq!(result.operation, Some(ScriptOperation::Api));
        assert_eq!(result.profile, Some(ScriptProfile::Pure));
        let catalog = result.value.expect("API catalog");
        assert_eq!(
            catalog["limits"]["defaults"]["wall_time_ms"],
            ScriptBudgets::default().wall_time_ms
        );
        assert_eq!(
            catalog["limits"]["hard_maximums"]["wall_time_ms"],
            ScriptBudgets::hard_limits().wall_time_ms
        );
        assert_eq!(
            catalog["limits"]["invocation_bytes"],
            SCRIPT_INVOCATION_MAX_BYTES
        );
        assert_eq!(catalog["exit_classes"]["configuration"], 2);
        assert_eq!(catalog["exit_classes"]["limit"], 3);
        assert_eq!(catalog["apis"][2]["name"], "new_tab");
        assert_eq!(catalog["apis"][2]["available"], false);
    }

    #[test]
    fn check_rejects_unknown_and_unavailable_apis() {
        let unknown = execute(invocation(ScriptOperation::Check, "made_up_api()"));
        assert_eq!(failure_code(&unknown), "script_api_unknown");
        assert_eq!(unknown.exit_class, ScriptExitClass::Script);

        let unavailable = execute(invocation(ScriptOperation::Check, "new_tab()"));
        assert_eq!(failure_code(&unavailable), "script_api_unavailable");
        assert_eq!(unavailable.exit_class, ScriptExitClass::Script);
    }

    #[test]
    fn check_accepts_shipped_api_methods_and_user_functions() {
        let source = r#"
            fn twice(value) { value * 2 }
            let values = [1, 2];
            print(twice(values.len()));
        "#;
        assert!(execute(invocation(ScriptOperation::Check, source)).ok);
    }

    #[test]
    fn invalid_or_excessive_budget_is_configuration_failure() {
        let mut zero = invocation(ScriptOperation::Eval, "1");
        zero.budgets.operations = 0;
        let zero = execute(zero);
        assert_eq!(failure_code(&zero), "configuration_budget_operations");
        assert_eq!(zero.exit_class, ScriptExitClass::Configuration);

        let mut excessive = invocation(ScriptOperation::Eval, "1");
        excessive.budgets.output_bytes = ScriptBudgets::hard_limits().output_bytes + 1;
        assert_eq!(
            failure_code(&execute(excessive)),
            "configuration_budget_output_bytes"
        );
    }

    #[test]
    fn operation_output_and_wall_time_limits_are_typed() {
        let mut operations = invocation(ScriptOperation::Eval, "loop {}");
        operations.budgets.operations = 100;
        let operations = execute(operations);
        assert_eq!(failure_code(&operations), "limit_operations");
        assert_eq!(operations.exit_class, ScriptExitClass::Limit);

        let mut output = invocation(ScriptOperation::Eval, r#"print("abcdef")"#);
        output.budgets.output_bytes = 3;
        assert_eq!(failure_code(&execute(output)), "limit_output_bytes");

        let mut wall_time = invocation(ScriptOperation::Eval, "loop {}");
        wall_time.budgets.operations = ScriptBudgets::hard_limits().operations;
        wall_time.budgets.wall_time_ms = 1;
        let wall_time = execute(wall_time);
        assert_eq!(failure_code(&wall_time), "limit_wall_time");
        assert_eq!(wall_time.exit_class, ScriptExitClass::Limit);
    }

    #[test]
    fn source_and_expression_limits_are_typed() {
        let mut source = invocation(ScriptOperation::Check, "12345");
        source.budgets.source_bytes = 4;
        assert_eq!(failure_code(&execute(source)), "limit_source_bytes");

        let mut expression = invocation(
            ScriptOperation::Check,
            "((((((((((((((((((((((((((((((((1))))))))))))))))))))))))))))))))",
        );
        expression.budgets.expression_depth = 4;
        let expression = execute(expression);
        assert_eq!(
            failure_code(&expression),
            "limit_expression_depth",
            "{:?}",
            expression.failure
        );
    }

    #[test]
    fn call_collection_and_string_limits_are_typed() {
        let mut call_depth = invocation(
            ScriptOperation::Eval,
            "fn recurse() { recurse(); } recurse();",
        );
        call_depth.budgets.call_depth = 2;
        assert_eq!(failure_code(&execute(call_depth)), "limit_call_depth");

        let mut collection = invocation(ScriptOperation::Eval, "[1, 2, 3]");
        collection.budgets.collection_items = 2;
        let collection = execute(collection);
        assert_eq!(
            failure_code(&collection),
            "limit_collection_items",
            "{:?}",
            collection.failure
        );

        let mut string = invocation(ScriptOperation::Eval, r#""abcdef""#);
        string.budgets.string_bytes = 3;
        assert_eq!(failure_code(&execute(string)), "limit_string_bytes");
    }

    #[test]
    fn malformed_and_oversized_invocations_have_protocol_envelopes() {
        let malformed = protocol_failure("protocol_invalid_invocation", "bad JSON");
        assert!(!malformed.ok);
        assert_eq!(malformed.exit_class, ScriptExitClass::Protocol);
        assert_eq!(
            malformed
                .failure
                .as_ref()
                .expect("protocol failure")
                .category,
            ScriptFailureCategory::Protocol
        );
        assert!(malformed.operation.is_none());
        assert!(malformed.profile.is_none());

        let oversized = vec![0_u8; SCRIPT_INVOCATION_MAX_BYTES as usize + 1];
        assert!(oversized.len() as u64 > SCRIPT_INVOCATION_MAX_BYTES);
    }

    #[test]
    fn api_scanner_ignores_strings_comments_and_method_calls() {
        let source = r#"
            // hidden_api()
            let text = "also_hidden()";
            values.len();
            /* another_hidden() */
        "#;
        assert!(external_function_calls(source).is_empty());
    }

    #[test]
    fn api_scanner_accepts_forward_function_declarations() {
        let source = "twice(21); fn twice(value) { value * 2 }";
        assert!(external_function_calls(source).is_empty());
    }
}
