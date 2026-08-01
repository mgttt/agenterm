use windows_sys::Win32::Foundation::{ERROR_INSUFFICIENT_BUFFER, ERROR_INVALID_PARAMETER};
use windows_sys::Win32::System::SystemInformation::{
    GetLogicalProcessorInformationEx, LOGICAL_PROCESSOR_RELATIONSHIP, RelationNumaNode,
    RelationNumaNodeEx, RelationProcessorCore, RelationProcessorPackage,
    SYSTEM_LOGICAL_PROCESSOR_INFORMATION_EX,
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
    let mut length = 0_u32;
    let first = unsafe {
        GetLogicalProcessorInformationEx(relationship, std::ptr::null_mut(), &mut length)
    };
    let first_error = std::io::Error::last_os_error();
    if first != 0 || first_error.raw_os_error() != Some(ERROR_INSUFFICIENT_BUFFER as i32) {
        return Err(first_error);
    }
    if length < 8 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("topology relationship {relationship} reported {length} bytes"),
        ));
    }

    let bytes = length as usize;
    let mut storage = vec![0_usize; bytes.div_ceil(std::mem::size_of::<usize>())];
    let mut written = length;
    if unsafe {
        GetLogicalProcessorInformationEx(relationship, storage.as_mut_ptr().cast(), &mut written)
    } == 0
    {
        return Err(std::io::Error::last_os_error());
    }
    if written as usize > storage.len() * std::mem::size_of::<usize>() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "topology query wrote beyond the requested buffer length",
        ));
    }

    count_records(
        storage.as_ptr().cast(),
        written as usize,
        returned_relationship(relationship),
    )
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

fn count_records(
    bytes: *const u8,
    length: usize,
    expected: LOGICAL_PROCESSOR_RELATIONSHIP,
) -> std::io::Result<u64> {
    let mut offset = 0_usize;
    let mut count = 0_u64;
    while offset < length {
        if length - offset < 8 {
            return Err(malformed("truncated topology record header"));
        }
        let record = unsafe {
            bytes
                .add(offset)
                .cast::<SYSTEM_LOGICAL_PROCESSOR_INFORMATION_EX>()
        };
        let relationship = unsafe { std::ptr::addr_of!((*record).Relationship).read_unaligned() };
        let size = unsafe { std::ptr::addr_of!((*record).Size).read_unaligned() } as usize;
        if relationship != expected {
            return Err(malformed(
                "topology record relationship changed inside the buffer",
            ));
        }
        if size < 8 || size > length - offset {
            return Err(malformed("topology record has an invalid size"));
        }
        count = count
            .checked_add(1)
            .ok_or_else(|| malformed("topology record count overflow"))?;
        offset += size;
    }
    if count == 0 {
        return Err(malformed("topology query returned no records"));
    }
    Ok(count)
}

fn malformed(message: &'static str) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::InvalidData, message)
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
    fn record_parser_rejects_truncation_and_wrong_relationship() {
        let truncated = [0_u8; 7];
        assert_eq!(
            count_records(truncated.as_ptr(), truncated.len(), RelationProcessorCore)
                .unwrap_err()
                .kind(),
            std::io::ErrorKind::InvalidData
        );

        let mut record = SYSTEM_LOGICAL_PROCESSOR_INFORMATION_EX {
            Relationship: RelationProcessorPackage,
            Size: 8,
            ..Default::default()
        };
        assert_eq!(
            count_records((&raw mut record).cast(), 8, RelationProcessorCore,)
                .unwrap_err()
                .kind(),
            std::io::ErrorKind::InvalidData
        );
    }

    #[test]
    fn numa_node_ex_query_expects_numa_node_records() {
        assert_eq!(returned_relationship(RelationNumaNodeEx), RelationNumaNode);
        assert_eq!(
            returned_relationship(RelationProcessorCore),
            RelationProcessorCore
        );
    }
}
