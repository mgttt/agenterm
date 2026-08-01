//! Current host user identity facts without authorization or product policy.

use std::io;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct PosixCredentials {
    pub real_user_id: u32,
    pub effective_user_id: u32,
    pub real_group_id: u32,
    pub effective_group_id: u32,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
#[non_exhaustive]
pub enum CurrentUserIdentity {
    Posix(PosixCredentials),
    WindowsSid(Vec<u8>),
}

impl CurrentUserIdentity {
    #[must_use]
    pub const fn stable_kind(&self) -> &'static str {
        match self {
            Self::Posix(_) => "uid",
            Self::WindowsSid(_) => "sid",
        }
    }

    #[must_use]
    pub fn stable_bytes(&self) -> Vec<u8> {
        match self {
            Self::Posix(credentials) => credentials.effective_user_id.to_le_bytes().to_vec(),
            Self::WindowsSid(sid) => sid.clone(),
        }
    }

    #[must_use]
    pub const fn posix_credentials(&self) -> Option<PosixCredentials> {
        match self {
            Self::Posix(credentials) => Some(*credentials),
            Self::WindowsSid(_) => None,
        }
    }

    #[must_use]
    pub fn windows_sid(&self) -> Option<&[u8]> {
        match self {
            Self::Posix(_) => None,
            Self::WindowsSid(sid) => Some(sid),
        }
    }
}

pub fn current_user_identity() -> io::Result<CurrentUserIdentity> {
    crate::selected::user_identity::current_user_identity()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn current_identity_has_stable_nonempty_bytes() {
        let identity = current_user_identity().expect("query current user identity");
        assert!(!identity.stable_kind().is_empty());
        assert!(!identity.stable_bytes().is_empty());
        #[cfg(windows)]
        {
            use windows_sys::Win32::Security::{GetLengthSid, IsValidSid};
            let sid = identity.windows_sid().expect("Windows SID identity");
            assert_ne!(unsafe { IsValidSid(sid.as_ptr().cast_mut().cast()) }, 0);
            assert_eq!(
                unsafe { GetLengthSid(sid.as_ptr().cast_mut().cast()) } as usize,
                sid.len()
            );
        }
        #[cfg(unix)]
        assert!(identity.posix_credentials().is_some());
    }
}
