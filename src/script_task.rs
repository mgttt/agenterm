use std::{
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicU64, AtomicUsize, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

use rhai::{Array, Dynamic, Engine, EvalAltResult, Module};

use crate::script_process::ScriptDuration;

const MAX_TASKS_PER_CALL: usize = 64;
pub const MAX_ACTIVE_TASKS: usize = 64;
static NEXT_TASK_ID: AtomicU64 = AtomicU64::new(1);
static ACTIVE_TASKS: AtomicUsize = AtomicUsize::new(0);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum TaskStatus {
    Pending,
    Completed,
    Failed,
    Cancelled,
}

#[derive(Clone, Debug)]
pub(crate) enum ScriptTaskPayload {
    Unit,
    Http(crate::script_http::ScriptHttpResponse),
}

#[derive(Debug)]
struct TaskState {
    status: TaskStatus,
    payload: Option<ScriptTaskPayload>,
    failure_code: Option<String>,
}

#[derive(Debug)]
struct TaskInner {
    id: u64,
    kind: &'static str,
    state: Mutex<TaskState>,
    changed: Condvar,
}

#[derive(Clone, Debug)]
pub struct ScriptTask(Arc<TaskInner>);

impl Drop for ScriptTask {
    fn drop(&mut self) {
        // The timer thread owns one Arc. When the last script-visible handle
        // disappears, cancel and wake that thread instead of leaving work
        // running until the worker process exits.
        if Arc::strong_count(&self.0) <= 2 {
            let _ = cancel_inner(&self.0);
        }
    }
}

pub(crate) fn register(engine: &mut Engine, rhai_module: &mut Module) {
    engine.register_type_with_name::<ScriptTask>("Task");
    engine.register_get("id", task_id);
    engine.register_get("kind", task_kind);
    engine.register_get("state", task_state);
    engine.register_get("done", task_done);
    engine.register_get("cancelled", task_cancelled);
    engine.register_fn("wait", task_wait);
    engine.register_fn("wait", task_wait_for);
    engine.register_fn("cancel", task_cancel);

    let mut task = Module::new();
    task.set_native_fn("after", task_after);
    task.set_native_fn("sleep", task_sleep);
    task.set_native_fn("wait_all", task_wait_all);
    task.set_native_fn("wait_all", task_wait_all_for);
    task.set_native_fn("race", task_race);
    task.set_native_fn("race", task_race_for);
    task.set_native_fn("cancel_all", task_cancel_all);
    rhai_module.set_sub_module("task", task);
}

fn task_after(duration: ScriptDuration) -> Result<ScriptTask, Box<EvalAltResult>> {
    let permit = acquire_task_permit()?;
    let inner = Arc::new(TaskInner {
        id: NEXT_TASK_ID.fetch_add(1, Ordering::Relaxed),
        kind: "timer",
        state: Mutex::new(TaskState {
            status: TaskStatus::Pending,
            payload: None,
            failure_code: None,
        }),
        changed: Condvar::new(),
    });
    let timer = Arc::clone(&inner);
    thread::Builder::new()
        .name("agenterm-task-timer".to_owned())
        .spawn(move || {
            let _permit = permit;
            let state = match timer.state.lock() {
                Ok(state) => state,
                Err(_) => return,
            };
            let result = timer
                .changed
                .wait_timeout_while(state, duration.0, |state| {
                    state.status == TaskStatus::Pending
                });
            let Ok((mut state, timeout)) = result else {
                return;
            };
            if timeout.timed_out() && state.status == TaskStatus::Pending {
                state.status = TaskStatus::Completed;
                state.payload = Some(ScriptTaskPayload::Unit);
                timer.changed.notify_all();
            }
        })
        .map_err(|_| -> Box<EvalAltResult> {
            "task_spawn_failed: timer worker could not be created".into()
        })?;
    Ok(ScriptTask(inner))
}

fn task_sleep(duration: ScriptDuration) -> Result<(), Box<EvalAltResult>> {
    wait_inner(&task_after(duration)?.0, None).map(|_| ())
}

fn task_id(task: &mut ScriptTask) -> Result<rhai::INT, Box<EvalAltResult>> {
    rhai::INT::try_from(task.0.id).map_err(|_| "task_id_overflow".into())
}

fn task_kind(task: &mut ScriptTask) -> String {
    task.0.kind.to_owned()
}

fn task_state(task: &mut ScriptTask) -> Result<String, Box<EvalAltResult>> {
    let status = task
        .0
        .state
        .lock()
        .map_err(|_| "task_state_poisoned")?
        .status;
    Ok(status_name(status).to_owned())
}

fn task_done(task: &mut ScriptTask) -> Result<bool, Box<EvalAltResult>> {
    Ok(task
        .0
        .state
        .lock()
        .map_err(|_| "task_state_poisoned")?
        .status
        != TaskStatus::Pending)
}

fn task_cancelled(task: &mut ScriptTask) -> Result<bool, Box<EvalAltResult>> {
    Ok(task
        .0
        .state
        .lock()
        .map_err(|_| "task_state_poisoned")?
        .status
        == TaskStatus::Cancelled)
}

fn task_wait(task: &mut ScriptTask) -> Result<Dynamic, Box<EvalAltResult>> {
    wait_inner(&task.0, None).map(payload_dynamic)
}

fn task_wait_for(
    task: &mut ScriptTask,
    timeout: ScriptDuration,
) -> Result<Dynamic, Box<EvalAltResult>> {
    wait_inner(&task.0, Some(timeout.0)).map(payload_dynamic)
}

fn task_cancel(task: &mut ScriptTask) -> Result<bool, Box<EvalAltResult>> {
    cancel_inner(&task.0)
}

fn cancel_inner(task: &Arc<TaskInner>) -> Result<bool, Box<EvalAltResult>> {
    let mut state = task.state.lock().map_err(|_| "task_state_poisoned")?;
    if state.status != TaskStatus::Pending {
        return Ok(false);
    }
    state.status = TaskStatus::Cancelled;
    task.changed.notify_all();
    Ok(true)
}

fn wait_inner(
    task: &Arc<TaskInner>,
    timeout: Option<Duration>,
) -> Result<ScriptTaskPayload, Box<EvalAltResult>> {
    let state = task.state.lock().map_err(|_| "task_state_poisoned")?;
    let state = if let Some(timeout) = timeout {
        let (state, timed) = task
            .changed
            .wait_timeout_while(state, timeout, |state| state.status == TaskStatus::Pending)
            .map_err(|_| "task_state_poisoned")?;
        if timed.timed_out() && state.status == TaskStatus::Pending {
            return Err("task_wait_timeout: task remained pending".into());
        }
        state
    } else {
        task.changed
            .wait_while(state, |state| state.status == TaskStatus::Pending)
            .map_err(|_| "task_state_poisoned")?
    };
    match state.status {
        TaskStatus::Completed => state
            .payload
            .clone()
            .ok_or_else(|| "task_invariant: completed task has no payload".into()),
        TaskStatus::Failed => Err(format!(
            "task_failed: {}",
            state.failure_code.as_deref().unwrap_or("task_host_failure")
        )
        .into()),
        TaskStatus::Cancelled => Err("task_cancelled: task was cancelled".into()),
        TaskStatus::Pending => Err("task_invariant: wait returned while pending".into()),
    }
}

fn task_wait_all(tasks: Array) -> Result<Array, Box<EvalAltResult>> {
    wait_all(tasks, None)
}

fn task_wait_all_for(tasks: Array, timeout: ScriptDuration) -> Result<Array, Box<EvalAltResult>> {
    wait_all(tasks, Some(timeout.0))
}

fn wait_all(tasks: Array, timeout: Option<Duration>) -> Result<Array, Box<EvalAltResult>> {
    let tasks = typed_tasks(tasks)?;
    let deadline = timeout.map(|timeout| Instant::now() + timeout);
    let mut results = Array::with_capacity(tasks.len());
    for task in tasks {
        let remaining = deadline.map(|deadline| deadline.saturating_duration_since(Instant::now()));
        results.push(payload_dynamic(wait_inner(&task.0, remaining)?));
    }
    Ok(results)
}

fn task_race(tasks: Array) -> Result<rhai::INT, Box<EvalAltResult>> {
    race(tasks, None)
}

fn task_race_for(tasks: Array, timeout: ScriptDuration) -> Result<rhai::INT, Box<EvalAltResult>> {
    race(tasks, Some(timeout.0))
}

fn race(tasks: Array, timeout: Option<Duration>) -> Result<rhai::INT, Box<EvalAltResult>> {
    let tasks = typed_tasks(tasks)?;
    if tasks.is_empty() {
        return Err("task_race_empty: at least one task is required".into());
    }
    let deadline = timeout.map(|timeout| Instant::now() + timeout);
    loop {
        for (index, task) in tasks.iter().enumerate() {
            let status = task
                .0
                .state
                .lock()
                .map_err(|_| "task_state_poisoned")?
                .status;
            if status == TaskStatus::Completed {
                return rhai::INT::try_from(index).map_err(|_| "task_index_overflow".into());
            }
            if status == TaskStatus::Failed {
                let state = task.0.state.lock().map_err(|_| "task_state_poisoned")?;
                return Err(format!(
                    "task_failed: raced task {index} failed with {}",
                    state.failure_code.as_deref().unwrap_or("task_host_failure")
                )
                .into());
            }
            if status == TaskStatus::Cancelled {
                return Err(format!("task_cancelled: raced task {index} was cancelled").into());
            }
        }
        if deadline.is_some_and(|deadline| Instant::now() >= deadline) {
            return Err("task_wait_timeout: no raced task completed".into());
        }
        thread::sleep(Duration::from_millis(1));
    }
}

fn task_cancel_all(tasks: Array) -> Result<rhai::INT, Box<EvalAltResult>> {
    let tasks = typed_tasks(tasks)?;
    let mut cancelled = 0_i64;
    for task in tasks {
        if cancel_inner(&task.0)? {
            cancelled += 1;
        }
    }
    Ok(cancelled)
}

fn typed_tasks(values: Array) -> Result<Vec<ScriptTask>, Box<EvalAltResult>> {
    if values.len() > MAX_TASKS_PER_CALL {
        return Err(format!("task_collection_too_large: maximum is {MAX_TASKS_PER_CALL}").into());
    }
    values
        .into_iter()
        .map(|value| {
            value
                .try_cast::<ScriptTask>()
                .ok_or_else(|| "task_collection_type: every item must be a Task".into())
        })
        .collect()
}

const fn status_name(status: TaskStatus) -> &'static str {
    match status {
        TaskStatus::Pending => "pending",
        TaskStatus::Completed => "completed",
        TaskStatus::Failed => "failed",
        TaskStatus::Cancelled => "cancelled",
    }
}

fn payload_dynamic(payload: ScriptTaskPayload) -> Dynamic {
    match payload {
        ScriptTaskPayload::Unit => Dynamic::UNIT,
        ScriptTaskPayload::Http(response) => Dynamic::from(response),
    }
}

struct ActiveTaskPermit;

impl Drop for ActiveTaskPermit {
    fn drop(&mut self) {
        ACTIVE_TASKS.fetch_sub(1, Ordering::AcqRel);
    }
}

fn acquire_task_permit() -> Result<ActiveTaskPermit, Box<EvalAltResult>> {
    ACTIVE_TASKS
        .fetch_update(Ordering::AcqRel, Ordering::Acquire, |active| {
            (active < MAX_ACTIVE_TASKS).then_some(active + 1)
        })
        .map_err(|_| -> Box<EvalAltResult> {
            format!("task_limit: maximum active host tasks is {MAX_ACTIVE_TASKS}").into()
        })?;
    Ok(ActiveTaskPermit)
}

pub(crate) fn start_http_task(
    work: impl FnOnce() -> Result<crate::script_http::ScriptHttpResponse, String> + Send + 'static,
) -> Result<ScriptTask, Box<EvalAltResult>> {
    let permit = acquire_task_permit()?;
    let inner = Arc::new(TaskInner {
        id: NEXT_TASK_ID.fetch_add(1, Ordering::Relaxed),
        kind: "http",
        state: Mutex::new(TaskState {
            status: TaskStatus::Pending,
            payload: None,
            failure_code: None,
        }),
        changed: Condvar::new(),
    });
    let worker = Arc::clone(&inner);
    thread::Builder::new()
        .name("agenterm-task-http".to_owned())
        .spawn(move || {
            let _permit = permit;
            let result = work();
            let Ok(mut state) = worker.state.lock() else {
                return;
            };
            if state.status != TaskStatus::Pending {
                return;
            }
            match result {
                Ok(response) => {
                    state.status = TaskStatus::Completed;
                    state.payload = Some(ScriptTaskPayload::Http(response));
                }
                Err(code) => {
                    state.status = TaskStatus::Failed;
                    state.failure_code = Some(code);
                }
            }
            worker.changed.notify_all();
        })
        .map_err(|_| -> Box<EvalAltResult> {
            "task_spawn_failed: HTTP worker could not be created".into()
        })?;
    Ok(ScriptTask(inner))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rhai::Shared;

    fn engine() -> Engine {
        let mut engine = Engine::new();
        let mut rhai_module = Module::new();
        register(&mut engine, &mut rhai_module);
        engine.register_static_module("rh", Shared::new(rhai_module));
        let mut std_module = Module::new();
        let mut time = Module::new();
        crate::script_process::register(&mut engine, &mut std_module, &mut time);
        std_module.set_sub_module("time", time);
        engine.register_static_module("std", Shared::new(std_module));
        engine
    }

    #[test]
    fn timers_wait_race_and_cancel_with_stable_state() {
        let result = engine()
            .eval::<rhai::Map>(
                r#"
                    let slow = rh::task::after(std::time::Duration::from_millis(30));
                    let fast = rh::task::after(std::time::Duration::from_millis(1));
                    let winner = rh::task::race([slow, fast],
                        std::time::Duration::from_secs(1));
                    fast.wait();
                    let cancelled = slow.cancel();
                    #{winner: winner, fast: fast.state, cancelled: cancelled,
                      slow: slow.state}
                "#,
            )
            .unwrap();
        assert_eq!(result["winner"].as_int().unwrap(), 1);
        assert_eq!(result["fast"].clone().into_string().unwrap(), "completed");
        assert!(result["cancelled"].as_bool().unwrap());
        assert_eq!(result["slow"].clone().into_string().unwrap(), "cancelled");
    }

    #[test]
    fn wait_timeout_is_typed_without_cancelling_the_task() {
        let error = engine()
            .eval::<Dynamic>(
                r#"
                    let task = rh::task::after(std::time::Duration::from_secs(1));
                    task.wait(std::time::Duration::from_millis(1));
                "#,
            )
            .unwrap_err()
            .to_string();
        assert!(error.contains("task_wait_timeout"));
    }
}
