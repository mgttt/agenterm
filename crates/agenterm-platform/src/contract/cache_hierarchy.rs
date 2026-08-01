//! Product-neutral host CPU cache hierarchy facts.

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[non_exhaustive]
pub enum CacheKind {
    Unified,
    Data,
    Instruction,
    Trace,
    Other,
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct CacheGeometryFacts {
    pub level: std::num::NonZeroU8,
    pub kind: CacheKind,
    /// Capacity of one cache instance.
    pub size_bytes: std::num::NonZeroU64,
    pub line_bytes: std::num::NonZeroU32,
    /// Number of cache instances with this exact geometry, when discoverable.
    pub instances: Option<std::num::NonZeroUsize>,
    /// Logical processors sharing each instance, when discoverable and uniform.
    pub shared_logical_processors: Option<std::num::NonZeroUsize>,
}

impl CacheGeometryFacts {
    pub fn from_raw(
        level: u64,
        kind: CacheKind,
        size_bytes: u64,
        line_bytes: u64,
        instances: Option<u64>,
        shared_logical_processors: Option<u64>,
    ) -> Result<Self, CacheHierarchyError> {
        let level = u8::try_from(level)
            .ok()
            .and_then(std::num::NonZeroU8::new)
            .ok_or_else(|| invalid("cache level", level))?;
        let size_bytes = std::num::NonZeroU64::new(size_bytes)
            .ok_or_else(|| invalid("cache size bytes", size_bytes))?;
        let line_bytes = u32::try_from(line_bytes)
            .ok()
            .and_then(std::num::NonZeroU32::new)
            .ok_or_else(|| invalid("cache line bytes", line_bytes))?;
        if u64::from(line_bytes.get()) > size_bytes.get() {
            return Err(CacheHierarchyError::new(
                CacheHierarchyErrorKind::InvalidValue,
                format!(
                    "cache line {} exceeds cache size {}",
                    line_bytes.get(),
                    size_bytes.get()
                ),
            ));
        }
        Ok(Self {
            level,
            kind,
            size_bytes,
            line_bytes,
            instances: optional_nonzero_usize("cache instances", instances)?,
            shared_logical_processors: optional_nonzero_usize(
                "shared logical processors",
                shared_logical_processors,
            )?,
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CacheHierarchyFacts {
    pub geometries: Vec<CacheGeometryFacts>,
}

impl CacheHierarchyFacts {
    pub fn new(mut geometries: Vec<CacheGeometryFacts>) -> Result<Self, CacheHierarchyError> {
        if geometries.is_empty() {
            return Err(CacheHierarchyError::new(
                CacheHierarchyErrorKind::Unavailable,
                "host reported no CPU cache geometry",
            ));
        }
        geometries.sort_unstable();
        if geometries.windows(2).any(|pair| pair[0] == pair[1]) {
            return Err(CacheHierarchyError::new(
                CacheHierarchyErrorKind::InvalidValue,
                "duplicate CPU cache geometry",
            ));
        }
        Ok(Self { geometries })
    }

    /// Largest coherency line used by a data-bearing cache.
    #[must_use]
    pub fn max_data_line_bytes(&self) -> Option<std::num::NonZeroU32> {
        self.geometries
            .iter()
            .filter(|cache| matches!(cache.kind, CacheKind::Unified | CacheKind::Data))
            .map(|cache| cache.line_bytes)
            .max()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum CacheHierarchyErrorKind {
    Query,
    Unavailable,
    InvalidValue,
    MalformedNativeData,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CacheHierarchyError {
    kind: CacheHierarchyErrorKind,
    detail: String,
}

impl CacheHierarchyError {
    pub(crate) fn new(kind: CacheHierarchyErrorKind, detail: impl Into<String>) -> Self {
        Self {
            kind,
            detail: detail.into(),
        }
    }

    #[must_use]
    pub const fn kind(&self) -> CacheHierarchyErrorKind {
        self.kind
    }

    #[must_use]
    pub fn detail(&self) -> &str {
        &self.detail
    }
}

impl std::fmt::Display for CacheHierarchyError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "cache hierarchy {:?}: {}",
            self.kind, self.detail
        )
    }
}

impl std::error::Error for CacheHierarchyError {}

fn optional_nonzero_usize(
    name: &str,
    value: Option<u64>,
) -> Result<Option<std::num::NonZeroUsize>, CacheHierarchyError> {
    value
        .map(|value| {
            usize::try_from(value)
                .ok()
                .and_then(std::num::NonZeroUsize::new)
                .ok_or_else(|| invalid(name, value))
        })
        .transpose()
}

fn invalid(name: &str, value: u64) -> CacheHierarchyError {
    CacheHierarchyError::new(
        CacheHierarchyErrorKind::InvalidValue,
        format!("host reported invalid {name}: {value}"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn geometry(level: u64, kind: CacheKind, line: u64) -> CacheGeometryFacts {
        CacheGeometryFacts::from_raw(level, kind, 32 * 1024, line, Some(4), Some(2)).unwrap()
    }

    #[test]
    fn geometry_rejects_zero_overflow_and_incoherent_values() {
        for invalid in [
            CacheGeometryFacts::from_raw(0, CacheKind::Data, 1, 1, None, None),
            CacheGeometryFacts::from_raw(256, CacheKind::Data, 1, 1, None, None),
            CacheGeometryFacts::from_raw(1, CacheKind::Data, 0, 1, None, None),
            CacheGeometryFacts::from_raw(1, CacheKind::Data, 64, 128, None, None),
            CacheGeometryFacts::from_raw(1, CacheKind::Data, 64, 64, Some(0), None),
            CacheGeometryFacts::from_raw(1, CacheKind::Data, 64, 64, None, Some(0)),
        ] {
            assert_eq!(
                invalid.unwrap_err().kind(),
                CacheHierarchyErrorKind::InvalidValue
            );
        }
    }

    #[test]
    fn hierarchy_is_sorted_and_rejects_empty_or_duplicate_input() {
        assert_eq!(
            CacheHierarchyFacts::new(Vec::new()).unwrap_err().kind(),
            CacheHierarchyErrorKind::Unavailable
        );
        let l1 = geometry(1, CacheKind::Data, 64);
        let l2 = geometry(2, CacheKind::Unified, 128);
        let facts = CacheHierarchyFacts::new(vec![l2, l1]).unwrap();
        assert_eq!(facts.geometries, vec![l1, l2]);
        assert_eq!(
            CacheHierarchyFacts::new(vec![l1, l1]).unwrap_err().kind(),
            CacheHierarchyErrorKind::InvalidValue
        );
    }

    #[test]
    fn maximum_data_line_ignores_instruction_only_caches() {
        let facts = CacheHierarchyFacts::new(vec![
            geometry(1, CacheKind::Instruction, 256),
            geometry(1, CacheKind::Data, 64),
            geometry(2, CacheKind::Unified, 128),
        ])
        .unwrap();
        assert_eq!(facts.max_data_line_bytes().unwrap().get(), 128);
    }
}
