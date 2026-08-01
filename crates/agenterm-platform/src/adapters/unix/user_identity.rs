//! POSIX current real/effective user and group identity.

pub fn current_user_identity() -> std::io::Result<crate::user_identity::CurrentUserIdentity> {
    Ok(crate::user_identity::CurrentUserIdentity::Posix(
        crate::user_identity::PosixCredentials {
            real_user_id: unsafe { libc::getuid() },
            effective_user_id: unsafe { libc::geteuid() },
            real_group_id: unsafe { libc::getgid() },
            effective_group_id: unsafe { libc::getegid() },
        },
    ))
}
