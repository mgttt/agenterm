use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use crate::contract::cache_hierarchy::{
    CacheGeometryFacts, CacheHierarchyError, CacheHierarchyErrorKind, CacheHierarchyFacts,
    CacheKind,
};

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct CacheInstance {
    level: u64,
    kind: CacheKind,
    size_bytes: u64,
    line_bytes: u64,
    shared_cpus: Vec<u32>,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct RawGeometry {
    level: u64,
    kind: CacheKind,
    size_bytes: u64,
    line_bytes: u64,
    shared_logical_processors: u64,
}

pub(crate) fn facts() -> Result<CacheHierarchyFacts, CacheHierarchyError> {
    let cpu_root = Path::new("/sys/devices/system/cpu");
    let online = online_cpus(cpu_root)?;
    let mut instances = BTreeSet::new();
    for cpu in &online {
        let cache_root = cpu_root.join(format!("cpu{cpu}/cache"));
        let indexes =
            std::fs::read_dir(&cache_root).map_err(|error| query_error(&cache_root, error))?;
        for entry in indexes {
            let entry = entry.map_err(|error| query_error(&cache_root, error))?;
            let name = entry.file_name();
            if numbered_name(name.to_string_lossy().as_ref(), "index").is_none() {
                continue;
            }
            let instance = read_instance(&entry.path())?;
            if !instance.shared_cpus.contains(cpu) {
                return Err(malformed(format!(
                    "{} does not include cpu{cpu} in shared_cpu_list",
                    entry.path().display()
                )));
            }
            instances.insert(instance);
        }
    }

    let mut grouped = BTreeMap::<RawGeometry, u64>::new();
    for instance in instances {
        let shared = instance
            .shared_cpus
            .iter()
            .filter(|cpu| online.contains(cpu))
            .count();
        let shared = u64::try_from(shared)
            .ok()
            .filter(|count| *count > 0)
            .ok_or_else(|| malformed("cache has no online logical processors"))?;
        let geometry = RawGeometry {
            level: instance.level,
            kind: instance.kind,
            size_bytes: instance.size_bytes,
            line_bytes: instance.line_bytes,
            shared_logical_processors: shared,
        };
        let count = grouped.entry(geometry).or_default();
        *count = count.checked_add(1).ok_or_else(|| {
            CacheHierarchyError::new(
                CacheHierarchyErrorKind::InvalidValue,
                "cache instance count overflow",
            )
        })?;
    }
    let geometries = grouped
        .into_iter()
        .map(|(cache, instances)| {
            CacheGeometryFacts::from_raw(
                cache.level,
                cache.kind,
                cache.size_bytes,
                cache.line_bytes,
                Some(instances),
                Some(cache.shared_logical_processors),
            )
        })
        .collect::<Result<Vec<_>, _>>()?;
    CacheHierarchyFacts::new(geometries)
}

fn online_cpus(root: &Path) -> Result<BTreeSet<u32>, CacheHierarchyError> {
    let entries = std::fs::read_dir(root).map_err(|error| query_error(root, error))?;
    let mut cpus = BTreeSet::new();
    for entry in entries {
        let entry = entry.map_err(|error| query_error(root, error))?;
        let name = entry.file_name();
        let Some(cpu) = numbered_name(name.to_string_lossy().as_ref(), "cpu") else {
            continue;
        };
        let cpu = u32::try_from(cpu).map_err(|_| malformed("CPU identifier exceeds u32"))?;
        let online_path = entry.path().join("online");
        let online = match std::fs::read_to_string(&online_path) {
            Ok(value) => match value.trim() {
                "0" => false,
                "1" => true,
                value => {
                    return Err(malformed(format!(
                        "{} contains invalid online state {value:?}",
                        online_path.display()
                    )));
                }
            },
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => true,
            Err(error) => return Err(query_error(&online_path, error)),
        };
        if online {
            cpus.insert(cpu);
        }
    }
    if cpus.is_empty() {
        return Err(CacheHierarchyError::new(
            CacheHierarchyErrorKind::Unavailable,
            "sysfs reported no online CPUs",
        ));
    }
    Ok(cpus)
}

fn read_instance(root: &Path) -> Result<CacheInstance, CacheHierarchyError> {
    let level = read_u64(root.join("level"))?;
    let kind_path = root.join("type");
    let kind = match read_text(&kind_path)?.as_str() {
        "Unified" => CacheKind::Unified,
        "Data" => CacheKind::Data,
        "Instruction" => CacheKind::Instruction,
        "Trace" => CacheKind::Trace,
        _ => CacheKind::Other,
    };
    let size_path = root.join("size");
    let size_bytes = parse_size(&read_text(&size_path)?)
        .map_err(|message| malformed(format!("{}: {message}", size_path.display())))?;
    let line_bytes = read_u64(root.join("coherency_line_size"))?;
    let shared_path = root.join("shared_cpu_list");
    let shared_cpus = parse_cpu_list(&read_text(&shared_path)?)
        .map_err(|message| malformed(format!("{}: {message}", shared_path.display())))?;
    Ok(CacheInstance {
        level,
        kind,
        size_bytes,
        line_bytes,
        shared_cpus,
    })
}

fn read_u64(path: PathBuf) -> Result<u64, CacheHierarchyError> {
    let value = read_text(&path)?;
    value.parse().map_err(|_| {
        malformed(format!(
            "{} contains invalid integer {value:?}",
            path.display()
        ))
    })
}

fn read_text(path: &Path) -> Result<String, CacheHierarchyError> {
    std::fs::read_to_string(path)
        .map(|value| value.trim().to_owned())
        .map_err(|error| query_error(path, error))
}

fn parse_size(value: &str) -> Result<u64, &'static str> {
    let digits = value.bytes().take_while(u8::is_ascii_digit).count();
    if digits == 0 {
        return Err("cache size has no numeric value");
    }
    let amount: u64 = value[..digits]
        .parse()
        .map_err(|_| "cache size integer overflow")?;
    let multiplier = match value[digits..].trim().to_ascii_uppercase().as_str() {
        "" | "B" => 1,
        "K" | "KB" => 1024,
        "M" | "MB" => 1024 * 1024,
        "G" | "GB" => 1024 * 1024 * 1024,
        _ => return Err("cache size has an unknown unit"),
    };
    amount
        .checked_mul(multiplier)
        .ok_or("cache size multiplication overflow")
}

fn parse_cpu_list(value: &str) -> Result<Vec<u32>, &'static str> {
    const MAX_EXPANDED_CPUS: usize = 1024 * 1024;

    let mut cpus = BTreeSet::new();
    if value.is_empty() {
        return Err("empty CPU list");
    }
    for part in value.split(',') {
        let mut bounds = part.split('-');
        let start: u32 = bounds
            .next()
            .ok_or("missing CPU range start")?
            .parse()
            .map_err(|_| "invalid CPU identifier")?;
        let end = bounds
            .next()
            .map(str::parse)
            .transpose()
            .map_err(|_| "invalid CPU range end")?
            .unwrap_or(start);
        if bounds.next().is_some() || end < start {
            return Err("invalid CPU range");
        }
        let range_len = usize::try_from(u64::from(end) - u64::from(start) + 1)
            .map_err(|_| "CPU range length overflow")?;
        if cpus.len().saturating_add(range_len) > MAX_EXPANDED_CPUS {
            return Err("CPU list exceeds expansion limit");
        }
        for cpu in start..=end {
            cpus.insert(cpu);
        }
    }
    Ok(cpus.into_iter().collect())
}

fn numbered_name(name: &str, prefix: &str) -> Option<u64> {
    name.strip_prefix(prefix)?.parse().ok()
}

fn query_error(path: &Path, error: std::io::Error) -> CacheHierarchyError {
    CacheHierarchyError::new(
        CacheHierarchyErrorKind::Query,
        format!("{}: {error}", path.display()),
    )
}

fn malformed(detail: impl Into<String>) -> CacheHierarchyError {
    CacheHierarchyError::new(CacheHierarchyErrorKind::MalformedNativeData, detail)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_sizes_use_binary_sysfs_units_and_reject_bad_values() {
        assert_eq!(parse_size("64"), Ok(64));
        assert_eq!(parse_size("32K"), Ok(32 * 1024));
        assert_eq!(parse_size("2M"), Ok(2 * 1024 * 1024));
        assert!(parse_size("K").is_err());
        assert!(parse_size("32KiB").is_err());
    }

    #[test]
    fn cpu_lists_expand_sort_and_deduplicate_ranges() {
        assert_eq!(
            parse_cpu_list("0-3,2,8,10-11"),
            Ok(vec![0, 1, 2, 3, 8, 10, 11])
        );
        assert!(parse_cpu_list("").is_err());
        assert!(parse_cpu_list("3-1").is_err());
        assert!(parse_cpu_list("1-2-3").is_err());
        assert!(parse_cpu_list("0-4294967295").is_err());
    }
}
