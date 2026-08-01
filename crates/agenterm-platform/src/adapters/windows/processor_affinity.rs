use windows_sys::Win32::System::Threading::{
    GetActiveProcessorGroupCount, GetCurrentProcess, GetProcessAffinityMask,
    GetProcessGroupAffinity,
};

use crate::contract::processor_affinity::{
    LogicalProcessorLocation, ProcessorAffinityError, ProcessorAffinityErrorKind,
    ProcessorAffinityFacts, ProcessorSetSemantics,
};

pub(crate) fn current_process() -> Result<ProcessorAffinityFacts, ProcessorAffinityError> {
    let handle = unsafe { GetCurrentProcess() };
    let active_group_count = unsafe { GetActiveProcessorGroupCount() };
    if active_group_count == 0 {
        return Err(query_error("GetActiveProcessorGroupCount"));
    }
    let mut groups = vec![0_u16; usize::from(active_group_count)];
    let mut group_count = active_group_count;
    if unsafe { GetProcessGroupAffinity(handle, &mut group_count, groups.as_mut_ptr()) } == 0 {
        return Err(query_error("GetProcessGroupAffinity"));
    }
    let returned = usize::from(group_count);
    if returned == 0 || returned > groups.len() {
        return Err(ProcessorAffinityError::new(
            ProcessorAffinityErrorKind::MalformedNativeData,
            format!("GetProcessGroupAffinity returned invalid count {group_count}"),
        ));
    }
    groups.truncate(returned);
    if groups.len() != 1 {
        return Err(ProcessorAffinityError::new(
            ProcessorAffinityErrorKind::Unsupported,
            format!(
                "process spans {} processor groups; one affinity mask would be incomplete",
                groups.len()
            ),
        ));
    }

    let mut process_mask = 0_usize;
    let mut system_mask = 0_usize;
    if unsafe { GetProcessAffinityMask(handle, &mut process_mask, &mut system_mask) } == 0 {
        return Err(query_error("GetProcessAffinityMask"));
    }
    if process_mask == 0 || process_mask & !system_mask != 0 {
        return Err(ProcessorAffinityError::new(
            ProcessorAffinityErrorKind::MalformedNativeData,
            format!("invalid process/system affinity masks {process_mask:#x}/{system_mask:#x}"),
        ));
    }
    ProcessorAffinityFacts::from_locations(
        locations_from_mask(groups[0], process_mask),
        ProcessorSetSemantics::ProcessAffinityMask,
    )
}

fn locations_from_mask(group: u16, mask: usize) -> Vec<LogicalProcessorLocation> {
    (0..usize::BITS)
        .filter(|bit| mask & (1_usize << bit) != 0)
        .map(|index| LogicalProcessorLocation { group, index })
        .collect()
}

fn query_error(context: &str) -> ProcessorAffinityError {
    ProcessorAffinityError::new(
        ProcessorAffinityErrorKind::Query,
        format!("{context}: {}", std::io::Error::last_os_error()),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mask_locations_preserve_group_and_bit_indexes() {
        assert_eq!(
            locations_from_mask(3, 0b1010),
            vec![
                LogicalProcessorLocation { group: 3, index: 1 },
                LogicalProcessorLocation { group: 3, index: 3 },
            ]
        );
    }
}
