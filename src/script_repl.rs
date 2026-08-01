//! Reusable, persistent Rhai session core.
//!
//! This module deliberately owns no terminal UI. CLI, Control Center, tests,
//! and future agent-facing adapters can all drive the same transactional
//! language-state contract.

use std::{
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

use rhai::{AST, Dynamic, Engine, ParseError, ParseErrorType, Scope};
use serde::Serialize;
use serde_json::Value;

use crate::{
    script_fleet::{self, FleetBrokerCall},
    script_project::ProjectModuleResolver,
    script_protocol::ScriptBudgets,
    script_runtime::configure_engine,
    script_stdlib::{InvocationTempScope, enter_invocation_temp_root},
};

pub const REPL_MAX_HISTORY_ITEMS: usize = 1_000;
pub const REPL_MAX_HISTORY_BYTES: usize = 1024 * 1024;

#[derive(Clone, Default)]
pub struct ReplSessionConfig {
    pub budgets: ScriptBudgets,
    pub arguments: Vec<String>,
    pub project_root: Option<PathBuf>,
    pub invocation_temp_root: Option<PathBuf>,
    pub fleet: Option<FleetBrokerCall>,
}

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct ReplValue {
    pub type_name: String,
    pub serializable: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<Value>,
}

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct ReplCellResult {
    pub sequence: u64,
    pub ok: bool,
    pub stdout: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<ReplValue>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failure: Option<ReplCellFailure>,
    pub state_committed: bool,
    pub duration_ms: u64,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct ReplCellFailure {
    pub code: String,
    pub category: String,
    pub message: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReplInputState {
    Complete,
    Incomplete,
    Invalid(ReplCellFailure),
}

#[derive(Clone, Debug)]
pub struct ReplCancelHandle {
    epoch: Arc<AtomicU64>,
}

impl ReplCancelHandle {
    pub fn cancel(&self) {
        // Saturation is deliberately fail-closed: once MAX is reached every
        // current and future cell observes permanent cancellation.
        let _ = self
            .epoch
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |epoch| {
                if epoch == u64::MAX {
                    None
                } else {
                    Some(epoch + 1)
                }
            });
    }
}

#[derive(Default)]
struct CellControl {
    output: String,
    output_exceeded: bool,
    deadline: Option<Instant>,
    wall_time_exceeded: bool,
}

pub struct ReplSession {
    engine: Engine,
    scope: Scope<'static>,
    initial_scope: Scope<'static>,
    functions: AST,
    budgets: ScriptBudgets,
    control: Arc<Mutex<CellControl>>,
    cancel_epoch: Arc<AtomicU64>,
    active_cancel_baseline: Arc<AtomicU64>,
    sequence: u64,
    history: Vec<String>,
    history_bytes: usize,
    _temp_scope: InvocationTempScope,
}

impl ReplSession {
    pub fn new(config: ReplSessionConfig) -> Result<Self, String> {
        validate_budgets(&config.budgets)?;
        let temp_scope = enter_invocation_temp_root(config.invocation_temp_root.as_deref());
        let control = Arc::new(Mutex::new(CellControl::default()));
        let cancel_epoch = Arc::new(AtomicU64::new(0));
        let active_cancel_baseline = Arc::new(AtomicU64::new(0));
        let mut engine = build_engine(
            &config.budgets,
            Arc::clone(&control),
            Arc::clone(&cancel_epoch),
            Arc::clone(&active_cancel_baseline),
        );
        if let Some(root) = config.project_root.as_deref() {
            engine.set_module_resolver(
                ProjectModuleResolver::new(root)
                    .map_err(|error| format!("script_project_root: {error}"))?,
            );
        }

        let mut scope = Scope::new();
        let arguments = rhai::serde::to_dynamic(&config.arguments)
            .map_err(|error| format!("configuration_arguments: {error}"))?;
        scope.push_dynamic("args", arguments);
        if let Some(call) = config.fleet {
            scope.push("fleet", script_fleet::bind(call));
        }
        let initial_scope = scope.clone_visible();

        Ok(Self {
            engine,
            scope,
            initial_scope,
            functions: AST::empty(),
            budgets: config.budgets,
            control,
            cancel_epoch,
            active_cancel_baseline,
            sequence: 0,
            history: Vec::new(),
            history_bytes: 0,
            _temp_scope: temp_scope,
        })
    }

    pub fn inspect_input(&self, source: &str) -> ReplInputState {
        if source.len() > self.budgets.source_bytes {
            return ReplInputState::Invalid(failure(
                "limit_source_bytes",
                "limit",
                "REPL cell source exceeds its byte budget",
            ));
        }
        if source_structure_is_incomplete(source) {
            return ReplInputState::Incomplete;
        }
        match self.engine.compile_with_scope(&self.scope, source) {
            Ok(_) => ReplInputState::Complete,
            Err(error) if parse_error_is_incomplete(&error) => ReplInputState::Incomplete,
            Err(error) => ReplInputState::Invalid(compile_failure(error)),
        }
    }

    pub fn evaluate_cell(&mut self, source: &str) -> ReplCellResult {
        let baseline = self.capture_cancel_epoch();
        self.evaluate_with_baseline(source, baseline)
    }

    pub fn capture_cancel_epoch(&self) -> u64 {
        self.cancel_epoch.load(Ordering::Acquire)
    }

    pub fn cancel_handle(&self) -> ReplCancelHandle {
        ReplCancelHandle {
            epoch: Arc::clone(&self.cancel_epoch),
        }
    }

    pub fn evaluate_with_baseline(&mut self, source: &str, baseline: u64) -> ReplCellResult {
        self.sequence = self.sequence.saturating_add(1);
        let sequence = self.sequence;
        let started = Instant::now();
        self.reset_cell_control(baseline);
        if self.cancellation_requested(baseline) {
            return failed_result(sequence, started, cancellation_failure(), String::new());
        }
        if source.len() > self.budgets.source_bytes {
            return failed_result(
                sequence,
                started,
                failure(
                    "limit_source_bytes",
                    "limit",
                    "REPL cell source exceeds its byte budget",
                ),
                String::new(),
            );
        }

        if let Err(error) = self.engine.compile_with_scope(&self.scope, source) {
            if self.cancellation_requested(baseline) {
                return failed_result(
                    sequence,
                    started,
                    cancellation_failure(),
                    self.take_output(),
                );
            }
            return failed_result(
                sequence,
                started,
                compile_failure(error),
                self.take_output(),
            );
        }
        let cell_ast = match self.engine.compile_into_self_contained(&self.scope, source) {
            Ok(ast) => ast,
            Err(error) => {
                if self.cancellation_requested(baseline) {
                    return failed_result(
                        sequence,
                        started,
                        cancellation_failure(),
                        self.take_output(),
                    );
                }
                return failed_result(
                    sequence,
                    started,
                    failure("script_compile", "script", error.to_string()),
                    self.take_output(),
                );
            }
        };
        let candidate_ast = self.functions.merge(&cell_ast);
        let mut candidate_scope = self.scope.clone_visible();
        let evaluation = self
            .engine
            .eval_ast_with_scope::<Dynamic>(&mut candidate_scope, &candidate_ast);
        let output = self.take_output();
        let cancelled = self.cancellation_requested(baseline);
        match evaluation {
            Ok(value) => {
                if cancelled {
                    return failed_result(sequence, started, cancellation_failure(), output);
                }
                if self.output_exceeded() {
                    return failed_result(
                        sequence,
                        started,
                        failure(
                            "limit_output_bytes",
                            "limit",
                            "REPL cell output reached its byte budget",
                        ),
                        output,
                    );
                }
                self.scope = candidate_scope.clone_visible();
                self.functions = candidate_ast.clone_functions_only();
                self.remember(source);
                ReplCellResult {
                    sequence,
                    ok: true,
                    stdout: output,
                    value: dynamic_value(&value),
                    failure: None,
                    state_committed: true,
                    duration_ms: duration_ms(started),
                }
            }
            Err(error) => failed_result(
                sequence,
                started,
                runtime_failure(
                    &error,
                    cancelled,
                    self.wall_time_exceeded(),
                    self.output_exceeded(),
                ),
                output,
            ),
        }
    }

    pub fn reset(&mut self) {
        self.scope = self.initial_scope.clone_visible();
        self.functions = AST::empty();
        self.reset_cell_control(self.capture_cancel_epoch());
    }

    pub fn cancel(&self) {
        self.cancel_handle().cancel();
    }

    pub fn variables(&self) -> Vec<(String, String)> {
        let mut variables = self
            .scope
            .iter_raw()
            .map(|(name, _, value)| (name.to_owned(), value.type_name().to_owned()))
            .collect::<Vec<_>>();
        variables.sort();
        variables.dedup_by(|left, right| left.0 == right.0);
        variables
    }

    pub fn functions(&self) -> Vec<String> {
        let mut functions = self
            .functions
            .iter_functions()
            .map(|function| function.to_string())
            .collect::<Vec<_>>();
        functions.sort();
        functions
    }

    pub fn history(&self) -> &[String] {
        &self.history
    }

    pub fn budgets(&self) -> &ScriptBudgets {
        &self.budgets
    }

    fn reset_cell_control(&self, baseline: u64) {
        self.active_cancel_baseline
            .store(baseline, Ordering::Release);
        let mut control = self.control.lock().expect("REPL cell control poisoned");
        control.output.clear();
        control.output_exceeded = false;
        control.wall_time_exceeded = false;
        control.deadline =
            Instant::now().checked_add(Duration::from_millis(self.budgets.wall_time_ms));
    }

    fn cancellation_requested(&self, baseline: u64) -> bool {
        let epoch = self.cancel_epoch.load(Ordering::Acquire);
        epoch == u64::MAX || epoch != baseline
    }

    fn take_output(&self) -> String {
        self.control
            .lock()
            .map(|control| control.output.clone())
            .unwrap_or_default()
    }

    fn output_exceeded(&self) -> bool {
        self.control
            .lock()
            .is_ok_and(|control| control.output_exceeded)
    }

    fn wall_time_exceeded(&self) -> bool {
        self.control
            .lock()
            .is_ok_and(|control| control.wall_time_exceeded)
    }

    fn remember(&mut self, source: &str) {
        if source.trim().is_empty() || source.len() > REPL_MAX_HISTORY_BYTES {
            return;
        }
        while self.history.len() >= REPL_MAX_HISTORY_ITEMS
            || self.history_bytes.saturating_add(source.len()) > REPL_MAX_HISTORY_BYTES
        {
            if self.history.is_empty() {
                return;
            }
            self.history_bytes = self
                .history_bytes
                .saturating_sub(self.history.remove(0).len());
        }
        self.history.push(source.to_owned());
        self.history_bytes = self.history_bytes.saturating_add(source.len());
    }
}

fn build_engine(
    budgets: &ScriptBudgets,
    control: Arc<Mutex<CellControl>>,
    cancel_epoch: Arc<AtomicU64>,
    active_cancel_baseline: Arc<AtomicU64>,
) -> Engine {
    let mut engine = Engine::new();
    configure_engine(&mut engine, budgets);

    let print_control = Arc::clone(&control);
    let output_limit = budgets.output_bytes;
    engine.on_print(move |text| {
        let mut state = print_control.lock().expect("REPL output lock poisoned");
        let remaining = output_limit.saturating_sub(state.output.len());
        if text.len().saturating_add(1) > remaining {
            state.output_exceeded = true;
        }
        let mut take = text.len().min(remaining);
        while take > 0 && !text.is_char_boundary(take) {
            take -= 1;
        }
        state.output.push_str(&text[..take]);
        if state.output.len() < output_limit {
            state.output.push('\n');
        }
    });
    engine.on_progress(move |_| {
        let epoch = cancel_epoch.load(Ordering::Acquire);
        let baseline = active_cancel_baseline.load(Ordering::Acquire);
        if epoch == u64::MAX || epoch != baseline {
            return Some(Dynamic::from("REPL cell cancellation requested"));
        }
        let mut state = control.lock().expect("REPL progress lock poisoned");
        if state.output_exceeded {
            return Some(Dynamic::from("REPL output byte budget exceeded"));
        }
        if state
            .deadline
            .is_some_and(|deadline| Instant::now() >= deadline)
        {
            state.wall_time_exceeded = true;
            return Some(Dynamic::from("REPL wall-time budget exceeded"));
        }
        None
    });
    engine
}

fn validate_budgets(budgets: &ScriptBudgets) -> Result<(), String> {
    let limits = ScriptBudgets::hard_limits();
    macro_rules! validate {
        ($field:ident) => {
            if budgets.$field == 0 || budgets.$field > limits.$field {
                return Err(format!(
                    "configuration_budget: {} must be from 1 to {}",
                    stringify!($field),
                    limits.$field
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

fn parse_error_is_incomplete(error: &ParseError) -> bool {
    match error.err_type() {
        ParseErrorType::UnexpectedEOF => true,
        ParseErrorType::MissingToken(token, _) => {
            matches!(token.as_str(), "}" | ")" | "]" | "`" | "\"")
        }
        _ => false,
    }
}

fn source_structure_is_incomplete(source: &str) -> bool {
    let mut delimiters = Vec::new();
    let mut chars = source.chars().peekable();
    let mut quoted = None;
    let mut escaped = false;
    let mut block_comment_depth = 0_usize;
    while let Some(character) = chars.next() {
        if block_comment_depth > 0 {
            match (character, chars.peek().copied()) {
                ('/', Some('*')) => {
                    chars.next();
                    block_comment_depth += 1;
                }
                ('*', Some('/')) => {
                    chars.next();
                    block_comment_depth -= 1;
                }
                _ => {}
            }
            continue;
        }
        if let Some(quote) = quoted {
            if escaped {
                escaped = false;
            } else if character == '\\' {
                escaped = true;
            } else if character == quote {
                quoted = None;
            }
            continue;
        }
        match (character, chars.peek().copied()) {
            ('/', Some('/')) => {
                chars.next();
                for next in chars.by_ref() {
                    if next == '\n' {
                        break;
                    }
                }
            }
            ('/', Some('*')) => {
                chars.next();
                block_comment_depth = 1;
            }
            ('"' | '`', _) => quoted = Some(character),
            ('{' | '(' | '[', _) => delimiters.push(character),
            ('}', _) if delimiters.last() == Some(&'{') => {
                delimiters.pop();
            }
            (')', _) if delimiters.last() == Some(&'(') => {
                delimiters.pop();
            }
            (']', _) if delimiters.last() == Some(&'[') => {
                delimiters.pop();
            }
            _ => {}
        }
    }
    !delimiters.is_empty() || quoted.is_some() || block_comment_depth > 0
}

fn compile_failure(error: ParseError) -> ReplCellFailure {
    failure("script_compile", "script", error.to_string())
}

fn runtime_failure(
    error: &rhai::EvalAltResult,
    cancelled: bool,
    wall_time_exceeded: bool,
    output_exceeded: bool,
) -> ReplCellFailure {
    let message = error.to_string();
    if cancelled {
        failure("limit_cancelled", "cancelled", message)
    } else if wall_time_exceeded {
        failure("limit_wall_time", "limit", message)
    } else if output_exceeded {
        failure("limit_output_bytes", "limit", message)
    } else if message.contains("Too many operations") {
        failure("limit_operations", "limit", message)
    } else if message.contains("exceeds the maximum")
        || message.contains("Maximum call stack depth")
        || message.contains("Stack overflow")
    {
        failure("limit_engine", "limit", message)
    } else {
        failure("script_runtime", "script", message)
    }
}

fn cancellation_failure() -> ReplCellFailure {
    failure(
        "limit_cancelled",
        "cancelled",
        "REPL cell cancellation requested",
    )
}

fn dynamic_value(value: &Dynamic) -> Option<ReplValue> {
    if value.is_unit() {
        return None;
    }
    let serialized = rhai::serde::from_dynamic::<Value>(value).ok();
    Some(ReplValue {
        type_name: value.type_name().to_owned(),
        serializable: serialized.is_some(),
        value: serialized,
    })
}

fn failure(
    code: impl Into<String>,
    category: impl Into<String>,
    message: impl Into<String>,
) -> ReplCellFailure {
    ReplCellFailure {
        code: code.into(),
        category: category.into(),
        message: message.into(),
    }
}

fn failed_result(
    sequence: u64,
    started: Instant,
    failure: ReplCellFailure,
    stdout: String,
) -> ReplCellResult {
    ReplCellResult {
        sequence,
        ok: false,
        stdout,
        value: None,
        failure: Some(failure),
        state_committed: false,
        duration_ms: duration_ms(started),
    }
}

fn duration_ms(started: Instant) -> u64 {
    started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64
}

pub fn canonical_project_root(path: Option<&str>) -> Result<Option<PathBuf>, String> {
    path.map(|path| {
        let path = Path::new(path);
        std::fs::canonicalize(path)
            .map_err(|error| format!("script_project_root: {}: {error}", path.display()))
            .and_then(|canonical| {
                if canonical.is_dir() {
                    Ok(canonical)
                } else {
                    Err(format!(
                        "script_project_root: {} is not a directory",
                        canonical.display()
                    ))
                }
            })
    })
    .transpose()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{sync::mpsc, thread};

    fn session() -> ReplSession {
        ReplSession::new(ReplSessionConfig::default()).unwrap()
    }

    #[test]
    fn variables_and_functions_persist_across_cells() {
        let mut session = session();
        assert!(session.evaluate_cell("let x = 40;").ok);
        assert!(session.evaluate_cell("fn add(n) { 40 + n }").ok);
        let result = session.evaluate_cell("add(2)");
        assert!(result.ok, "{result:?}");
        assert_eq!(result.value.unwrap().value, Some(Value::from(42)));
    }

    #[test]
    fn failed_cell_does_not_commit_language_state() {
        let mut session = session();
        assert!(session.evaluate_cell("let x = 1; fn f() { 1 }").ok);
        assert!(
            !session
                .evaluate_cell("x = 9; fn f() { 99 } throw \"no\";")
                .ok
        );
        let result = session.evaluate_cell("[x, f()]");
        assert!(result.ok, "{result:?}");
        assert_eq!(result.value.unwrap().value, Some(serde_json::json!([1, 1])));
    }

    #[test]
    fn reset_removes_user_state_but_preserves_prelude() {
        let mut session = session();
        assert!(session.evaluate_cell("let x = 1; fn f() { 2 }").ok);
        session.reset();
        assert!(!session.evaluate_cell("x").ok);
        assert!(session.variables().iter().any(|(name, _)| name == "args"));
        assert!(session.functions().is_empty());
    }

    #[test]
    fn incomplete_input_is_structured() {
        let session = session();
        assert_eq!(
            session.inspect_input("fn add(x) {"),
            ReplInputState::Incomplete
        );
        assert!(matches!(
            session.inspect_input("let = 1"),
            ReplInputState::Invalid(_)
        ));
    }

    #[test]
    fn output_limit_is_utf8_safe_and_recoverable() {
        let mut config = ReplSessionConfig::default();
        config.budgets.output_bytes = 5;
        let mut session = ReplSession::new(config).unwrap();
        let failed = session.evaluate_cell("print(\"你好世界\")");
        assert!(!failed.ok);
        assert!(failed.stdout.is_char_boundary(failed.stdout.len()));
        assert!(session.evaluate_cell("40 + 2").ok);
    }

    #[test]
    fn cancellation_after_captured_baseline_is_not_lost_at_cell_start() {
        let mut session = session();
        let stale_baseline = session.capture_cancel_epoch();
        session.cancel();

        let cancelled = session.evaluate_with_baseline("40 + 2", stale_baseline);
        assert!(!cancelled.ok);
        assert_eq!(
            cancelled
                .failure
                .as_ref()
                .map(|failure| failure.code.as_str()),
            Some("limit_cancelled")
        );
        assert!(!cancelled.state_committed);

        let recovered = session.evaluate_cell("40 + 2");
        assert!(recovered.ok, "{recovered:?}");
        assert_eq!(recovered.value.unwrap().value, Some(Value::from(42)));
    }

    #[test]
    fn cpu_loop_observes_cooperative_cancellation() {
        let (control_tx, control_rx) = mpsc::channel();
        let (started_tx, started_rx) = mpsc::channel();

        let worker = thread::spawn(move || {
            let mut config = ReplSessionConfig::default();
            config.budgets.operations = ScriptBudgets::hard_limits().operations;
            config.budgets.wall_time_ms = 5_000;
            let mut session = ReplSession::new(config).unwrap();
            session.engine.register_fn("mark_started", move || {
                started_tx.send(()).unwrap();
            });
            let baseline = session.capture_cancel_epoch();
            control_tx.send(session.cancel_handle()).unwrap();
            session.evaluate_with_baseline("mark_started(); while true {}", baseline)
        });
        let cancel = control_rx.recv().unwrap();
        started_rx.recv().unwrap();
        cancel.cancel();

        let cancelled = worker.join().unwrap();
        assert!(!cancelled.ok);
        assert_eq!(
            cancelled
                .failure
                .as_ref()
                .map(|failure| failure.code.as_str()),
            Some("limit_cancelled")
        );
        assert!(!cancelled.state_committed);
    }

    #[test]
    fn saturated_cancel_epoch_is_permanently_cancelled() {
        let mut session = session();
        session.cancel_epoch.store(u64::MAX, Ordering::Release);
        session.cancel();

        let cancelled = session.evaluate_cell("40 + 2");
        assert_eq!(session.capture_cancel_epoch(), u64::MAX);
        assert_eq!(
            cancelled
                .failure
                .as_ref()
                .map(|failure| failure.code.as_str()),
            Some("limit_cancelled")
        );
    }
}
