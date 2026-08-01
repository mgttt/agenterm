use crate::contract::host_memory::{
    HostMemoryError, HostMemoryErrorKind, HostMemoryFacts, checked_facts,
};

pub(crate) fn facts() -> Result<HostMemoryFacts, HostMemoryError> {
    use windows_sys::Win32::System::SystemInformation::{
        GetSystemInfo, GlobalMemoryStatusEx, MEMORYSTATUSEX, SYSTEM_INFO,
    };

    let mut system = SYSTEM_INFO::default();
    unsafe { GetSystemInfo(&mut system) };
    let mut memory = MEMORYSTATUSEX {
        dwLength: std::mem::size_of::<MEMORYSTATUSEX>() as u32,
        ..Default::default()
    };
    if unsafe { GlobalMemoryStatusEx(&mut memory) } == 0 {
        return Err(HostMemoryError::new(
            HostMemoryErrorKind::Query,
            std::io::Error::last_os_error().to_string(),
        ));
    }
    checked_facts(
        u64::from(system.dwPageSize),
        u64::from(system.dwAllocationGranularity),
        memory.ullTotalPhys,
    )
}
