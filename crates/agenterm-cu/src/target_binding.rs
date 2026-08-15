//! Verified target and desktop-session identity contract.
//!
//! Routing values such as host names, IP addresses, user names, ports and
//! display names are intentionally absent from this API. They locate a
//! transport but do not prove which machine or desktop session answered.
//! This module does not synthesize identity and contains no cryptographic
//! placeholder. Until a tier supplies a sealed verified provider, resolution
//! fails closed.

use crate::target::TargetRef;

const MAX_OPAQUE_ID_BYTES: usize = 512;

/// Opaque identity of one verified target and one exact desktop session.
///
/// Fields are private to external callers. Only a crate-owned, sealed
/// [`VerifiedIdentityProvider`] can produce a value for the resolver.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TargetBinding {
    pub(crate) tier: TargetRef,
    pub(crate) target_id: String,
    pub(crate) session_binding: String,
}

impl TargetBinding {
    pub fn tier(&self) -> TargetRef {
        self.tier
    }

    pub fn target_id(&self) -> &str {
        &self.target_id
    }

    pub fn session_binding(&self) -> &str {
        &self.session_binding
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TargetBindingErrorKind {
    /// No crate-owned provider has proved target and session identity.
    VerifiedIdentityProviderRequired,
    /// A provider could not obtain a stable target identity.
    IdentityUnavailable,
    /// A provider could not obtain an exact live desktop-session identity.
    SessionUnavailable,
    /// Transport authentication did not prove the responding target.
    UnverifiedTransport,
    /// Provider evidence described a different tier or malformed opaque ID.
    InvalidProviderEvidence,
    /// The tier has no live identity-bearing transport.
    UnsupportedTier,
}

/// Secret-free typed failure. Provider evidence and routing strings are never
/// retained, echoed or formatted by this error.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TargetBindingError {
    kind: TargetBindingErrorKind,
    tier: TargetRef,
}

impl TargetBindingError {
    fn new(kind: TargetBindingErrorKind, tier: TargetRef) -> Self {
        Self { kind, tier }
    }

    pub fn kind(&self) -> TargetBindingErrorKind {
        self.kind
    }

    pub fn tier(&self) -> TargetRef {
        self.tier
    }
}

impl std::fmt::Display for TargetBindingError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let reason = match self.kind {
            TargetBindingErrorKind::VerifiedIdentityProviderRequired => {
                "a verified identity provider is required"
            }
            TargetBindingErrorKind::IdentityUnavailable => {
                "verified target identity is unavailable"
            }
            TargetBindingErrorKind::SessionUnavailable => {
                "verified desktop-session identity is unavailable"
            }
            TargetBindingErrorKind::UnverifiedTransport => {
                "the transport did not prove target identity"
            }
            TargetBindingErrorKind::InvalidProviderEvidence => {
                "the verified identity provider returned invalid evidence"
            }
            TargetBindingErrorKind::UnsupportedTier => {
                "the target tier has no live identity-bearing transport"
            }
        };
        write!(
            formatter,
            "{} target binding failed: {reason}",
            self.tier.as_str()
        )
    }
}

impl std::error::Error for TargetBindingError {}

/// Future mechanism seam for a crate-owned provider that has already verified
/// both target and live desktop-session identity.
///
/// The trait is sealed: downstream callers cannot turn routing strings into a
/// provider or construct a binding. Provider errors are deliberately kinds,
/// not free-form strings, so secrets cannot leak through resolver diagnostics.
pub trait VerifiedIdentityProvider: sealed::Sealed {
    fn resolve_verified(&self, tier: TargetRef) -> Result<TargetBinding, TargetBindingErrorKind>;
}

/// Resolves one exact target/session binding without accepting route material.
///
/// RDP remains unsupported even if a provider is supplied because its current
/// transport is a non-live placeholder. The other tiers require a verified
/// provider and never derive identity from their target enum alone.
pub fn resolve_target_binding(
    tier: TargetRef,
    provider: Option<&dyn VerifiedIdentityProvider>,
) -> Result<TargetBinding, TargetBindingError> {
    if tier == TargetRef::Rdp {
        return Err(TargetBindingError::new(
            TargetBindingErrorKind::UnsupportedTier,
            tier,
        ));
    }
    let provider = provider.ok_or_else(|| {
        TargetBindingError::new(
            TargetBindingErrorKind::VerifiedIdentityProviderRequired,
            tier,
        )
    })?;
    let binding = provider
        .resolve_verified(tier)
        .map_err(|kind| TargetBindingError::new(kind, tier))?;
    if binding.tier != tier
        || !valid_opaque_id(&binding.target_id, "agt-cu-tgt-v1-")
        || !valid_opaque_id(&binding.session_binding, "agt-cu-ses-v1-")
    {
        return Err(TargetBindingError::new(
            TargetBindingErrorKind::InvalidProviderEvidence,
            tier,
        ));
    }
    Ok(binding)
}

fn valid_opaque_id(value: &str, prefix: &str) -> bool {
    let Some(opaque) = value.strip_prefix(prefix) else {
        return false;
    };
    opaque.len() >= 16
        && value.len() <= MAX_OPAQUE_ID_BYTES
        && opaque
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b':'))
}

mod sealed {
    pub trait Sealed {}
}

#[cfg(test)]
mod tests {
    use super::*;

    struct VerifiedFixture {
        returned_tier: TargetRef,
        target_id: &'static str,
        session_binding: &'static str,
    }

    impl sealed::Sealed for VerifiedFixture {}

    impl VerifiedIdentityProvider for VerifiedFixture {
        fn resolve_verified(
            &self,
            _tier: TargetRef,
        ) -> Result<TargetBinding, TargetBindingErrorKind> {
            Ok(TargetBinding {
                tier: self.returned_tier,
                target_id: self.target_id.to_owned(),
                session_binding: self.session_binding.to_owned(),
            })
        }
    }

    #[test]
    fn all_live_tiers_require_a_verified_provider_and_rdp_is_unsupported() {
        for tier in [TargetRef::Current, TargetRef::Ssh, TargetRef::Vnc] {
            let error = resolve_target_binding(tier, None).unwrap_err();
            assert_eq!(
                error.kind(),
                TargetBindingErrorKind::VerifiedIdentityProviderRequired
            );
            assert_eq!(error.tier(), tier);
        }
        let error = resolve_target_binding(TargetRef::Rdp, None).unwrap_err();
        assert_eq!(error.kind(), TargetBindingErrorKind::UnsupportedTier);
        assert_eq!(error.tier(), TargetRef::Rdp);
    }

    #[test]
    fn verified_provider_yields_private_binding_with_accessors() {
        let provider = VerifiedFixture {
            returned_tier: TargetRef::Current,
            target_id: "agt-cu-tgt-v1-deadbeefdeadbeef",
            session_binding: "agt-cu-ses-v1-cafebabecafebabe",
        };
        let binding = resolve_target_binding(TargetRef::Current, Some(&provider)).unwrap();
        assert_eq!(binding.tier(), TargetRef::Current);
        assert_eq!(binding.target_id(), "agt-cu-tgt-v1-deadbeefdeadbeef");
        assert_eq!(binding.session_binding(), "agt-cu-ses-v1-cafebabecafebabe");
    }

    #[test]
    fn provider_cannot_cross_tiers_or_return_arbitrary_route_text() {
        let wrong_tier = VerifiedFixture {
            returned_tier: TargetRef::Vnc,
            target_id: "agt-cu-tgt-v1-deadbeefdeadbeef",
            session_binding: "agt-cu-ses-v1-cafebabecafebabe",
        };
        assert_eq!(
            resolve_target_binding(TargetRef::Ssh, Some(&wrong_tier))
                .unwrap_err()
                .kind(),
            TargetBindingErrorKind::InvalidProviderEvidence
        );

        for forbidden in [
            "user@example.test 22",
            "192.0.2.1/desktop",
            "DISPLAY=:1",
            "secret\nsecond-line",
            "",
        ] {
            let provider = VerifiedFixture {
                returned_tier: TargetRef::Current,
                target_id: forbidden,
                session_binding: "agt-cu-ses-v1-cafebabecafebabe",
            };
            assert_eq!(
                resolve_target_binding(TargetRef::Current, Some(&provider))
                    .unwrap_err()
                    .kind(),
                TargetBindingErrorKind::InvalidProviderEvidence
            );
        }
    }

    #[test]
    fn errors_never_echo_route_or_secret_material() {
        let route = "user@private.example.test:2222";
        let display = "DISPLAY=:77";
        let secret = "provider-secret-material";
        for tier in [
            TargetRef::Current,
            TargetRef::Ssh,
            TargetRef::Vnc,
            TargetRef::Rdp,
        ] {
            let rendered = resolve_target_binding(tier, None).unwrap_err().to_string();
            assert!(!rendered.contains(route));
            assert!(!rendered.contains(display));
            assert!(!rendered.contains(secret));
        }
    }

    #[test]
    fn target_ref_round_trips_through_binding_tier() {
        for tier in [TargetRef::Current, TargetRef::Ssh, TargetRef::Vnc] {
            let provider = VerifiedFixture {
                returned_tier: tier,
                target_id: "agt-cu-tgt-v1-deadbeefdeadbeef",
                session_binding: "agt-cu-ses-v1-cafebabecafebabe",
            };
            let binding = resolve_target_binding(tier, Some(&provider)).unwrap();
            assert_eq!(TargetRef::parse(binding.tier().as_str()), Some(tier));
        }
    }

    #[test]
    fn rdp_rejects_even_a_fixture_provider() {
        let provider = VerifiedFixture {
            returned_tier: TargetRef::Rdp,
            target_id: "agt-cu-tgt-v1-deadbeefdeadbeef",
            session_binding: "agt-cu-ses-v1-cafebabecafebabe",
        };
        let error = resolve_target_binding(TargetRef::Rdp, Some(&provider)).unwrap_err();
        assert_eq!(error.kind(), TargetBindingErrorKind::UnsupportedTier);
    }
}
