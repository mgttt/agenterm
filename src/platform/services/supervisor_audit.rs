//! Product-neutral Script worker and audit coordination facade.

use std::{
    path::{Path, PathBuf},
    process::{Child, Command},
    sync::atomic::{AtomicUsize, Ordering},
};

use crate::platform::{contract::supervisor_audit::SupervisorAuditError, selected::supervisor_audit as adapter};

pub(crate) struct ConcurrencyPermit {
    global: adapter::GlobalConcurrencyPermit,
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
            return Err(adapter::concurrency_limit_error());
        }
        match adapter::GlobalConcurrencyPermit::try_acquire(global_limit) {
            Ok(global) => Ok(Self { global, active }),
            Err(error) => {
                active.fetch_sub(1, Ordering::AcqRel);
                Err(error)
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
        self.0.terminate(exit_code).map_err(adapter::process_tree_error)
    }
}

pub(crate) fn terminate_worker(child: &mut Child, _pid: u32) {
    let _ = child.kill();
    let _ = child.wait();
}

pub(crate) struct NamedAuditLock(adapter::NamedAuditLock);

impl NamedAuditLock {
    pub(crate) fn acquire(path: &Path) -> Result<Self, SupervisorAuditError> {
        adapter::NamedAuditLock::acquire(path).map(Self)
    }
}

pub(crate) fn default_audit_path() -> PathBuf {
    adapter::default_audit_path()
}
