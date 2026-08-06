//! Per-invocation context for rh compat-delegating packs (args, project root).

use std::cell::RefCell;
use std::path::PathBuf;

use serde_json::Value;

use crate::script_protocol::ScriptBudgets;

#[derive(Clone, Debug, Default)]
pub struct RhRunContext {
    pub project_root: Option<PathBuf>,
    pub arguments: Option<Value>,
    pub budgets: Option<ScriptBudgets>,
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
