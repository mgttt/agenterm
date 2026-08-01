use std::{
    io,
    os::fd::{AsFd, AsRawFd, BorrowedFd, FromRawFd as _, OwnedFd},
};

pub struct ProcessReference {
    descriptor: OwnedFd,
    process_id: u32,
}

impl ProcessReference {
    pub(crate) fn open(process_id: u32) -> io::Result<Self> {
        let descriptor = unsafe { libc::syscall(libc::SYS_pidfd_open, process_id, 0) };
        if descriptor < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(Self {
            descriptor: unsafe { OwnedFd::from_raw_fd(descriptor as i32) },
            process_id,
        })
    }

    pub(crate) const fn id(&self) -> u32 {
        self.process_id
    }

    pub(crate) fn is_alive(&self) -> io::Result<bool> {
        let mut descriptor = libc::pollfd {
            fd: self.descriptor.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        };
        let ready = unsafe { libc::poll(&raw mut descriptor, 1, 0) };
        match ready {
            0 => Ok(true),
            1 if descriptor.revents & libc::POLLIN != 0 => Ok(false),
            1 if descriptor.revents & libc::POLLNVAL != 0 => Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "pidfd is invalid",
            )),
            1 => Ok(false),
            -1 => Err(io::Error::last_os_error()),
            value => Err(io::Error::other(format!(
                "unexpected pidfd poll result {value}"
            ))),
        }
    }
}

impl AsRawFd for crate::process_reference::ProcessReference {
    fn as_raw_fd(&self) -> std::os::fd::RawFd {
        self.0.descriptor.as_raw_fd()
    }
}

impl AsFd for crate::process_reference::ProcessReference {
    fn as_fd(&self) -> BorrowedFd<'_> {
        self.0.descriptor.as_fd()
    }
}
