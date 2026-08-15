//! Opaque identities for one provider installation and its current desktop session.

use std::{borrow::Cow, fmt};

pub const CURRENT_TARGET_BINDING_VERSION: u16 = 1;
pub const CURRENT_TARGET_ID_BYTES: usize = 32;

#[derive(Clone, Copy, Eq, Hash, PartialEq)]
pub struct ProviderIdentity([u8; CURRENT_TARGET_ID_BYTES]);

impl ProviderIdentity {
    pub(crate) const fn new(bytes: [u8; CURRENT_TARGET_ID_BYTES]) -> Self {
        Self(bytes)
    }

    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; CURRENT_TARGET_ID_BYTES] {
        &self.0
    }
}

impl fmt::Debug for ProviderIdentity {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("ProviderIdentity(<opaque>)")
    }
}

#[derive(Clone, Copy, Eq, Hash, PartialEq)]
pub struct SessionIdentity([u8; CURRENT_TARGET_ID_BYTES]);

impl SessionIdentity {
    pub(crate) const fn new(bytes: [u8; CURRENT_TARGET_ID_BYTES]) -> Self {
        Self(bytes)
    }

    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; CURRENT_TARGET_ID_BYTES] {
        &self.0
    }
}

impl fmt::Debug for SessionIdentity {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("SessionIdentity(<opaque>)")
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct CurrentTargetBinding {
    version: u16,
    provider: ProviderIdentity,
    session: SessionIdentity,
}

impl CurrentTargetBinding {
    pub(crate) const fn new(provider: ProviderIdentity, session: SessionIdentity) -> Self {
        Self {
            version: CURRENT_TARGET_BINDING_VERSION,
            provider,
            session,
        }
    }

    #[must_use]
    pub const fn version(&self) -> u16 {
        self.version
    }

    #[must_use]
    pub const fn provider(&self) -> ProviderIdentity {
        self.provider
    }

    #[must_use]
    pub const fn session(&self) -> SessionIdentity {
        self.session
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[non_exhaustive]
pub enum CurrentTargetBindingErrorKind {
    Unsupported,
    Missing,
    Corrupt,
    Permission,
    Contended,
    Entropy,
    Native,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CurrentTargetBindingError {
    kind: CurrentTargetBindingErrorKind,
    code: Cow<'static, str>,
    message: String,
}

impl CurrentTargetBindingError {
    pub fn new(
        kind: CurrentTargetBindingErrorKind,
        code: impl Into<Cow<'static, str>>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            kind,
            code: code.into(),
            message: message.into(),
        }
    }

    #[must_use]
    pub const fn kind(&self) -> CurrentTargetBindingErrorKind {
        self.kind
    }

    #[must_use]
    pub fn code(&self) -> &str {
        &self.code
    }

    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for CurrentTargetBindingError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "current target binding failed ({}): {}",
            self.code, self.message
        )
    }
}

impl std::error::Error for CurrentTargetBindingError {}
