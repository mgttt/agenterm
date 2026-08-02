//! Native process-containment objects with exact process-reference assignment.

use std::fmt;

use crate::process_reference::ProcessReference;

/// Native containment limits after the embedding product has applied its units and policy.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ProcessContainmentLimits {
    pub memory_bytes: Option<u64>,
    /// CPU hard cap in hundredths of one percent (`1..=10_000`).
    pub cpu_rate_hundredths: Option<u32>,
    pub active_processes: Option<u32>,
}

/// Mechanism-level behavior for a newly created containment object.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ProcessContainmentOptions {
    pub terminate_on_last_close: bool,
    pub allow_breakaway: bool,
    pub limits: ProcessContainmentLimits,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum ProcessContainmentErrorKind {
    InvalidInput,
    AlreadyExists,
    NotFound,
    Unsupported,
    Closed,
    NativeFailure,
}

#[derive(Debug)]
pub struct ProcessContainmentError {
    kind: ProcessContainmentErrorKind,
    operation: &'static str,
    native_code: Option<u32>,
    detail: String,
}

impl ProcessContainmentError {
    pub const fn kind(&self) -> ProcessContainmentErrorKind {
        self.kind
    }

    pub const fn operation(&self) -> &'static str {
        self.operation
    }

    pub const fn native_code(&self) -> Option<u32> {
        self.native_code
    }

    pub(crate) fn new(
        kind: ProcessContainmentErrorKind,
        operation: &'static str,
        native_code: Option<u32>,
        detail: impl Into<String>,
    ) -> Self {
        Self {
            kind,
            operation,
            native_code,
            detail: detail.into(),
        }
    }
}

impl fmt::Display for ProcessContainmentError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{} failed: {}", self.operation, self.detail)
    }
}

impl std::error::Error for ProcessContainmentError {}

/// An owned native containment object.
pub struct ProcessContainment(crate::selected::process_containment::ProcessContainment);

impl ProcessContainment {
    /// Creates an anonymous or explicitly named containment object.
    ///
    /// Names are native object identities, not paths. Product prefixes, reuse policy, and
    /// lifecycle waiting remain the caller's responsibility.
    pub fn create(
        name: Option<&str>,
        options: ProcessContainmentOptions,
    ) -> Result<Self, ProcessContainmentError> {
        validate_name(name)?;
        validate_options(options)?;
        crate::selected::process_containment::ProcessContainment::create(name, options).map(Self)
    }

    /// Opens an existing named containment object with control and query access.
    pub fn open(name: &str) -> Result<Self, ProcessContainmentError> {
        validate_name(Some(name))?;
        crate::selected::process_containment::ProcessContainment::open(name).map(Self)
    }

    /// Assigns the exact retained process object to this containment object.
    pub fn assign(&self, process: &ProcessReference) -> Result<(), ProcessContainmentError> {
        self.0.assign(process)
    }

    /// Reports exact process-object membership.
    pub fn contains(&self, process: &ProcessReference) -> Result<bool, ProcessContainmentError> {
        self.0.contains(process)
    }

    /// Returns a race-tolerant snapshot of current member process identifiers.
    pub fn process_ids(&self) -> Result<Vec<u32>, ProcessContainmentError> {
        self.0.process_ids()
    }

    /// Terminates every current member using one caller-selected native exit code.
    pub fn terminate(&self, exit_code: u32) -> Result<(), ProcessContainmentError> {
        self.0.terminate(exit_code)
    }

    /// Releases this owner's native reference without changing product state.
    pub fn close(&mut self) {
        self.0.close();
    }
}

fn validate_name(name: Option<&str>) -> Result<(), ProcessContainmentError> {
    if let Some(name) = name {
        if name.is_empty() {
            return Err(invalid(
                "validate-containment-name",
                "name must not be empty",
            ));
        }
        if name.encode_utf16().any(|unit| unit == 0) {
            return Err(invalid("validate-containment-name", "name contains NUL"));
        }
    }
    Ok(())
}

fn validate_options(options: ProcessContainmentOptions) -> Result<(), ProcessContainmentError> {
    if options.limits.memory_bytes == Some(0) {
        return Err(invalid(
            "validate-containment-limits",
            "memory limit must be nonzero when present",
        ));
    }
    if options.limits.active_processes == Some(0) {
        return Err(invalid(
            "validate-containment-limits",
            "active process limit must be nonzero when present",
        ));
    }
    if options
        .limits
        .cpu_rate_hundredths
        .is_some_and(|rate| !(1..=10_000).contains(&rate))
    {
        return Err(invalid(
            "validate-containment-limits",
            "CPU rate must be between 1 and 10000 hundredths of one percent",
        ));
    }
    Ok(())
}

fn invalid(operation: &'static str, detail: &'static str) -> ProcessContainmentError {
    ProcessContainmentError::new(
        ProcessContainmentErrorKind::InvalidInput,
        operation,
        None,
        detail,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_ambiguous_names_and_zero_or_out_of_range_limits() {
        for name in [Some(""), Some("bad\0name")] {
            assert_eq!(
                ProcessContainment::create(name, ProcessContainmentOptions::default())
                    .err()
                    .expect("invalid name")
                    .kind(),
                ProcessContainmentErrorKind::InvalidInput
            );
        }
        for limits in [
            ProcessContainmentLimits {
                memory_bytes: Some(0),
                ..ProcessContainmentLimits::default()
            },
            ProcessContainmentLimits {
                cpu_rate_hundredths: Some(10_001),
                ..ProcessContainmentLimits::default()
            },
            ProcessContainmentLimits {
                active_processes: Some(0),
                ..ProcessContainmentLimits::default()
            },
        ] {
            assert_eq!(
                ProcessContainment::create(
                    None,
                    ProcessContainmentOptions {
                        limits,
                        ..ProcessContainmentOptions::default()
                    }
                )
                .err()
                .expect("invalid limits")
                .kind(),
                ProcessContainmentErrorKind::InvalidInput
            );
        }
    }

    #[test]
    fn capability_does_not_claim_full_process() {
        assert_eq!(
            crate::capability_status(crate::Capability::ProcessContainment),
            crate::CapabilityStatus::Available
        );
        #[cfg(not(feature = "process"))]
        assert_eq!(
            crate::capability_status(crate::Capability::Process),
            crate::CapabilityStatus::Unsupported {
                reason: std::borrow::Cow::Borrowed("feature-disabled")
            }
        );
    }
}
