use std::num::{NonZeroU64, NonZeroUsize};

use crate::contract::host_memory::{HostMemoryError, HostMemoryErrorKind, HostMemoryFacts};

pub(crate) fn facts() -> Result<HostMemoryFacts, HostMemoryError> {
    let page_size = positive_sysconf(libc::_SC_PAGESIZE, "page size")?;
    let physical_pages = positive_sysconf(libc::_SC_PHYS_PAGES, "physical page count")?;
    let physical_bytes = physical_pages.checked_mul(page_size).ok_or_else(|| {
        HostMemoryError::new(
            HostMemoryErrorKind::Overflow,
            "physical page count multiplied by page size overflowed u64",
        )
    })?;
    let page_size = usize::try_from(page_size)
        .ok()
        .and_then(NonZeroUsize::new)
        .ok_or_else(|| {
            HostMemoryError::new(
                HostMemoryErrorKind::InvalidValue,
                "page size does not fit this process pointer width",
            )
        })?;
    Ok(HostMemoryFacts {
        page_size,
        allocation_granularity: page_size,
        physical_bytes: NonZeroU64::new(physical_bytes).expect("positive product"),
    })
}

fn positive_sysconf(key: libc::c_int, name: &str) -> Result<u64, HostMemoryError> {
    let value = unsafe { libc::sysconf(key) };
    u64::try_from(value)
        .ok()
        .filter(|value| *value > 0)
        .ok_or_else(|| {
            HostMemoryError::new(
                HostMemoryErrorKind::InvalidValue,
                format!("host reported invalid {name}: {value}"),
            )
        })
}
