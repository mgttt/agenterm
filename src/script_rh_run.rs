//! Per-invocation context for rh compat-delegating packs (args, project root).

use std::cell::RefCell;
use std::path::PathBuf;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};

use agenterm_rh::RhError;
use serde_json::Value;

use crate::script_protocol::ScriptBudgets;

#[derive(Debug)]
pub struct RhOutputCapture {
    text: Mutex<String>,
    limit: usize,
    exceeded: AtomicBool,
}

impl RhOutputCapture {
    pub fn new(limit: usize) -> Self {
        Self {
            text: Mutex::new(String::new()),
            limit,
            exceeded: AtomicBool::new(false),
        }
    }

    pub fn push_line(&self, text: &str) {
        let mut output = self.text.lock().expect("rh output lock poisoned");
        let remaining = self.limit.saturating_sub(output.len());
        if text.len().saturating_add(1) > remaining {
            self.exceeded.store(true, Ordering::Relaxed);
        }
        let mut take = text.len().min(remaining);
        while take > 0 && !text.is_char_boundary(take) {
            take -= 1;
        }
        output.push_str(&text[..take]);
        if output.len() < self.limit {
            output.push('\n');
        }
    }

    pub fn finish(&self) -> Result<String, RhError> {
        if self.exceeded.load(Ordering::Relaxed) {
            return Err(RhError::Compile(
                "rh invocation output exceeds its byte budget".into(),
            ));
        }
        Ok(self.text.lock().expect("rh output lock poisoned").clone())
    }
}

#[derive(Clone, Debug, Default)]
pub struct RhRunContext {
    pub project_root: Option<PathBuf>,
    pub arguments: Option<Value>,
    pub budgets: Option<ScriptBudgets>,
    pub output_capture: Option<Arc<RhOutputCapture>>,
}

thread_local! {
    static RUN_CONTEXT: RefCell<Option<RhRunContext>> = const { RefCell::new(None) };
}

pub fn with_run_context<T>(context: RhRunContext, run: impl FnOnce() -> T) -> T {
    RUN_CONTEXT.with(|slot| {
        *slot.borrow_mut() = Some(context);
        let output = run();
        *slot.borrow_mut() = None;
        output
    })
}

pub(crate) fn current_run_context() -> Option<RhRunContext> {
    RUN_CONTEXT.with(|slot| slot.borrow().clone())
}
