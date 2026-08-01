use crate::contract::host_memory::{
    HostMemoryError, HostMemoryErrorKind, HostMemoryFacts, checked_facts,
};

pub(crate) fn facts() -> Result<HostMemoryFacts, HostMemoryError> {
    let page_size = positive_sysconf(libc::_SC_PAGESIZE, "page size")?;
    let physical_pages = positive_sysconf(libc::_SC_PHYS_PAGES, "physical page count")?;
    let physical_bytes = checked_page_product(physical_pages, page_size)?;
    checked_facts(page_size, page_size, physical_bytes)
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

fn checked_page_product(page_count: u64, page_size: u64) -> Result<u64, HostMemoryError> {
    page_count.checked_mul(page_size).ok_or_else(|| {
        HostMemoryError::new(
            HostMemoryErrorKind::Overflow,
            "physical page count multiplied by page size overflowed u64",
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn physical_page_product_rejects_overflow() {
        let error = checked_page_product(u64::MAX, 4096).expect_err("reject overflow");
        assert_eq!(error.kind(), HostMemoryErrorKind::Overflow);
    }
}
