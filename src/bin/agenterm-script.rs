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
    SCRIPT_API_VERSION, SCRIPT_ENVELOPE_VERSION, ScriptFailure, ScriptInvocation, ScriptOperation,
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
                 Public scripts are invoked through `agentermctl script ...`."
            );
            Ok(0)
        }
        _ => anyhow::bail!(
            "unknown agenterm-script option; use --help or invoke scripts through agentermctl"
        ),
    }
}

fn run_worker() -> anyhow::Result<u8> {
    let mut input = String::new();
    std::io::stdin().read_to_string(&mut input)?;
    let invocation: ScriptInvocation = serde_json::from_str(&input)?;
    let result = execute(invocation);
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
        exit_class: "configuration".to_owned(),
        stdout: String::new(),
        value: None,
        failure: None,
        duration_ms: 0,
    };
    let execution = execute_inner(&invocation);
    match execution {
        Ok((stdout, value)) => {
            result.ok = true;
            result.exit_class = "success".to_owned();
            result.stdout = stdout;
            result.value = value;
        }
        Err(failure) => {
            result.exit_class = if failure.code.starts_with("limit_") {
                "limit"
            } else if failure.code.starts_with("script_") {
                "script"
            } else {
                "configuration"
            }
            .to_owned();
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
        return Err(failure(
            "unsupported_envelope",
            format!(
                "worker supports envelope {}, requested {}",
                SCRIPT_ENVELOPE_VERSION, invocation.envelope_version
            ),
        ));
    }
    if invocation.api_version != SCRIPT_API_VERSION {
        return Err(failure(
            "unsupported_api",
            format!(
                "worker supports API {}, requested {}",
                SCRIPT_API_VERSION, invocation.api_version
            ),
        ));
    }
    if invocation.source.len() > invocation.budgets.source_bytes {
        return Err(failure(
            "limit_source_bytes",
            "script source exceeds its byte budget",
        ));
    }
    if invocation.operation == ScriptOperation::Api {
        return Ok((String::new(), Some(api_catalog())));
    }

    let output = Arc::new(Mutex::new(String::new()));
    let output_exceeded = Arc::new(AtomicBool::new(false));
    let output_for_print = Arc::clone(&output);
    let exceeded_for_print = Arc::clone(&output_exceeded);
    let output_limit = invocation.budgets.output_bytes;
    let deadline = Instant::now()
        .checked_add(std::time::Duration::from_millis(
            invocation.budgets.wall_time_ms,
        ))
        .unwrap_or_else(Instant::now);
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
        (Instant::now() >= deadline).then(|| Dynamic::from("wall-time budget exceeded"))
    });

    let ast = engine
        .compile(&invocation.source)
        .map_err(|error| failure("script_parse", error.to_string()))?;
    if invocation.operation == ScriptOperation::Check {
        return Ok((String::new(), None));
    }

    let mut scope = Scope::new();
    let arguments = rhai::serde::to_dynamic(&invocation.arguments)
        .map_err(|error| failure("configuration_arguments", error.to_string()))?;
    scope.push_dynamic("args", arguments);
    match invocation.profile {
        ScriptProfile::Pure => {}
        ScriptProfile::Observe => {
            let Some(observation) = invocation.observation.as_ref() else {
                return Err(failure(
                    "configuration_observation",
                    "observe profile requires a brokered observation snapshot",
                ));
            };
            let observation = rhai::serde::to_dynamic(observation)
                .map_err(|error| failure("configuration_observation", error.to_string()))?;
            scope.push_dynamic("observe", observation);
        }
    }
    let value = engine
        .eval_ast_with_scope::<Dynamic>(&mut scope, &ast)
        .map_err(|error| {
            let message = error.to_string();
            if message.contains("Too many operations") {
                failure("limit_operations", message)
            } else if message.contains("wall-time budget exceeded") {
                failure("limit_wall_time", message)
            } else if output_exceeded.load(Ordering::Relaxed) {
                failure("limit_output_bytes", message)
            } else {
                failure("script_runtime", message)
            }
        })?;
    let value = if value.is_unit() {
        None
    } else {
        Some(
            rhai::serde::from_dynamic(&value)
                .map_err(|error| failure("script_result_type", error.to_string()))?,
        )
    };
    let stdout = output
        .lock()
        .map(|output| output.clone())
        .unwrap_or_default();
    if output_exceeded.load(Ordering::Relaxed) {
        return Err(failure(
            "limit_output_bytes",
            "script output reached its byte budget",
        ));
    }
    Ok((stdout, value))
}

fn api_catalog() -> serde_json::Value {
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
        "limits": agenterm::script_protocol::ScriptBudgets::default(),
        "deferred_capabilities": [
            "control", "fs.read", "fs.write", "env.read", "proc.exec", "network"
        ],
    })
}

fn failure(code: impl Into<String>, message: impl Into<String>) -> ScriptFailure {
    ScriptFailure {
        code: code.into(),
        message: message.into(),
    }
}
