//! macOS cumulative process resource counters.

use std::time::Duration;

use crate::contract::process_metrics::{
    ProcessMetrics, ProcessMetricsError, ProcessMetricsErrorKind, checked_page_faults,
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
        let error = std::io::Error::last_os_error();
        let kind = if error.raw_os_error() == Some(libc::ESRCH) {
            ProcessMetricsErrorKind::NotFound
        } else {
            ProcessMetricsErrorKind::Read
        };
        return Err(ProcessMetricsError::new(kind, error.to_string()));
    }
    let total_faults = nonnegative_counter(task.pti_faults, "page faults")?;
    let page_ins = nonnegative_counter(task.pti_pageins, "page-ins")?;
    Ok(ProcessMetrics {
        cpu_time: Duration::from_nanos(task.pti_total_user.saturating_add(task.pti_total_system)),
        resident_bytes: task.pti_resident_size,
        page_faults: checked_page_faults(total_faults, None, Some(page_ins))?,
    })
}

fn nonnegative_counter(value: i32, name: &str) -> Result<u64, ProcessMetricsError> {
    u64::try_from(value).map_err(|_| {
        ProcessMetricsError::new(
            ProcessMetricsErrorKind::InvalidValue,
            format!("host reported negative {name}: {value}"),
        )
    })
}
