//! Per-invocation context for Lua scripts (args, project root, budgets).

use std::cell::RefCell;
use std::path::PathBuf;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};

use serde_json::Value;

use crate::script_protocol::ScriptBudgets;

#[derive(Debug)]
pub struct LuaOutputCapture {
    text: Mutex<String>,
    limit: usize,
    exceeded: AtomicBool,
}

impl LuaOutputCapture {
    pub fn new(limit: usize) -> Self {
        Self {
            text: Mutex::new(String::new()),
            limit,
            exceeded: AtomicBool::new(false),
        }
    }

    pub fn push_line(&self, text: &str) {
        let mut output = self.text.lock().expect("lua output lock poisoned");
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

    pub fn finish(&self) -> Result<String, String> {
        if self.exceeded.load(Ordering::Relaxed) {
            return Err("lua invocation output exceeds its byte budget".into());
        }
        Ok(self.text.lock().expect("lua output lock poisoned").clone())
    }
}

#[derive(Clone, Debug, Default)]
pub struct LuaRunContext {
    pub project_root: Option<PathBuf>,
    pub arguments: Option<Value>,
    pub budgets: Option<ScriptBudgets>,
    pub output_capture: Option<Arc<LuaOutputCapture>>,
}

thread_local! {
    static RUN_CONTEXT: RefCell<Option<LuaRunContext>> = const { RefCell::new(None) };
}

pub fn with_run_context<T>(context: LuaRunContext, run: impl FnOnce() -> T) -> T {
    RUN_CONTEXT.with(|slot| {
        *slot.borrow_mut() = Some(context);
        let output = run();
        *slot.borrow_mut() = None;
        output
    })
}

pub(crate) fn current_run_context() -> Option<LuaRunContext> {
    RUN_CONTEXT.with(|slot| slot.borrow().clone())
}
