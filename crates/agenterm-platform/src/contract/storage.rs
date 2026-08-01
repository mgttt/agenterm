//! Product-neutral capacity facts for the volume containing a host path.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct VolumeSpace {
    pub total_bytes: std::num::NonZeroU64,
    /// Bytes available to the current user, including quota effects.
    pub available_bytes: u64,
    pub allocation_unit: std::num::NonZeroU64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum StorageErrorKind {
    Path,
    Query,
    InvalidValue,
    Overflow,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StorageError {
    kind: StorageErrorKind,
    detail: String,
}

impl StorageError {
    pub(crate) fn new(kind: StorageErrorKind, detail: impl Into<String>) -> Self {
        Self {
            kind,
            detail: detail.into(),
        }
    }

    pub const fn kind(&self) -> StorageErrorKind {
        self.kind
    }

    pub fn detail(&self) -> &str {
        &self.detail
    }
}

impl std::fmt::Display for StorageError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "storage {:?}: {}", self.kind, self.detail)
    }
}

impl std::error::Error for StorageError {}

pub(crate) fn checked_space(
    total_bytes: u64,
    available_bytes: u64,
    allocation_unit: u64,
) -> Result<VolumeSpace, StorageError> {
    let total_bytes = std::num::NonZeroU64::new(total_bytes).ok_or_else(|| {
        StorageError::new(StorageErrorKind::InvalidValue, "volume capacity is zero")
    })?;
    let allocation_unit = std::num::NonZeroU64::new(allocation_unit).ok_or_else(|| {
        StorageError::new(StorageErrorKind::InvalidValue, "allocation unit is zero")
    })?;
    if available_bytes > total_bytes.get() {
        return Err(StorageError::new(
            StorageErrorKind::InvalidValue,
            "available bytes exceed total volume capacity",
        ));
    }
    if allocation_unit.get() > total_bytes.get() {
        return Err(StorageError::new(
            StorageErrorKind::InvalidValue,
            "allocation unit exceeds total volume capacity",
        ));
    }
    Ok(VolumeSpace {
        total_bytes,
        available_bytes,
        allocation_unit,
    })
}

pub(crate) fn checked_product(
    count: impl Into<u64>,
    unit: u64,
    name: &str,
) -> Result<u64, StorageError> {
    count.into().checked_mul(unit).ok_or_else(|| {
        StorageError::new(
            StorageErrorKind::Overflow,
            format!("{name} multiplied by allocation unit overflowed u64"),
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn raw_space_rejects_incoherent_values() {
        for result in [
            checked_space(0, 0, 4096),
            checked_space(1024, 1025, 512),
            checked_space(1024, 0, 0),
            checked_space(1024, 0, 2048),
        ] {
            assert_eq!(
                result.expect_err("reject invalid volume facts").kind(),
                StorageErrorKind::InvalidValue
            );
        }
    }

    #[test]
    fn block_product_rejects_overflow() {
        let error = checked_product(u64::MAX, 4096, "blocks").expect_err("reject overflow");
        assert_eq!(error.kind(), StorageErrorKind::Overflow);
    }
}
