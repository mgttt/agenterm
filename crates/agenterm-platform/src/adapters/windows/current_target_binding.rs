//! Windows proof of the current interactive logon and input desktop.

use std::{io, mem::size_of, ptr::null_mut};

use windows_sys::Win32::{
    Foundation::{CloseHandle, CompareObjectHandles, HANDLE, LocalFree},
    Security::{
        ACCESS_ALLOWED_ACE, ACE_HEADER, ACL_SIZE_INFORMATION, AclSizeInformation,
        Authorization::{GetNamedSecurityInfoW, SE_FILE_OBJECT},
        DACL_SECURITY_INFORMATION, EqualSid, GetAce, GetAclInformation, GetTokenInformation,
        TOKEN_QUERY, TOKEN_STATISTICS, TokenSessionId, TokenStatistics,
    },
    System::{
        RemoteDesktop::{
            WTS_CURRENT_SERVER_HANDLE, WTSActive, WTSConnectState, WTSFreeMemory, WTSINFOEXW,
            WTSQuerySessionInformationW, WTSSessionInfoEx,
        },
        StationsAndDesktops::{
            CloseDesktop, DESKTOP_READOBJECTS, GetProcessWindowStation, GetThreadDesktop,
            GetUserObjectInformationW, OpenInputDesktop, UOI_NAME,
        },
        SystemServices::ACCESS_ALLOWED_ACE_TYPE,
        Threading::{GetCurrentProcess, GetCurrentThreadId, OpenProcessToken},
    },
};

use crate::{
    CapabilityStatus,
    contract::current_target_binding::{CurrentTargetBindingError, CurrentTargetBindingErrorKind},
};

const MAX_USER_OBJECT_NAME_BYTES: u32 = 32 * 1024;

pub(crate) struct NativeCurrentSessionFacts(Vec<u8>);

impl NativeCurrentSessionFacts {
    pub(crate) fn as_bytes(&self) -> &[u8] {
        &self.0
    }
}

pub(crate) fn capability_status() -> CapabilityStatus {
    CapabilityStatus::Available
}

pub(crate) fn validate_private_key_file(
    path: &std::path::Path,
) -> Result<(), CurrentTargetBindingError> {
    use std::os::windows::ffi::OsStrExt as _;

    let wide = path
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect::<Vec<_>>();
    let mut dacl = null_mut();
    let mut descriptor = null_mut();
    let status = unsafe {
        GetNamedSecurityInfoW(
            wide.as_ptr(),
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION,
            null_mut(),
            null_mut(),
            &mut dacl,
            null_mut(),
            &mut descriptor,
        )
    };
    if status != 0 || dacl.is_null() || descriptor.is_null() {
        if !descriptor.is_null() {
            unsafe { LocalFree(descriptor) };
        }
        return Err(permission(
            "install-key-acl-unavailable",
            "installation key permissions could not be verified",
        ));
    }
    let descriptor = LocalAllocation(descriptor);
    let mut size = ACL_SIZE_INFORMATION {
        AceCount: 0,
        AclBytesInUse: 0,
        AclBytesFree: 0,
    };
    if unsafe {
        GetAclInformation(
            dacl,
            (&raw mut size).cast(),
            size_of::<ACL_SIZE_INFORMATION>() as u32,
            AclSizeInformation,
        )
    } == 0
        || size.AceCount == 0
    {
        return Err(permission(
            "install-key-acl-invalid",
            "installation key does not have a private access list",
        ));
    }
    let identity = crate::user_identity::current_user_identity().map_err(|_| {
        permission(
            "install-key-owner-unavailable",
            "installation key owner could not be verified",
        )
    })?;
    let sid = identity.windows_sid().ok_or_else(|| {
        permission(
            "install-key-owner-invalid",
            "installation key owner is not a Windows SID",
        )
    })?;
    let mut aligned_sid = vec![0_usize; sid.len().div_ceil(size_of::<usize>())];
    unsafe {
        std::ptr::copy_nonoverlapping(sid.as_ptr(), aligned_sid.as_mut_ptr().cast(), sid.len());
    }
    let sid_offset = size_of::<ACCESS_ALLOWED_ACE>() - size_of::<u32>();
    let private = (0..size.AceCount).all(|index| {
        let mut ace = null_mut();
        if unsafe { GetAce(dacl, index, &mut ace) } == 0 || ace.is_null() {
            return false;
        }
        let header = unsafe { ace.cast::<ACE_HEADER>().read_unaligned() };
        let ace_bytes = usize::from(header.AceSize);
        if u32::from(header.AceType) != ACCESS_ALLOWED_ACE_TYPE || ace_bytes < sid_offset + 8 {
            return false;
        }
        let sid_ptr = unsafe { ace.cast::<u8>().add(sid_offset) };
        let sub_authorities = usize::from(unsafe { sid_ptr.add(1).read() });
        let sid_bytes = 8_usize.saturating_add(sub_authorities.saturating_mul(4));
        if sid_offset.saturating_add(sid_bytes) > ace_bytes {
            return false;
        }
        (unsafe { EqualSid(sid_ptr.cast(), aligned_sid.as_mut_ptr().cast()) }) != 0
    });
    drop(descriptor);
    if !private {
        return Err(permission(
            "install-key-acl-not-private",
            "installation key grants access outside the current user",
        ));
    }
    Ok(())
}

pub(crate) fn current_session_facts() -> Result<NativeCurrentSessionFacts, CurrentTargetBindingError>
{
    let token = Token::current()?;
    let statistics: TOKEN_STATISTICS = token.information(TokenStatistics)?;
    let session_id: u32 = token.information(TokenSessionId)?;
    if session_id == 0 {
        return Err(unsupported(
            "session-zero",
            "session zero is not an interactive desktop session",
        ));
    }

    let wts = query_wts_session(session_id)?;
    if wts.state != WTSActive {
        return Err(unsupported(
            "session-not-active",
            "the current logon session is not WTS active",
        ));
    }

    let station = unsafe { GetProcessWindowStation() };
    if station.is_null() {
        return Err(unsupported(
            "window-station-unavailable",
            "the process has no window station",
        ));
    }
    let desktop = unsafe { GetThreadDesktop(GetCurrentThreadId()) };
    if desktop.is_null() {
        return Err(unsupported(
            "desktop-unavailable",
            "the current thread has no desktop",
        ));
    }
    let input = unsafe { OpenInputDesktop(0, 0, DESKTOP_READOBJECTS) };
    if input.is_null() {
        return Err(unsupported(
            "input-desktop-unavailable",
            "the input desktop could not be opened for comparison",
        ));
    }
    let input = Desktop(input);
    if unsafe { CompareObjectHandles(desktop, input.0) } == 0 {
        return Err(unsupported(
            "input-desktop-mismatch",
            "the current thread is not attached to the input desktop",
        ));
    }

    let sid = crate::user_identity::current_user_identity()
        .map_err(|_| native("token-sid", "the current token SID could not be read"))?
        .stable_bytes();
    let station_name = user_object_name(station)?;
    let desktop_name = user_object_name(desktop)?;
    let mut facts = Vec::with_capacity(
        64 + sid.len() + station_name.len() * size_of::<u16>() + desktop_name.len() * 2,
    );
    push_bytes(&mut facts, 1, &sid);
    push_bytes(
        &mut facts,
        2,
        &statistics.AuthenticationId.LowPart.to_le_bytes(),
    );
    push_bytes(
        &mut facts,
        3,
        &statistics.AuthenticationId.HighPart.to_le_bytes(),
    );
    push_bytes(&mut facts, 4, &session_id.to_le_bytes());
    push_u16(&mut facts, 5, &station_name);
    push_u16(&mut facts, 6, &desktop_name);
    push_bytes(&mut facts, 7, &wts.logon_time.to_le_bytes());
    Ok(NativeCurrentSessionFacts(facts))
}

struct Token(HANDLE);

impl Token {
    fn current() -> Result<Self, CurrentTargetBindingError> {
        let mut handle = null_mut();
        if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut handle) } == 0 {
            return Err(native(
                "process-token",
                "the current process token could not be opened",
            ));
        }
        Ok(Self(handle))
    }

    fn information<T: Copy + Default>(&self, class: i32) -> Result<T, CurrentTargetBindingError> {
        let mut value = T::default();
        let mut returned = 0;
        if unsafe {
            GetTokenInformation(
                self.0,
                class,
                (&raw mut value).cast(),
                size_of::<T>() as u32,
                &mut returned,
            )
        } == 0
            || returned != size_of::<T>() as u32
        {
            return Err(native(
                "token-information",
                "required token information is unavailable",
            ));
        }
        Ok(value)
    }
}

impl Drop for Token {
    fn drop(&mut self) {
        unsafe { CloseHandle(self.0) };
    }
}

struct Desktop(HANDLE);

impl Drop for Desktop {
    fn drop(&mut self) {
        unsafe { CloseDesktop(self.0) };
    }
}

struct WtsBuffer(*mut u16);

impl Drop for WtsBuffer {
    fn drop(&mut self) {
        unsafe { WTSFreeMemory(self.0.cast()) };
    }
}

struct LocalAllocation(*mut core::ffi::c_void);

impl Drop for LocalAllocation {
    fn drop(&mut self) {
        unsafe { LocalFree(self.0) };
    }
}

struct WtsSession {
    state: i32,
    logon_time: i64,
}

fn query_wts_session(session_id: u32) -> Result<WtsSession, CurrentTargetBindingError> {
    let mut state_ptr = null_mut();
    let mut state_bytes = 0;
    if unsafe {
        WTSQuerySessionInformationW(
            WTS_CURRENT_SERVER_HANDLE,
            session_id,
            WTSConnectState,
            &mut state_ptr,
            &mut state_bytes,
        )
    } == 0
        || state_ptr.is_null()
        || state_bytes != size_of::<i32>() as u32
    {
        return Err(unsupported(
            "wts-state-unavailable",
            "WTS connection state could not be proven",
        ));
    }
    let state_buffer = WtsBuffer(state_ptr);
    let state = unsafe { state_buffer.0.cast::<i32>().read_unaligned() };

    let mut info_ptr = null_mut();
    let mut info_bytes = 0;
    if unsafe {
        WTSQuerySessionInformationW(
            WTS_CURRENT_SERVER_HANDLE,
            session_id,
            WTSSessionInfoEx,
            &mut info_ptr,
            &mut info_bytes,
        )
    } == 0
        || info_ptr.is_null()
        || info_bytes < size_of::<WTSINFOEXW>() as u32
    {
        return Err(unsupported(
            "wts-logon-unavailable",
            "WTS logon identity could not be proven",
        ));
    }
    let info_buffer = WtsBuffer(info_ptr);
    let info = unsafe { info_buffer.0.cast::<WTSINFOEXW>().read_unaligned() };
    if info.Level != 1 {
        return Err(unsupported(
            "wts-logon-level",
            "WTS returned an unsupported session information level",
        ));
    }
    let level = unsafe { info.Data.WTSInfoExLevel1 };
    if level.SessionId != session_id || level.SessionState != state || level.LogonTime <= 0 {
        return Err(unsupported(
            "wts-logon-inconsistent",
            "WTS session identity was incomplete or inconsistent",
        ));
    }
    Ok(WtsSession {
        state,
        logon_time: level.LogonTime,
    })
}

fn user_object_name(handle: HANDLE) -> Result<Vec<u16>, CurrentTargetBindingError> {
    let mut required = 0;
    unsafe { GetUserObjectInformationW(handle, UOI_NAME, null_mut(), 0, &mut required) };
    if required < size_of::<u16>() as u32
        || required > MAX_USER_OBJECT_NAME_BYTES
        || required % size_of::<u16>() as u32 != 0
    {
        return Err(unsupported(
            "desktop-name-unavailable",
            "window station or desktop identity is unavailable",
        ));
    }
    let capacity_bytes = required;
    let mut buffer = vec![0_u16; required as usize / size_of::<u16>()];
    if unsafe {
        GetUserObjectInformationW(
            handle,
            UOI_NAME,
            buffer.as_mut_ptr().cast(),
            required,
            &mut required,
        )
    } == 0
        || required < size_of::<u16>() as u32
        || required > capacity_bytes
        || required % size_of::<u16>() as u32 != 0
    {
        return Err(unsupported(
            "desktop-name-unavailable",
            "window station or desktop identity is unavailable",
        ));
    }
    let returned_units = required as usize / size_of::<u16>();
    let returned = &buffer[..returned_units];
    if returned.last() != Some(&0) {
        return Err(native(
            "desktop-name-invalid",
            "window station or desktop identity was malformed",
        ));
    }
    let name = &returned[..returned_units - 1];
    if name.is_empty() {
        return Err(unsupported(
            "desktop-name-empty",
            "window station or desktop identity was empty",
        ));
    }
    if name.contains(&0) {
        return Err(native(
            "desktop-name-invalid",
            "window station or desktop identity was malformed",
        ));
    }
    buffer.truncate(returned_units - 1);
    Ok(buffer)
}

fn push_bytes(output: &mut Vec<u8>, tag: u8, value: &[u8]) {
    output.push(tag);
    output.extend_from_slice(&(value.len() as u32).to_le_bytes());
    output.extend_from_slice(value);
}

fn push_u16(output: &mut Vec<u8>, tag: u8, value: &[u16]) {
    output.push(tag);
    output.extend_from_slice(&(value.len() as u32).to_le_bytes());
    output.extend(value.iter().flat_map(|unit| unit.to_le_bytes()));
}

fn unsupported(code: &'static str, message: &'static str) -> CurrentTargetBindingError {
    CurrentTargetBindingError::new(CurrentTargetBindingErrorKind::Unsupported, code, message)
}

fn native(code: &'static str, message: &'static str) -> CurrentTargetBindingError {
    let _ = io::Error::last_os_error();
    CurrentTargetBindingError::new(CurrentTargetBindingErrorKind::Native, code, message)
}

fn permission(code: &'static str, message: &'static str) -> CurrentTargetBindingError {
    CurrentTargetBindingError::new(CurrentTargetBindingErrorKind::Permission, code, message)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn current_session_is_proven_or_fails_closed() {
        match current_session_facts() {
            Ok(facts) => assert!(!facts.as_bytes().is_empty()),
            Err(error) => assert!(matches!(
                error.kind(),
                CurrentTargetBindingErrorKind::Unsupported | CurrentTargetBindingErrorKind::Native
            )),
        }
    }
}
