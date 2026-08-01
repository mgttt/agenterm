//! Stable host memory geometry without product resource-limit policy.

pub use crate::contract::host_memory::{HostMemoryError, HostMemoryErrorKind, HostMemoryFacts};

pub fn facts() -> Result<HostMemoryFacts, HostMemoryError> {
    crate::selected::host_memory::facts()
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
}
