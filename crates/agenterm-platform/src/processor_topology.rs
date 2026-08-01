//! Current-host processor and NUMA topology facts.

pub use crate::contract::processor_topology::{
    ProcessorTopologyError, ProcessorTopologyErrorKind, ProcessorTopologyFacts,
};

pub fn facts() -> Result<ProcessorTopologyFacts, ProcessorTopologyError> {
    crate::selected::processor_topology::facts()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_a_coherent_current_host_topology() {
        assert_eq!(
            crate::capability_status(crate::Capability::ProcessorTopology),
            crate::CapabilityStatus::Available
        );
        let facts = facts().expect("query current host processor topology");
        eprintln!("processor topology: {facts:?}");
        assert!(facts.system_logical_processors.get() > 0);
        assert!(facts.physical_cores.is_none_or(|count| count.get() > 0));
        assert!(facts.packages.is_none_or(|count| count.get() > 0));
        assert!(facts.numa_nodes.is_none_or(|count| count.get() > 0));
        assert!(facts.processor_groups.is_none_or(|count| count.get() > 0));
    }
}
