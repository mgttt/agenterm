//! Stable host memory geometry without product resource-limit policy.

pub use crate::contract::host_memory::{
    HostMemoryAvailability, HostMemoryAvailabilitySemantics, HostMemoryError, HostMemoryErrorKind,
    HostMemoryFacts,
};

pub fn facts() -> Result<HostMemoryFacts, HostMemoryError> {
    crate::selected::host_memory::facts()
}

/// Capture the host's current available-physical-memory estimate.
///
/// This dynamic observation is deliberately separate from [`facts`], whose
/// page geometry and installed capacity are stable. It is not a promise that a
/// process or guest can allocate the reported amount.
pub fn availability() -> Result<HostMemoryAvailability, HostMemoryError> {
    crate::selected::host_memory::availability()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_coherent_host_memory_geometry() {
        let facts = facts().expect("query host memory facts");
        assert!(facts.allocation_granularity.get() >= facts.page_size.get());
        assert_eq!(
            facts.allocation_granularity.get() % facts.page_size.get(),
            0
        );
        assert!(facts.physical_bytes.get() >= facts.page_size.get() as u64);
    }

    #[test]
    fn reports_bounded_dynamic_availability() {
        let facts = facts().expect("query host memory facts");
        let availability = availability().expect("query host memory availability");
        assert!(availability.available_physical_bytes <= facts.physical_bytes.get());
        assert!(!availability.semantics.as_str().is_empty());
    }
}
