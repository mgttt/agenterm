use std::collections::HashSet;

use crate::contract::processor_topology::{
    ProcessorTopologyError, ProcessorTopologyErrorKind, ProcessorTopologyFacts,
};

pub(crate) fn facts() -> Result<ProcessorTopologyFacts, ProcessorTopologyError> {
    let logical = unsafe { libc::sysconf(libc::_SC_NPROCESSORS_ONLN) };
    let logical = u64::try_from(logical).map_err(|_| {
        ProcessorTopologyError::new(
            ProcessorTopologyErrorKind::Query,
            format!("sysconf(_SC_NPROCESSORS_ONLN) returned {logical}"),
        )
    })?;
    let (physical_cores, packages) = cpu_topology_counts();
    ProcessorTopologyFacts::from_counts(
        logical,
        physical_cores,
        packages,
        numbered_directories("/sys/devices/system/node", "node"),
        None,
    )
}

fn cpu_topology_counts() -> (Option<u64>, Option<u64>) {
    let Ok(entries) = std::fs::read_dir("/sys/devices/system/cpu") else {
        return (None, None);
    };
    let mut cores = HashSet::new();
    let mut packages = HashSet::new();
    let mut online_cpus = 0_u64;
    let mut complete_core_topology = true;
    let mut complete_package_topology = true;
    for entry in entries.flatten() {
        let name = entry.file_name();
        if numbered_name(name.to_string_lossy().as_ref(), "cpu").is_none() {
            continue;
        }
        let root = entry.path();
        if std::fs::read_to_string(root.join("online")).is_ok_and(|value| value.trim() == "0") {
            continue;
        }
        online_cpus += 1;
        let package = read_i64(root.join("topology/physical_package_id"));
        let core = read_i64(root.join("topology/core_id"));
        if let Some(package) = package {
            packages.insert(package);
            if let Some(core) = core {
                cores.insert((package, core));
            } else {
                complete_core_topology = false;
            }
        } else {
            complete_package_topology = false;
            complete_core_topology = false;
        }
    }
    if online_cpus == 0 {
        return (None, None);
    }
    (
        complete_core_topology
            .then(|| nonempty_len(&cores))
            .flatten(),
        complete_package_topology
            .then(|| nonempty_len(&packages))
            .flatten(),
    )
}

fn numbered_directories(root: &str, prefix: &str) -> Option<u64> {
    let entries = std::fs::read_dir(root).ok()?;
    let count = entries
        .flatten()
        .filter(|entry| {
            entry.file_type().is_ok_and(|kind| kind.is_dir())
                && numbered_name(entry.file_name().to_string_lossy().as_ref(), prefix).is_some()
        })
        .count();
    u64::try_from(count).ok().filter(|count| *count > 0)
}

fn numbered_name(name: &str, prefix: &str) -> Option<u64> {
    name.strip_prefix(prefix)?.parse().ok()
}

fn read_i64(path: std::path::PathBuf) -> Option<i64> {
    std::fs::read_to_string(path).ok()?.trim().parse().ok()
}

fn nonempty_len<T>(values: &HashSet<T>) -> Option<u64> {
    u64::try_from(values.len()).ok().filter(|count| *count > 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbered_sysfs_names_are_strict() {
        assert_eq!(numbered_name("cpu0", "cpu"), Some(0));
        assert_eq!(numbered_name("cpu127", "cpu"), Some(127));
        assert_eq!(numbered_name("cpufreq", "cpu"), None);
        assert_eq!(numbered_name("node-1", "node"), None);
    }
}
