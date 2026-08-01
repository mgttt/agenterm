use std::{
    io,
    mem::size_of,
    os::windows::io::{AsRawHandle as _, BorrowedHandle, FromRawHandle as _, OwnedHandle},
    ptr,
};

use windows_sys::Win32::{
    Foundation::HANDLE,
    Security::{
        GetLengthSid, GetTokenInformation, IsValidSid, TOKEN_APPCONTAINER_INFORMATION,
        TOKEN_INFORMATION_CLASS, TOKEN_QUERY, TOKEN_USER, TokenAppContainerSid, TokenUser,
    },
    System::Threading::{
        GetCurrentProcess, OpenProcess, OpenProcessToken, PROCESS_QUERY_LIMITED_INFORMATION,
    },
};

use crate::process_security::{
    ProcessPrincipal, ProcessSandboxIdentity, ProcessSecurityFacts, ProcessSecurityHandle,
};

pub(crate) fn current_process() -> io::Result<ProcessSecurityFacts> {
    token_facts(unsafe { GetCurrentProcess() })
}

pub(crate) fn process(process_id: u32) -> io::Result<ProcessSecurityFacts> {
    let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, process_id) };
    if handle.is_null() {
        return Err(io::Error::last_os_error());
    }
    let handle = unsafe { OwnedHandle::from_raw_handle(handle) };
    token_facts(handle.as_raw_handle())
}

impl ProcessSecurityHandle for BorrowedHandle<'_> {
    fn process_security_facts(self) -> io::Result<ProcessSecurityFacts> {
        token_facts(self.as_raw_handle())
    }
}

fn token_facts(process: HANDLE) -> io::Result<ProcessSecurityFacts> {
    let mut token = ptr::null_mut();
    if unsafe { OpenProcessToken(process, TOKEN_QUERY, &mut token) } == 0 {
        return Err(io::Error::last_os_error());
    }
    let token = unsafe { OwnedHandle::from_raw_handle(token) };
    let token = token.as_raw_handle();

    let user_buffer = token_information(token, TokenUser, size_of::<TOKEN_USER>())?;
    let user = unsafe { &*user_buffer.as_ptr().cast::<TOKEN_USER>() };
    let user_sid = copy_sid(user.User.Sid, &user_buffer)?;

    let app_container_buffer = token_information(
        token,
        TokenAppContainerSid,
        size_of::<TOKEN_APPCONTAINER_INFORMATION>(),
    )?;
    let app_container = unsafe {
        &*app_container_buffer
            .as_ptr()
            .cast::<TOKEN_APPCONTAINER_INFORMATION>()
    };
    let sandbox = if app_container.TokenAppContainer.is_null() {
        None
    } else {
        Some(ProcessSandboxIdentity::WindowsAppContainerSid(copy_sid(
            app_container.TokenAppContainer,
            &app_container_buffer,
        )?))
    };

    Ok(ProcessSecurityFacts::new(
        ProcessPrincipal::WindowsSid(user_sid),
        sandbox,
    ))
}

fn token_information(
    token: HANDLE,
    class: TOKEN_INFORMATION_CLASS,
    minimum_size: usize,
) -> io::Result<Vec<usize>> {
    let mut required = 0;
    unsafe { GetTokenInformation(token, class, ptr::null_mut(), 0, &mut required) };
    if required < minimum_size as u32 {
        return Err(io::Error::last_os_error());
    }
    let mut buffer = vec![0_usize; (required as usize).div_ceil(size_of::<usize>())];
    if unsafe {
        GetTokenInformation(
            token,
            class,
            buffer.as_mut_ptr().cast(),
            required,
            &mut required,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    Ok(buffer)
}

fn copy_sid(sid: *mut core::ffi::c_void, buffer: &[usize]) -> io::Result<Vec<u8>> {
    if sid.is_null() || unsafe { IsValidSid(sid) } == 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "process token returned an invalid SID",
        ));
    }
    let sid_length = unsafe { GetLengthSid(sid) } as usize;
    let buffer_start = buffer.as_ptr() as usize;
    let buffer_end = buffer_start
        .checked_add(std::mem::size_of_val(buffer))
        .ok_or_else(|| io::Error::from(io::ErrorKind::InvalidData))?;
    let sid_start = sid as usize;
    let sid_end = sid_start
        .checked_add(sid_length)
        .ok_or_else(|| io::Error::from(io::ErrorKind::InvalidData))?;
    if sid_length == 0 || sid_start < buffer_start || sid_end > buffer_end {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "process token SID points outside its query buffer",
        ));
    }
    Ok(unsafe { std::slice::from_raw_parts(sid.cast::<u8>(), sid_length) }.to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::windows::io::RawHandle;

    #[test]
    fn current_process_handle_reports_a_valid_user_sid() {
        let handle = unsafe { BorrowedHandle::borrow_raw(GetCurrentProcess() as RawHandle) };
        let facts = crate::process_security::process_handle(handle)
            .expect("current process handle security");
        let ProcessPrincipal::WindowsSid(sid) = facts.principal() else {
            panic!("Windows process must report a SID principal");
        };
        assert_ne!(unsafe { IsValidSid(sid.as_ptr().cast_mut().cast()) }, 0);
    }
}
