//! Product-neutral Script worker and audit coordination facade.

use std::{
    path::{Path, PathBuf},
    process::{Child, Command},
    sync::atomic::{AtomicUsize, Ordering},
};

use crate::platform::{
    contract::supervisor_audit::{SupervisorAuditError, SupervisorAuditErrorKind},
    selected::supervisor_audit as adapter,
};

pub(crate) struct ConcurrencyPermit {
    global: agenterm_platform::locking::SlotPermit,
    active: &'static AtomicUsize,
}

impl ConcurrencyPermit {
    pub(crate) fn try_acquire(
        active: &'static AtomicUsize,
        process_limit: usize,
        global_limit: usize,
    ) -> Result<Self, SupervisorAuditError> {
        let previous = active.fetch_add(1, Ordering::AcqRel);
        if previous >= process_limit {
            active.fetch_sub(1, Ordering::AcqRel);
            return Err(concurrency_limit_error());
        }
        match agenterm_platform::locking::SlotPermit::try_acquire(
            &std::env::temp_dir(),
            "agenterm-script-supervisor-v1",
            global_limit,
        ) {
            Ok(global) => Ok(Self { global, active }),
            Err(error) => {
                active.fetch_sub(1, Ordering::AcqRel);
                Err(map_lock_error(error))
            }
        }
    }
}

impl Drop for ConcurrencyPermit {
    fn drop(&mut self) {
        let _ = &self.global;
        self.active.fetch_sub(1, Ordering::AcqRel);
    }
}

pub(crate) fn configure_worker_command(command: &mut Command) -> Result<(), SupervisorAuditError> {
    adapter::configure_worker_command(command).map_err(adapter::process_tree_error)
}

pub(crate) struct ProcessTreeGuard(adapter::ProcessTreeGuard);

impl ProcessTreeGuard {
    pub(crate) fn attach(child: &Child) -> Result<Self, SupervisorAuditError> {
        adapter::ProcessTreeGuard::attach(child)
            .map(Self)
            .map_err(adapter::process_tree_error)
    }

    pub(crate) fn terminate(&mut self, exit_code: u32) -> Result<(), SupervisorAuditError> {
        self.0
            .terminate(exit_code)
            .map_err(adapter::process_tree_error)
    }
}

pub(crate) fn terminate_worker(child: &mut Child, _pid: u32) {
    let _ = child.kill();
    let _ = child.wait();
}

pub(crate) struct NamedAuditLock(agenterm_platform::locking::PathLock);

impl NamedAuditLock {
    pub(crate) fn acquire(path: &Path) -> Result<Self, SupervisorAuditError> {
        let lock_path = path.with_extension("jsonl.lock");
        agenterm_platform::locking::PathLock::acquire(&lock_path)
            .map(Self)
            .map_err(map_lock_error)
    }
}

pub(crate) fn default_audit_path() -> PathBuf {
    adapter::default_audit_path()
}

fn concurrency_limit_error() -> SupervisorAuditError {
    SupervisorAuditError::new(
        SupervisorAuditErrorKind::LockWait,
        "global worker concurrency limit reached",
    )
}

fn map_lock_error(error: agenterm_platform::locking::LockError) -> SupervisorAuditError {
    let kind = match error.kind() {
        agenterm_platform::locking::LockErrorKind::Open
        | agenterm_platform::locking::LockErrorKind::InvalidInput => {
            SupervisorAuditErrorKind::LockOpen
        }
        agenterm_platform::locking::LockErrorKind::Wait
        | agenterm_platform::locking::LockErrorKind::Contended => {
            SupervisorAuditErrorKind::LockWait
        }
        _ => SupervisorAuditErrorKind::LockWait,
    };
    SupervisorAuditError::new(kind, error.to_string())
}
