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

/// Install `context` for the duration of `run`, restoring whatever was there.
///
/// This used to clear the slot to `None` on the way out instead of restoring the
/// previous value, so a nested call silently destroyed the outer context: every
/// later `print` lost its capture and every later host read lost `project_root`
/// and `arguments`, with no error anywhere. Nesting is reachable -- the pack
/// probes and `rh eval` both install a context and can run underneath a worker
/// that already installed one -- so restore rather than clear. The guard makes
/// the restore hold even if `run` unwinds.
pub fn with_run_context<T>(context: RhRunContext, run: impl FnOnce() -> T) -> T {
    struct Restore(Option<RhRunContext>);
    impl Drop for Restore {
        fn drop(&mut self) {
            let previous = self.0.take();
            RUN_CONTEXT.with(|slot| *slot.borrow_mut() = previous);
        }
    }

    let _restore = Restore(RUN_CONTEXT.with(|slot| slot.borrow_mut().replace(context)));
    run()
}

pub(crate) fn current_run_context() -> Option<RhRunContext> {
    RUN_CONTEXT.with(|slot| slot.borrow().clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn context_with_root(root: &str) -> RhRunContext {
        RhRunContext {
            project_root: Some(PathBuf::from(root)),
            ..Default::default()
        }
    }

    fn current_root() -> Option<PathBuf> {
        current_run_context().and_then(|context| context.project_root)
    }

    #[test]
    fn a_nested_run_context_restores_the_outer_one_instead_of_clearing_it() {
        assert!(current_run_context().is_none(), "test started dirty");
        with_run_context(context_with_root("outer"), || {
            assert_eq!(current_root(), Some(PathBuf::from("outer")));
            with_run_context(context_with_root("inner"), || {
                assert_eq!(current_root(), Some(PathBuf::from("inner")));
            });
            assert_eq!(
                current_root(),
                Some(PathBuf::from("outer")),
                "the inner scope cleared the outer context: every later print \
                 loses its capture and every later host read loses project_root"
            );
        });
        assert!(
            current_run_context().is_none(),
            "the outermost scope must leave the slot empty"
        );
    }

    #[test]
    fn an_unwinding_body_still_restores_the_outer_context() {
        with_run_context(context_with_root("outer"), || {
            let panicked = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                with_run_context(context_with_root("inner"), || panic!("body failed"));
            }));
            assert!(panicked.is_err(), "the body was expected to unwind");
            assert_eq!(current_root(), Some(PathBuf::from("outer")));
        });
    }
}
