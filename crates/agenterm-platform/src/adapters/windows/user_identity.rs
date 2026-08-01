//! Windows current process token user SID identity.

use std::{io, mem::size_of, ptr::null_mut};

use windows_sys::Win32::{
    Foundation::{CloseHandle, HANDLE},
    Security::{GetLengthSid, GetTokenInformation, TOKEN_QUERY, TOKEN_USER, TokenUser},
    System::Threading::{GetCurrentProcess, OpenProcessToken},
};

struct Token(HANDLE);

impl Drop for Token {
    fn drop(&mut self) {
        unsafe { CloseHandle(self.0) };
    }
}

pub fn current_user_identity() -> io::Result<crate::user_identity::CurrentUserIdentity> {
    let mut handle = null_mut();
    if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut handle) } == 0 {
        return Err(io::Error::last_os_error());
    }
    let token = Token(handle);
    let mut required = 0;
    unsafe { GetTokenInformation(token.0, TokenUser, null_mut(), 0, &mut required) };
    if required < size_of::<TOKEN_USER>() as u32 {
        return Err(io::Error::last_os_error());
    }
    let mut buffer = vec![0_usize; (required as usize).div_ceil(size_of::<usize>())];
    if unsafe {
        GetTokenInformation(
            token.0,
            TokenUser,
            buffer.as_mut_ptr().cast(),
            required,
            &mut required,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    let user = unsafe { &*buffer.as_ptr().cast::<TOKEN_USER>() };
    if user.User.Sid.is_null() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "process token returned a null user SID",
        ));
    }
    let sid_length = unsafe { GetLengthSid(user.User.Sid) } as usize;
    let buffer_start = buffer.as_ptr() as usize;
    let buffer_end = buffer_start
        .checked_add(required as usize)
        .ok_or_else(|| io::Error::from(io::ErrorKind::InvalidData))?;
    let sid_start = user.User.Sid as usize;
    let sid_end = sid_start
        .checked_add(sid_length)
        .ok_or_else(|| io::Error::from(io::ErrorKind::InvalidData))?;
    if sid_length == 0 || sid_start < buffer_start || sid_end > buffer_end {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "process token returned an invalid user SID",
        ));
    }
    let sid = unsafe { std::slice::from_raw_parts(user.User.Sid.cast::<u8>(), sid_length) };
    Ok(crate::user_identity::CurrentUserIdentity::WindowsSid(
        sid.to_vec(),
    ))
}
