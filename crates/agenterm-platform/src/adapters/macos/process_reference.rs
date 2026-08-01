use std::{
    io,
    os::fd::{AsFd, AsRawFd, BorrowedFd, FromRawFd as _, OwnedFd},
    sync::atomic::{AtomicBool, Ordering},
};

pub struct ProcessReference {
    queue: OwnedFd,
    process_id: u32,
    exited: AtomicBool,
}

impl ProcessReference {
    pub(crate) fn open(process_id: u32) -> io::Result<Self> {
        let queue = unsafe { libc::kqueue() };
        if queue < 0 {
            return Err(io::Error::last_os_error());
        }
        let queue = unsafe { OwnedFd::from_raw_fd(queue) };
        let change = libc::kevent {
            ident: process_id as libc::uintptr_t,
            filter: libc::EVFILT_PROC,
            flags: libc::EV_ADD | libc::EV_ENABLE | libc::EV_CLEAR,
            fflags: libc::NOTE_EXIT,
            data: 0,
            udata: std::ptr::null_mut(),
        };
        if unsafe {
            libc::kevent(
                queue.as_raw_fd(),
                &raw const change,
                1,
                std::ptr::null_mut(),
                0,
                std::ptr::null(),
            )
        } < 0
        {
            return Err(io::Error::last_os_error());
        }
        Ok(Self {
            queue,
            process_id,
            exited: AtomicBool::new(false),
        })
    }

    pub(crate) const fn id(&self) -> u32 {
        self.process_id
    }

    pub(crate) fn is_alive(&self) -> io::Result<bool> {
        if self.exited.load(Ordering::Acquire) {
            return Ok(false);
        }
        let mut event = unsafe { std::mem::zeroed::<libc::kevent>() };
        let timeout = libc::timespec {
            tv_sec: 0,
            tv_nsec: 0,
        };
        let ready = unsafe {
            libc::kevent(
                self.queue.as_raw_fd(),
                std::ptr::null(),
                0,
                &raw mut event,
                1,
                &raw const timeout,
            )
        };
        match ready {
            0 => Ok(true),
            1 if event.filter == libc::EVFILT_PROC && event.fflags & libc::NOTE_EXIT != 0 => {
                self.exited.store(true, Ordering::Release);
                Ok(false)
            }
            1 if event.flags & libc::EV_ERROR != 0 => {
                Err(io::Error::from_raw_os_error(event.data as i32))
            }
            1 => Err(io::Error::other("unexpected kqueue process event")),
            -1 => Err(io::Error::last_os_error()),
            value => Err(io::Error::other(format!(
                "unexpected kqueue result {value}"
            ))),
        }
    }
}

impl AsRawFd for crate::process_reference::ProcessReference {
    fn as_raw_fd(&self) -> std::os::fd::RawFd {
        self.0.queue.as_raw_fd()
    }
}

impl AsFd for crate::process_reference::ProcessReference {
    fn as_fd(&self) -> BorrowedFd<'_> {
        self.0.queue.as_fd()
    }
}
