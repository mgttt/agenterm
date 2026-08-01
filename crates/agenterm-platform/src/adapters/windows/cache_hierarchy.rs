use std::collections::{BTreeMap, HashSet};

use windows_sys::Win32::System::SystemInformation::{
    CACHE_RELATIONSHIP, CacheData, CacheInstruction, CacheTrace, CacheUnified, GROUP_AFFINITY,
    RelationCache, SYSTEM_LOGICAL_PROCESSOR_INFORMATION_EX,
};

use crate::contract::cache_hierarchy::{
    CacheGeometryFacts, CacheHierarchyError, CacheHierarchyErrorKind, CacheHierarchyFacts,
    CacheKind,
};

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct RawGeometry {
    level: u8,
    kind: CacheKind,
    size_bytes: u32,
    line_bytes: u16,
    shared_logical_processors: Option<u64>,
}

pub(crate) fn facts() -> Result<CacheHierarchyFacts, CacheHierarchyError> {
    let caches = super::logical_processor::query_records(RelationCache, RelationCache, parse_cache)
        .map_err(|error| native_error("RelationCache", error))?;
    let mut grouped = BTreeMap::<RawGeometry, u64>::new();
    for cache in caches {
        let instances = grouped.entry(cache).or_default();
        *instances = instances.checked_add(1).ok_or_else(|| {
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
                u64::from(cache.level),
                cache.kind,
                u64::from(cache.size_bytes),
                u64::from(cache.line_bytes),
                Some(instances),
                cache.shared_logical_processors,
            )
        })
        .collect::<Result<Vec<_>, _>>()?;
    CacheHierarchyFacts::new(geometries)
}

fn parse_cache(record: &[u8]) -> std::io::Result<RawGeometry> {
    let cache_offset = std::mem::offset_of!(SYSTEM_LOGICAL_PROCESSOR_INFORMATION_EX, Anonymous);
    let groups_offset = cache_offset + std::mem::offset_of!(CACHE_RELATIONSHIP, Anonymous);
    let fixed_size = cache_offset + std::mem::size_of::<CACHE_RELATIONSHIP>();
    if record.len() < fixed_size {
        return Err(malformed("truncated cache relationship"));
    }
    let cache = unsafe {
        record
            .as_ptr()
            .add(cache_offset)
            .cast::<CACHE_RELATIONSHIP>()
            .read_unaligned()
    };
    let group_count = usize::from(cache.GroupCount);
    if group_count == 0 {
        return Err(malformed("cache relationship has no processor groups"));
    }
    let group_bytes = group_count
        .checked_mul(std::mem::size_of::<GROUP_AFFINITY>())
        .and_then(|bytes| groups_offset.checked_add(bytes))
        .ok_or_else(|| malformed("cache processor group length overflow"))?;
    if group_bytes > record.len() {
        return Err(malformed("truncated cache processor group array"));
    }

    let mut groups = HashSet::new();
    let mut shared = 0_u64;
    for index in 0..group_count {
        let affinity = unsafe {
            record
                .as_ptr()
                .add(groups_offset + index * std::mem::size_of::<GROUP_AFFINITY>())
                .cast::<GROUP_AFFINITY>()
                .read_unaligned()
        };
        if affinity.Mask == 0 || !groups.insert(affinity.Group) {
            return Err(malformed("cache relationship has an invalid group mask"));
        }
        shared = shared
            .checked_add(u64::from(affinity.Mask.count_ones()))
            .ok_or_else(|| malformed("cache sharing count overflow"))?;
    }
    #[cfg(target_pointer_width = "64")]
    let shared_logical_processors = Some(shared);
    // WOW64 folds affinity masks above processor 31, so a 32-bit process
    // cannot truthfully report cache sharing width on large hosts.
    #[cfg(target_pointer_width = "32")]
    let shared_logical_processors = {
        let _ = shared;
        None
    };

    Ok(RawGeometry {
        level: cache.Level,
        kind: cache_kind(cache.Type),
        size_bytes: cache.CacheSize,
        line_bytes: cache.LineSize,
        shared_logical_processors,
    })
}

fn cache_kind(native: i32) -> CacheKind {
    if native == CacheUnified {
        CacheKind::Unified
    } else if native == CacheData {
        CacheKind::Data
    } else if native == CacheInstruction {
        CacheKind::Instruction
    } else if native == CacheTrace {
        CacheKind::Trace
    } else {
        CacheKind::Other
    }
}

fn malformed(message: &'static str) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::InvalidData, message)
}

fn native_error(context: &str, error: std::io::Error) -> CacheHierarchyError {
    let kind = if error.kind() == std::io::ErrorKind::InvalidData {
        CacheHierarchyErrorKind::MalformedNativeData
    } else {
        CacheHierarchyErrorKind::Query
    };
    CacheHierarchyError::new(kind, format!("{context}: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_cache_kinds_have_stable_neutral_mappings() {
        assert_eq!(cache_kind(CacheUnified), CacheKind::Unified);
        assert_eq!(cache_kind(CacheData), CacheKind::Data);
        assert_eq!(cache_kind(CacheInstruction), CacheKind::Instruction);
        assert_eq!(cache_kind(CacheTrace), CacheKind::Trace);
        assert_eq!(cache_kind(i32::MAX), CacheKind::Other);
    }
}
