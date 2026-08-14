use std::time::Duration;

use crate::contract::desktop_host::{DesktopActionSpec, DesktopHostError};

pub(crate) struct DesktopHost;

impl DesktopHost {
    pub(crate) fn open(_actions: Vec<DesktopActionSpec>) -> Result<Self, DesktopHostError> {
        Err(DesktopHostError::unsupported("unsupported_platform"))
    }

    pub(crate) fn poll_action(
        &mut self,
        _timeout: Duration,
    ) -> Result<Option<u32>, DesktopHostError> {
        Err(DesktopHostError::unsupported("unsupported_platform"))
    }

    pub(crate) fn close(&mut self) -> Result<(), DesktopHostError> {
        Err(DesktopHostError::unsupported("unsupported_platform"))
    }
}
