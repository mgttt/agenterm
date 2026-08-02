use crate::{
    process_containment::{
        ProcessContainmentError, ProcessContainmentErrorKind, ProcessContainmentOptions,
    },
    process_reference::ProcessReference,
};

pub struct ProcessContainment;

impl ProcessContainment {
    pub(crate) fn create(
        _name: Option<&str>,
        _options: ProcessContainmentOptions,
    ) -> Result<Self, ProcessContainmentError> {
        Err(unsupported("create-process-containment"))
    }

    pub(crate) fn open(_name: &str) -> Result<Self, ProcessContainmentError> {
        Err(unsupported("open-process-containment"))
    }

    pub(crate) fn assign(
        &self,
        _process: &ProcessReference,
    ) -> Result<(), ProcessContainmentError> {
        Err(unsupported("assign-process-containment"))
    }

    pub(crate) fn contains(
        &self,
        _process: &ProcessReference,
    ) -> Result<bool, ProcessContainmentError> {
        Err(unsupported("query-process-containment-membership"))
    }

    pub(crate) fn process_ids(&self) -> Result<Vec<u32>, ProcessContainmentError> {
        Err(unsupported("query-process-containment-members"))
    }

    pub(crate) fn terminate(&self, _exit_code: u32) -> Result<(), ProcessContainmentError> {
        Err(unsupported("terminate-process-containment"))
    }

    pub(crate) fn close(&mut self) {}
}

fn unsupported(operation: &'static str) -> ProcessContainmentError {
    ProcessContainmentError::new(
        ProcessContainmentErrorKind::Unsupported,
        operation,
        None,
        "native named process containment is unsupported on this host",
    )
}
