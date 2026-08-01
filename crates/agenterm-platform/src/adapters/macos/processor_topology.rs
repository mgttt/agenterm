use crate::contract::processor_topology::{
    ProcessorTopologyError, ProcessorTopologyErrorKind, ProcessorTopologyFacts,
};

pub(crate) fn facts() -> Result<ProcessorTopologyFacts, ProcessorTopologyError> {
    let logical = sysctl_u32(c"hw.logicalcpu")
        .or_else(|_| sysctl_u32(c"hw.ncpu"))
        .map_err(|error| {
            ProcessorTopologyError::new(
                ProcessorTopologyErrorKind::Query,
                format!("logical processor sysctl: {error}"),
            )
        })?;
    ProcessorTopologyFacts::from_counts(
        u64::from(logical),
        optional_sysctl(c"hw.physicalcpu"),
        optional_sysctl(c"hw.packages"),
        None,
        None,
    )
}

fn optional_sysctl(name: &std::ffi::CStr) -> Option<u64> {
    sysctl_u32(name)
        .ok()
        .map(u64::from)
        .filter(|value| *value > 0)
}

fn sysctl_u32(name: &std::ffi::CStr) -> std::io::Result<u32> {
    let mut value = 0_u32;
    let mut length = std::mem::size_of_val(&value);
    if unsafe {
        libc::sysctlbyname(
            name.as_ptr(),
            (&raw mut value).cast(),
            &mut length,
            std::ptr::null_mut(),
            0,
        )
    } != 0
    {
        return Err(std::io::Error::last_os_error());
    }
    if length != std::mem::size_of_val(&value) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("{} returned {length} bytes", name.to_string_lossy()),
        ));
    }
    Ok(value)
}
