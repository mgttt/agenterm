use crate::{
    app_container_process::{AppContainerProcessError, PreparedAppContainerProcess},
    process_reference::ProcessReference,
};

pub(crate) struct ResumeToken;

pub(crate) fn spawn(
    _options: &PreparedAppContainerProcess<'_>,
) -> Result<(ProcessReference, ResumeToken), AppContainerProcessError> {
    Err(AppContainerProcessError::unsupported(
        "spawn-app-container-process",
    ))
}

pub(crate) fn resume(_token: &mut ResumeToken) -> Result<(), AppContainerProcessError> {
    Err(AppContainerProcessError::unsupported(
        "resume-app-container-process",
    ))
}

pub(crate) fn abort_suspended(_process: &ProcessReference, _token: &mut ResumeToken) {}
