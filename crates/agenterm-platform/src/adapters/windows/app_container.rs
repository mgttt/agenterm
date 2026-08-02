//! Windows AppContainer profile and SID ownership primitives.
//!
//! The caller owns profile naming, capability policy, reuse decisions, and
//! deletion timing. This adapter only transacts the native profile APIs and
//! gives their allocated SID an exact owner.

use std::{fmt, ptr::NonNull};

use windows_sys::Win32::{
    Foundation::{GetLastError, LocalFree},
    Security::{
        Authorization::ConvertSidToStringSidW, FreeSid, GetLengthSid, IsValidSid,
        Isolation::CreateAppContainerProfile, Isolation::DeleteAppContainerProfile,
        Isolation::DeriveAppContainerSidFromAppContainerName, SID_AND_ATTRIBUTES,
    },
};

const ERROR_ALREADY_EXISTS: u32 = 183;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum AppContainerProfileErrorKind {
    InvalidInput,
    AlreadyExists,
    NativeFailure,
}

impl AppContainerProfileErrorKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::InvalidInput => "invalid-input",
            Self::AlreadyExists => "already-exists",
            Self::NativeFailure => "native-failure",
        }
    }
}

#[derive(Debug)]
pub struct AppContainerProfileError {
    kind: AppContainerProfileErrorKind,
    operation: &'static str,
    hresult: Option<u32>,
    detail: String,
}

impl AppContainerProfileError {
    pub const fn kind(&self) -> AppContainerProfileErrorKind {
        self.kind
    }

    pub const fn operation(&self) -> &'static str {
        self.operation
    }

    pub const fn hresult(&self) -> Option<u32> {
        self.hresult
    }

    fn invalid(operation: &'static str, detail: impl Into<String>) -> Self {
        Self {
            kind: AppContainerProfileErrorKind::InvalidInput,
            operation,
            hresult: None,
            detail: detail.into(),
        }
    }

    fn native(operation: &'static str, hresult: i32) -> Self {
        let raw = hresult as u32;
        Self {
            kind: if raw & 0xffff == ERROR_ALREADY_EXISTS {
                AppContainerProfileErrorKind::AlreadyExists
            } else {
                AppContainerProfileErrorKind::NativeFailure
            },
            operation,
            hresult: Some(raw),
            detail: format!("HRESULT=0x{raw:08X}"),
        }
    }

    fn invalid_sid(operation: &'static str) -> Self {
        Self::invalid(operation, "native API returned an invalid SID")
    }
}

impl fmt::Display for AppContainerProfileError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{} failed: {}", self.operation, self.detail)
    }
}

impl std::error::Error for AppContainerProfileError {}

/// A validated borrowed SID and its native capability attributes.
#[derive(Clone, Copy, Debug)]
pub struct AppContainerCapability<'a> {
    sid: &'a [u8],
    attributes: u32,
}

impl<'a> AppContainerCapability<'a> {
    pub fn new(sid: &'a [u8], attributes: u32) -> Result<Self, AppContainerProfileError> {
        validate_sid_bytes("AppContainerCapability", sid)?;
        Ok(Self { sid, attributes })
    }
}

/// An AppContainer SID allocated by a Windows profile API.
#[derive(Debug)]
pub struct OwnedAppContainerSid {
    sid: NonNull<std::ffi::c_void>,
    len: usize,
}

impl OwnedAppContainerSid {
    fn from_raw(
        operation: &'static str,
        sid: *mut std::ffi::c_void,
    ) -> Result<Self, AppContainerProfileError> {
        let Some(sid) = NonNull::new(sid) else {
            return Err(AppContainerProfileError::invalid_sid(operation));
        };
        if unsafe { IsValidSid(sid.as_ptr()) } == 0 {
            unsafe { FreeSid(sid.as_ptr()) };
            return Err(AppContainerProfileError::invalid_sid(operation));
        }
        let len = unsafe { GetLengthSid(sid.as_ptr()) } as usize;
        if len == 0 {
            unsafe { FreeSid(sid.as_ptr()) };
            return Err(AppContainerProfileError::invalid_sid(operation));
        }
        Ok(Self { sid, len })
    }

    pub fn as_bytes(&self) -> &[u8] {
        unsafe { std::slice::from_raw_parts(self.sid.as_ptr().cast(), self.len) }
    }

    /// Returns the borrowed native SID pointer required by Windows security APIs.
    pub fn as_raw_sid(&self) -> *mut std::ffi::c_void {
        self.sid.as_ptr()
    }

    pub fn string(&self) -> Result<String, AppContainerProfileError> {
        sid_string(self.as_bytes())
    }
}

/// Formats one exact, validated SID byte sequence using Windows canonical form.
pub fn sid_string(sid: &[u8]) -> Result<String, AppContainerProfileError> {
    const OPERATION: &str = "ConvertSidToStringSidW";
    validate_sid_bytes(OPERATION, sid)?;
    let mut text = std::ptr::null_mut();
    if unsafe { ConvertSidToStringSidW(sid.as_ptr().cast_mut().cast(), &raw mut text) } == 0 {
        let code = unsafe { GetLastError() };
        return Err(AppContainerProfileError::invalid(
            OPERATION,
            format!("GetLastError={code}"),
        ));
    }
    let value = unsafe {
        let mut len = 0;
        while *text.add(len) != 0 {
            len += 1;
        }
        let value = String::from_utf16_lossy(std::slice::from_raw_parts(text, len));
        LocalFree(text.cast());
        value
    };
    Ok(value)
}

impl Drop for OwnedAppContainerSid {
    fn drop(&mut self) {
        unsafe { FreeSid(self.sid.as_ptr()) };
    }
}

pub fn create_profile(
    name: &str,
    display_name: &str,
    description: &str,
    capabilities: &[AppContainerCapability<'_>],
) -> Result<OwnedAppContainerSid, AppContainerProfileError> {
    const OPERATION: &str = "CreateAppContainerProfile";
    let name = wide_required(OPERATION, "name", name)?;
    let display_name = wide_required(OPERATION, "display name", display_name)?;
    let description = wide_required(OPERATION, "description", description)?;
    let mut native_capabilities = capabilities
        .iter()
        .map(|capability| SID_AND_ATTRIBUTES {
            Sid: capability.sid.as_ptr().cast_mut().cast(),
            Attributes: capability.attributes,
        })
        .collect::<Vec<_>>();
    let capability_count = u32::try_from(native_capabilities.len()).map_err(|_| {
        AppContainerProfileError::invalid(OPERATION, "capability count exceeds u32")
    })?;
    let mut sid = std::ptr::null_mut();
    let hresult = unsafe {
        CreateAppContainerProfile(
            name.as_ptr(),
            display_name.as_ptr(),
            description.as_ptr(),
            if native_capabilities.is_empty() {
                std::ptr::null()
            } else {
                native_capabilities.as_mut_ptr()
            },
            capability_count,
            &raw mut sid,
        )
    };
    if hresult < 0 {
        return Err(AppContainerProfileError::native(OPERATION, hresult));
    }
    OwnedAppContainerSid::from_raw(OPERATION, sid)
}

pub fn derive_profile_sid(name: &str) -> Result<OwnedAppContainerSid, AppContainerProfileError> {
    const OPERATION: &str = "DeriveAppContainerSidFromAppContainerName";
    let name = wide_required(OPERATION, "name", name)?;
    let mut sid = std::ptr::null_mut();
    let hresult = unsafe { DeriveAppContainerSidFromAppContainerName(name.as_ptr(), &raw mut sid) };
    if hresult < 0 {
        return Err(AppContainerProfileError::native(OPERATION, hresult));
    }
    OwnedAppContainerSid::from_raw(OPERATION, sid)
}

pub fn delete_profile(name: &str) -> Result<(), AppContainerProfileError> {
    const OPERATION: &str = "DeleteAppContainerProfile";
    let name = wide_required(OPERATION, "name", name)?;
    let hresult = unsafe { DeleteAppContainerProfile(name.as_ptr()) };
    if hresult < 0 {
        Err(AppContainerProfileError::native(OPERATION, hresult))
    } else {
        Ok(())
    }
}

fn wide_required(
    operation: &'static str,
    field: &str,
    value: &str,
) -> Result<Vec<u16>, AppContainerProfileError> {
    if value.is_empty() {
        return Err(AppContainerProfileError::invalid(
            operation,
            format!("{field} must not be empty"),
        ));
    }
    if value.encode_utf16().any(|unit| unit == 0) {
        return Err(AppContainerProfileError::invalid(
            operation,
            format!("{field} contains NUL"),
        ));
    }
    Ok(value.encode_utf16().chain(std::iter::once(0)).collect())
}

fn validate_sid_bytes(operation: &'static str, sid: &[u8]) -> Result<(), AppContainerProfileError> {
    const SID_HEADER_BYTES: usize = 8;
    const MAX_SUB_AUTHORITIES: usize = 15;
    if sid.len() < SID_HEADER_BYTES {
        return Err(AppContainerProfileError::invalid(
            operation,
            "SID bytes are invalid",
        ));
    }
    let sub_authorities = usize::from(sid[1]);
    let Some(expected_len) = sub_authorities
        .checked_mul(std::mem::size_of::<u32>())
        .and_then(|bytes| SID_HEADER_BYTES.checked_add(bytes))
    else {
        return Err(AppContainerProfileError::invalid(
            operation,
            "SID byte length overflows",
        ));
    };
    if sub_authorities > MAX_SUB_AUTHORITIES || expected_len != sid.len() {
        return Err(AppContainerProfileError::invalid(
            operation,
            "SID byte length is not exact",
        ));
    }
    if unsafe { IsValidSid(sid.as_ptr().cast_mut().cast()) } == 0 {
        return Err(AppContainerProfileError::invalid(
            operation,
            "SID bytes are invalid",
        ));
    }
    let native_len = unsafe { GetLengthSid(sid.as_ptr().cast_mut().cast()) } as usize;
    if native_len != sid.len() {
        return Err(AppContainerProfileError::invalid(
            operation,
            "SID byte length is not exact",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    struct ProfileCleanup(String);

    impl Drop for ProfileCleanup {
        fn drop(&mut self) {
            let _ = delete_profile(&self.0);
        }
    }

    fn unique_name(label: &str) -> String {
        format!(
            "agenterm.platform.{}.{}.{}",
            std::process::id(),
            label,
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system clock")
                .as_nanos()
        )
    }

    #[test]
    fn derived_sid_is_owned_deterministic_and_printable() {
        let name = unique_name("derive");
        let first = derive_profile_sid(&name).expect("derive first SID");
        let second = derive_profile_sid(&name).expect("derive second SID");
        assert_eq!(first.as_bytes(), second.as_bytes());
        assert!(first.string().expect("format SID").starts_with("S-1-15-2-"));
        assert_eq!(
            first.string().unwrap(),
            sid_string(first.as_bytes()).unwrap()
        );
    }

    #[test]
    fn profile_creation_reports_existing_and_delete_is_explicit() {
        let name = unique_name("lifecycle");
        let cleanup = ProfileCleanup(name.clone());
        let created = create_profile(&name, &name, "agenterm-platform test profile", &[])
            .expect("create profile");
        let existing = create_profile(&name, &name, "agenterm-platform test profile", &[])
            .expect_err("second create must report the existing registration");
        assert_eq!(existing.kind(), AppContainerProfileErrorKind::AlreadyExists);
        let derived = derive_profile_sid(&name).expect("derive registered profile SID");
        assert_eq!(created.as_bytes(), derived.as_bytes());
        delete_profile(&name).expect("delete profile");
        std::mem::forget(cleanup);
    }

    #[test]
    fn invalid_strings_and_sid_slices_fail_before_profile_mutation() {
        let error = derive_profile_sid("bad\0name").expect_err("NUL must be rejected");
        assert_eq!(error.kind(), AppContainerProfileErrorKind::InvalidInput);
        let error = AppContainerCapability::new(&[1, 2, 3], 0)
            .expect_err("invalid SID bytes must be rejected");
        assert_eq!(error.kind(), AppContainerProfileErrorKind::InvalidInput);
        assert_eq!(
            sid_string(&[1, 2, 3]).unwrap_err().kind(),
            AppContainerProfileErrorKind::InvalidInput
        );
    }
}
