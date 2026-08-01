//! Current-host CPU cache hierarchy facts.

pub use crate::contract::cache_hierarchy::{
    CacheGeometryFacts, CacheHierarchyError, CacheHierarchyErrorKind, CacheHierarchyFacts,
    CacheKind,
};

pub fn facts() -> Result<CacheHierarchyFacts, CacheHierarchyError> {
    crate::selected::cache_hierarchy::facts()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_a_coherent_current_host_cache_hierarchy() {
        assert_eq!(
            crate::capability_status(crate::Capability::CacheHierarchy),
            crate::CapabilityStatus::Available
        );
        let facts = facts().expect("query current host cache hierarchy");
        eprintln!("cache hierarchy: {facts:?}");
        assert!(!facts.geometries.is_empty());
        assert!(facts.max_data_line_bytes().is_some());
        for cache in facts.geometries {
            assert!(cache.line_bytes.get() as u64 <= cache.size_bytes.get());
        }
    }
}
