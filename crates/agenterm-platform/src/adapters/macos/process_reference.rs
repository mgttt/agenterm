use std::{
    io,
    os::fd::{AsFd, AsRawFd, BorrowedFd, FromRawFd as _, OwnedFd},
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};

use crate::process_reference::ProcessWait;

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

    pub(crate) fn wait_for_exit(&self, timeout: Option<Duration>) -> io::Result<ProcessWait> {
        if self.exited.load(Ordering::Acquire) {
            return Ok(ProcessWait::Exited);
        }
        let started = Instant::now();
        loop {
            let mut event = unsafe { std::mem::zeroed::<libc::kevent>() };
            let remaining = timeout.map(|limit| limit.saturating_sub(started.elapsed()));
            let native_timeout = remaining.map(|duration| libc::timespec {
                tv_sec: duration.as_secs().min(i64::MAX as u64) as libc::time_t,
                tv_nsec: duration.subsec_nanos().into(),
            });
            let ready = unsafe {
                libc::kevent(
                    self.queue.as_raw_fd(),
                    std::ptr::null(),
                    0,
                    &raw mut event,
                    1,
                    native_timeout
                        .as_ref()
                        .map_or(std::ptr::null(), std::ptr::from_ref),
                )
            };
            match ready {
                0 if timeout.is_some_and(|limit| started.elapsed() >= limit) => {
                    return Ok(ProcessWait::TimedOut);
                }
                0 => {}
                1 if event.flags & libc::EV_ERROR != 0 => {
                    return Err(io::Error::from_raw_os_error(event.data as i32));
                }
                1 if event.filter == libc::EVFILT_PROC && event.fflags & libc::NOTE_EXIT != 0 => {
                    self.exited.store(true, Ordering::Release);
                    return Ok(ProcessWait::Exited);
                }
                1 => return Err(io::Error::other("unexpected kqueue process event")),
                -1 => {
                    let error = io::Error::last_os_error();
                    if error.kind() != io::ErrorKind::Interrupted {
                        return Err(error);
                    }
                }
                value => {
                    return Err(io::Error::other(format!(
                        "unexpected kqueue result {value}"
                    )));
                }
            }
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
