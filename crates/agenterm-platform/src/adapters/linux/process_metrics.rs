//! Linux `/proc` cumulative process resource counters.

use std::time::Duration;

use crate::contract::process_metrics::{
    ProcessMetrics, ProcessMetricsError, ProcessMetricsErrorKind, checked_page_faults,
};

pub(crate) fn metrics(pid: u32) -> Result<ProcessMetrics, ProcessMetricsError> {
    if pid == 0 {
        return Err(error(
            ProcessMetricsErrorKind::InvalidId,
            "process ID zero does not identify one process",
        ));
    }
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).map_err(|source| {
        error(
            if source.kind() == std::io::ErrorKind::NotFound {
                ProcessMetricsErrorKind::NotFound
            } else {
                ProcessMetricsErrorKind::Open
            },
            source.to_string(),
        )
    })?;
    let close = stat.rfind(')').ok_or_else(|| {
        error(
            ProcessMetricsErrorKind::Parse,
            "process stat has no closing command delimiter",
        )
    })?;
    let fields: Vec<&str> = stat[close + 1..].split_whitespace().collect();
    let minor_faults = parse_field(&fields, 7, "minor page faults")?;
    let major_faults = parse_field(&fields, 9, "major page faults")?;
    let user_ticks = parse_field(&fields, 11, "user CPU ticks")?;
    let system_ticks = parse_field(&fields, 12, "system CPU ticks")?;
    let clock_hz = sysconf(libc::_SC_CLK_TCK, "clock ticks")?;
    let page_size = sysconf(libc::_SC_PAGESIZE, "page size")?;
    let statm = std::fs::read_to_string(format!("/proc/{pid}/statm")).map_err(|source| {
        error(
            if source.kind() == std::io::ErrorKind::NotFound {
                ProcessMetricsErrorKind::NotFound
            } else {
                ProcessMetricsErrorKind::Read
            },
            source.to_string(),
        )
    })?;
    let resident_pages = statm
        .split_whitespace()
        .nth(1)
        .ok_or_else(|| {
            error(
                ProcessMetricsErrorKind::Parse,
                "process statm has no RSS field",
            )
        })?
        .parse::<u64>()
        .map_err(|source| error(ProcessMetricsErrorKind::Parse, source.to_string()))?;
    let ticks = user_ticks.saturating_add(system_ticks);
    let total_faults = minor_faults.checked_add(major_faults).ok_or_else(|| {
        error(
            ProcessMetricsErrorKind::Overflow,
            "total page-fault count overflows u64",
        )
    })?;
    Ok(ProcessMetrics {
        cpu_time: Duration::from_secs(ticks / clock_hz).saturating_add(Duration::from_nanos(
            (ticks % clock_hz).saturating_mul(1_000_000_000) / clock_hz,
        )),
        resident_bytes: resident_pages.saturating_mul(page_size),
        page_faults: checked_page_faults(total_faults, Some(minor_faults), Some(major_faults))?,
    })
}

fn parse_field(fields: &[&str], index: usize, name: &str) -> Result<u64, ProcessMetricsError> {
    fields
        .get(index)
        .ok_or_else(|| error(ProcessMetricsErrorKind::Parse, format!("missing {name}")))?
        .parse::<u64>()
        .map_err(|source| error(ProcessMetricsErrorKind::Parse, source.to_string()))
}

fn sysconf(key: libc::c_int, name: &str) -> Result<u64, ProcessMetricsError> {
    let value = unsafe { libc::sysconf(key) };
    u64::try_from(value).map_err(|_| {
        error(
            ProcessMetricsErrorKind::Clock,
            format!("host reported invalid {name}: {value}"),
        )
    })
}

fn error(kind: ProcessMetricsErrorKind, detail: impl Into<String>) -> ProcessMetricsError {
    ProcessMetricsError::new(kind, detail)
}
