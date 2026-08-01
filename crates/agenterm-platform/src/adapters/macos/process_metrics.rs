//! macOS cumulative process resource counters.

use std::time::Duration;

use crate::contract::process_metrics::{
    ProcessMetrics, ProcessMetricsError, ProcessMetricsErrorKind,
};

pub(crate) fn metrics(pid: u32) -> Result<ProcessMetrics, ProcessMetricsError> {
    if pid == 0 {
        return Err(ProcessMetricsError::new(
            ProcessMetricsErrorKind::InvalidId,
            "process ID zero does not identify one process",
        ));
    }
    let pid = libc::pid_t::try_from(pid).map_err(|source| {
        ProcessMetricsError::new(ProcessMetricsErrorKind::InvalidId, source.to_string())
    })?;
    let mut task = unsafe { std::mem::zeroed::<libc::proc_taskinfo>() };
    let size = std::mem::size_of::<libc::proc_taskinfo>() as libc::c_int;
    let read =
        unsafe { libc::proc_pidinfo(pid, libc::PROC_PIDTASKINFO, 0, (&raw mut task).cast(), size) };
    if read != size {
        return Err(ProcessMetricsError::new(
            ProcessMetricsErrorKind::Read,
            std::io::Error::last_os_error().to_string(),
        ));
    }
    Ok(ProcessMetrics {
        cpu_time: Duration::from_nanos(task.pti_total_user.saturating_add(task.pti_total_system)),
        resident_bytes: task.pti_resident_size,
    })
}
