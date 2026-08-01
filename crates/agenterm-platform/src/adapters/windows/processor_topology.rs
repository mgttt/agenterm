use windows_sys::Win32::Foundation::ERROR_INVALID_PARAMETER;
use windows_sys::Win32::System::SystemInformation::{
    LOGICAL_PROCESSOR_RELATIONSHIP, RelationNumaNode, RelationNumaNodeEx, RelationProcessorCore,
    RelationProcessorPackage,
};
use windows_sys::Win32::System::Threading::{
    GetActiveProcessorCount, GetActiveProcessorGroupCount,
};

use crate::contract::processor_topology::{
    ProcessorTopologyError, ProcessorTopologyErrorKind, ProcessorTopologyFacts,
};

const ALL_PROCESSOR_GROUPS: u16 = 0xffff;

pub(crate) fn facts() -> Result<ProcessorTopologyFacts, ProcessorTopologyError> {
    let logical = unsafe { GetActiveProcessorCount(ALL_PROCESSOR_GROUPS) };
    if logical == 0 {
        return Err(query_error("GetActiveProcessorCount"));
    }
    let groups = unsafe { GetActiveProcessorGroupCount() };
    if groups == 0 {
        return Err(query_error("GetActiveProcessorGroupCount"));
    }

    let physical_cores = relationship_count(RelationProcessorCore)
        .map_err(|error| native_error("RelationProcessorCore", error))?;
    let packages = relationship_count(RelationProcessorPackage)
        .map_err(|error| native_error("RelationProcessorPackage", error))?;
    let numa_nodes = match relationship_count(RelationNumaNodeEx) {
        Ok(count) => count,
        Err(error) if error.raw_os_error() == Some(ERROR_INVALID_PARAMETER as i32) => {
            relationship_count(RelationNumaNode)
                .map_err(|error| native_error("RelationNumaNode", error))?
        }
        Err(error) => return Err(native_error("RelationNumaNodeEx", error)),
    };

    ProcessorTopologyFacts::from_counts(
        u64::from(logical),
        Some(physical_cores),
        Some(packages),
        Some(numa_nodes),
        Some(u64::from(groups)),
    )
}

fn relationship_count(relationship: LOGICAL_PROCESSOR_RELATIONSHIP) -> std::io::Result<u64> {
    let records = super::logical_processor::query_records(
        relationship,
        returned_relationship(relationship),
        |_| Ok(()),
    )?;
    u64::try_from(records.len())
        .map_err(|_| std::io::Error::other("topology record count overflow"))
}

fn returned_relationship(
    requested: LOGICAL_PROCESSOR_RELATIONSHIP,
) -> LOGICAL_PROCESSOR_RELATIONSHIP {
    // RelationNumaNodeEx requests all processor groups for each NUMA node, but
    // Windows still labels every returned record as RelationNumaNode.
    if requested == RelationNumaNodeEx {
        RelationNumaNode
    } else {
        requested
    }
}

fn query_error(context: &str) -> ProcessorTopologyError {
    native_error(context, std::io::Error::last_os_error())
}

fn native_error(context: &str, error: std::io::Error) -> ProcessorTopologyError {
    let kind = if error.kind() == std::io::ErrorKind::InvalidData {
        ProcessorTopologyErrorKind::MalformedNativeData
    } else {
        ProcessorTopologyErrorKind::Query
    };
    ProcessorTopologyError::new(kind, format!("{context}: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numa_node_ex_query_expects_numa_node_records() {
        assert_eq!(returned_relationship(RelationNumaNodeEx), RelationNumaNode);
        assert_eq!(
            returned_relationship(RelationProcessorCore),
            RelationProcessorCore
        );
    }
}
