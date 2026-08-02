//! AppContainer profile, SID, and capability primitives.
//!
//! Profile naming, reuse, deletion timing, and capability policy belong to the
//! embedding product. This module owns only the native profile transaction and
//! exact SID storage. Non-Windows hosts preserve the API and return a typed
//! unsupported result instead of pretending to provide equivalent isolation.

#[cfg(windows)]
pub use crate::adapters::windows::app_container::{
    AppContainerCapability, AppContainerCapabilityKind, AppContainerCapabilitySid,
    AppContainerProfileError, AppContainerProfileErrorKind, OwnedAppContainerSid, create_profile,
    delete_profile, derive_profile_sid, sid_string,
};

#[cfg(any(target_os = "linux", target_os = "macos"))]
mod unsupported {
    use std::fmt;

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    #[non_exhaustive]
    pub enum AppContainerProfileErrorKind {
        InvalidInput,
        AlreadyExists,
        Unsupported,
        NativeFailure,
    }

    impl AppContainerProfileErrorKind {
        #[must_use]
        pub const fn as_str(self) -> &'static str {
            match self {
                Self::InvalidInput => "invalid-input",
                Self::AlreadyExists => "already-exists",
                Self::Unsupported => "unsupported",
                Self::NativeFailure => "native-failure",
            }
        }
    }

    #[derive(Debug)]
    pub struct AppContainerProfileError {
        kind: AppContainerProfileErrorKind,
        operation: &'static str,
        detail: &'static str,
    }

    impl AppContainerProfileError {
        #[must_use]
        pub const fn kind(&self) -> AppContainerProfileErrorKind {
            self.kind
        }

        #[must_use]
        pub const fn operation(&self) -> &'static str {
            self.operation
        }

        #[must_use]
        pub const fn hresult(&self) -> Option<u32> {
            None
        }

        #[must_use]
        pub const fn win32_code(&self) -> Option<u32> {
            None
        }

        const fn invalid(operation: &'static str) -> Self {
            Self {
                kind: AppContainerProfileErrorKind::InvalidInput,
                operation,
                detail: "SID bytes are invalid",
            }
        }

        const fn unsupported(operation: &'static str) -> Self {
            Self {
                kind: AppContainerProfileErrorKind::Unsupported,
                operation,
                detail: "AppContainer profiles are only available on Windows",
            }
        }
    }

    impl fmt::Display for AppContainerProfileError {
        fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(formatter, "{} failed: {}", self.operation, self.detail)
        }
    }

    impl std::error::Error for AppContainerProfileError {}

    #[derive(Clone, Copy, Debug)]
    pub struct AppContainerCapability<'a> {
        sid: &'a [u8],
        attributes: u32,
    }

    impl<'a> AppContainerCapability<'a> {
        pub fn enabled(sid: &'a [u8]) -> Result<Self, AppContainerProfileError> {
            Self::new(sid, 0x0000_0004)
        }

        pub fn new(sid: &'a [u8], attributes: u32) -> Result<Self, AppContainerProfileError> {
            validate_sid_bytes("AppContainerCapability", sid)?;
            Ok(Self { sid, attributes })
        }

        #[must_use]
        pub const fn sid(self) -> &'a [u8] {
            self.sid
        }

        #[must_use]
        pub const fn attributes(self) -> u32 {
            self.attributes
        }
    }

    #[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
    pub enum AppContainerCapabilityKind {
        InternetClient,
        InternetClientServer,
        PrivateNetworkClientServer,
    }

    impl AppContainerCapabilityKind {
        pub const ALL: [Self; 3] = [
            Self::InternetClient,
            Self::InternetClientServer,
            Self::PrivateNetworkClientServer,
        ];

        #[must_use]
        pub const fn as_str(self) -> &'static str {
            match self {
                Self::InternetClient => "internet-client",
                Self::InternetClientServer => "internet-client-server",
                Self::PrivateNetworkClientServer => "private-network-client-server",
            }
        }
    }

    #[derive(Debug)]
    pub struct AppContainerCapabilitySid {
        kind: AppContainerCapabilityKind,
        bytes: Vec<u8>,
    }

    impl AppContainerCapabilitySid {
        pub fn well_known(
            _kind: AppContainerCapabilityKind,
        ) -> Result<Self, AppContainerProfileError> {
            Err(AppContainerProfileError::unsupported(
                "create-well-known-app-container-capability-sid",
            ))
        }

        #[must_use]
        pub const fn kind(&self) -> AppContainerCapabilityKind {
            self.kind
        }

        #[must_use]
        pub fn as_bytes(&self) -> &[u8] {
            &self.bytes
        }

        pub fn string(&self) -> Result<String, AppContainerProfileError> {
            sid_string(self.as_bytes())
        }
    }

    #[derive(Debug)]
    pub struct OwnedAppContainerSid {
        bytes: Vec<u8>,
    }

    impl OwnedAppContainerSid {
        #[must_use]
        pub fn as_bytes(&self) -> &[u8] {
            &self.bytes
        }

        pub fn string(&self) -> Result<String, AppContainerProfileError> {
            sid_string(self.as_bytes())
        }
    }

    pub fn sid_string(_sid: &[u8]) -> Result<String, AppContainerProfileError> {
        Err(AppContainerProfileError::unsupported(
            "format-app-container-sid",
        ))
    }

    pub fn create_profile(
        _name: &str,
        _display_name: &str,
        _description: &str,
        _capabilities: &[AppContainerCapability<'_>],
    ) -> Result<OwnedAppContainerSid, AppContainerProfileError> {
        Err(AppContainerProfileError::unsupported(
            "create-app-container-profile",
        ))
    }

    pub fn derive_profile_sid(
        _name: &str,
    ) -> Result<OwnedAppContainerSid, AppContainerProfileError> {
        Err(AppContainerProfileError::unsupported(
            "derive-app-container-profile-sid",
        ))
    }

    pub fn delete_profile(_name: &str) -> Result<(), AppContainerProfileError> {
        Err(AppContainerProfileError::unsupported(
            "delete-app-container-profile",
        ))
    }

    fn validate_sid_bytes(
        operation: &'static str,
        sid: &[u8],
    ) -> Result<(), AppContainerProfileError> {
        let valid = sid.len() >= 8
            && sid[0] == 1
            && usize::from(sid[1]) <= 15
            && 8_usize
                .checked_add(usize::from(sid[1]) * 4)
                .is_some_and(|expected| expected == sid.len());
        if valid {
            Ok(())
        } else {
            Err(AppContainerProfileError::invalid(operation))
        }
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
pub use unsupported::*;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capability_kinds_and_errors_have_stable_names() {
        assert_eq!(
            AppContainerCapabilityKind::ALL.map(AppContainerCapabilityKind::as_str),
            [
                "internet-client",
                "internet-client-server",
                "private-network-client-server"
            ]
        );
        assert_eq!(
            [
                AppContainerProfileErrorKind::InvalidInput,
                AppContainerProfileErrorKind::AlreadyExists,
                AppContainerProfileErrorKind::Unsupported,
                AppContainerProfileErrorKind::NativeFailure,
            ]
            .map(AppContainerProfileErrorKind::as_str),
            [
                "invalid-input",
                "already-exists",
                "unsupported",
                "native-failure"
            ]
        );
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn unsupported_hosts_preserve_the_profile_contract() {
        for error in [
            create_profile("name", "display", "description", &[])
                .expect_err("profile creation must be unsupported"),
            derive_profile_sid("name").expect_err("SID derivation must be unsupported"),
        ] {
            assert_eq!(error.kind(), AppContainerProfileErrorKind::Unsupported);
            assert_eq!(error.hresult(), None);
            assert_eq!(error.win32_code(), None);
        }
        assert_eq!(
            delete_profile("name")
                .expect_err("profile deletion must be unsupported")
                .kind(),
            AppContainerProfileErrorKind::Unsupported
        );
    }
}
