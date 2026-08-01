use std::io;

use crate::process_security::{ProcessPrincipal, ProcessSecurityFacts};

pub(crate) fn current_process() -> io::Result<ProcessSecurityFacts> {
    process(std::process::id())
}

pub(crate) fn process(process_id: u32) -> io::Result<ProcessSecurityFacts> {
    let process_id = i32::try_from(process_id)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "process id is out of range"))?;
    let mut info = unsafe { std::mem::zeroed::<libc::proc_bsdinfo>() };
    let size = std::mem::size_of::<libc::proc_bsdinfo>() as libc::c_int;
    let read = unsafe {
        libc::proc_pidinfo(
            process_id,
            libc::PROC_PIDTBSDINFO,
            0,
            (&raw mut info).cast(),
            size,
        )
    };
    if read != size {
        return Err(io::Error::last_os_error());
    }
    Ok(ProcessSecurityFacts::new(
        ProcessPrincipal::Posix {
            effective_user_id: info.pbi_uid,
            effective_group_id: info.pbi_gid,
        },
        None,
    ))
}
