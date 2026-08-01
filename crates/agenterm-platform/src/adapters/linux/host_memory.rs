use crate::contract::host_memory::{
    HostMemoryAvailability, HostMemoryAvailabilitySemantics, HostMemoryError, HostMemoryErrorKind,
    HostMemoryFacts, checked_availability, checked_facts,
};

pub(crate) fn facts() -> Result<HostMemoryFacts, HostMemoryError> {
    let page_size = positive_sysconf(libc::_SC_PAGESIZE, "page size")?;
    let physical_pages = positive_sysconf(libc::_SC_PHYS_PAGES, "physical page count")?;
    let physical_bytes = checked_page_product(physical_pages, page_size)?;
    checked_facts(page_size, page_size, physical_bytes)
}

pub(crate) fn availability() -> Result<HostMemoryAvailability, HostMemoryError> {
    let meminfo = std::fs::read_to_string("/proc/meminfo").map_err(|error| {
        HostMemoryError::new(
            HostMemoryErrorKind::Query,
            format!("read /proc/meminfo: {error}"),
        )
    })?;
    let available_physical_bytes = parse_mem_available(&meminfo)?;
    checked_availability(
        available_physical_bytes,
        facts()?.physical_bytes.get(),
        HostMemoryAvailabilitySemantics::LinuxMemAvailable,
    )
}

fn parse_mem_available(meminfo: &str) -> Result<u64, HostMemoryError> {
    let line = meminfo
        .lines()
        .find(|line| line.starts_with("MemAvailable:"))
        .ok_or_else(|| {
            HostMemoryError::new(
                HostMemoryErrorKind::InvalidValue,
                "/proc/meminfo does not contain MemAvailable",
            )
        })?;
    let mut fields = line.split_ascii_whitespace();
    let key = fields.next();
    let kibibytes = fields.next().and_then(|value| value.parse::<u64>().ok());
    let unit = fields.next();
    if key != Some("MemAvailable:") || unit != Some("kB") || fields.next().is_some() {
        return Err(HostMemoryError::new(
            HostMemoryErrorKind::InvalidValue,
            format!("invalid MemAvailable line: {line}"),
        ));
    }
    kibibytes
        .ok_or_else(|| {
            HostMemoryError::new(
                HostMemoryErrorKind::InvalidValue,
                format!("invalid MemAvailable value: {line}"),
            )
        })?
        .checked_mul(1024)
        .ok_or_else(|| {
            HostMemoryError::new(
                HostMemoryErrorKind::Overflow,
                "MemAvailable kibibytes overflowed u64 bytes",
            )
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

    #[test]
    fn mem_available_parser_requires_the_kernel_unit_and_shape() {
        assert_eq!(
            parse_mem_available("MemTotal: 10 kB\nMemAvailable: 7 kB\n").unwrap(),
            7 * 1024
        );
        for input in [
            "MemTotal: 10 kB\n",
            "MemAvailable: nope kB\n",
            "MemAvailable: 7 MB\n",
            "MemAvailable: 7 kB extra\n",
        ] {
            assert_eq!(
                parse_mem_available(input).unwrap_err().kind(),
                HostMemoryErrorKind::InvalidValue
            );
        }
    }
}
