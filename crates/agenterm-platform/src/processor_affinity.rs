//! Current-process processor-affinity facts.

pub use crate::contract::processor_affinity::{
    LogicalProcessorLocation, ProcessorAffinityError, ProcessorAffinityErrorKind,
    ProcessorAffinityFacts, ProcessorSetSemantics,
};

/// Query the processor set assigned to the current host process.
///
/// The returned semantics must be inspected before treating the set as the
/// complete scheduler-allowed set. Product placement policy remains with the
/// embedding application.
pub fn current_process() -> Result<ProcessorAffinityFacts, ProcessorAffinityError> {
    crate::selected::processor_affinity::current_process()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn current_process_result_is_explicit_and_coherent() {
        assert_eq!(
            crate::capability_status(crate::Capability::ProcessorAffinity),
            crate::CapabilityStatus::Available
        );
        match current_process() {
            Ok(facts) => {
                eprintln!("processor affinity: {facts:?}");
                assert!(!facts.processors().is_empty());
                assert_eq!(facts.count().get(), facts.processors().len());
            }
            Err(error) => {
                assert_eq!(
                    error.kind(),
                    ProcessorAffinityErrorKind::Unsupported,
                    "native affinity query failed unexpectedly: {error}"
                );
            }
        }
    }
}
