//! Host-neutral thread launch boundary.
//!
//! Callers box product-specific work so `std` generates its thread startup and
//! panic-unwind trampoline once instead of once for every closure type.

use std::{io, thread};

pub type ThreadTask = Box<dyn FnOnce() + Send + 'static>;

#[inline(never)]
pub fn spawn_named(name: &'static str, task: ThreadTask) -> io::Result<thread::JoinHandle<()>> {
    thread::Builder::new()
        .name(name.to_owned())
        .spawn(move || run_task(task))
}

#[inline(never)]
fn run_task(task: ThreadTask) {
    task();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preserves_the_requested_thread_name() {
        let handle = spawn_named(
            "platform-thread-name-probe",
            Box::new(|| {
                assert_eq!(
                    std::thread::current().name(),
                    Some("platform-thread-name-probe")
                );
            }),
        )
        .expect("spawn named thread");
        handle.join().expect("named thread succeeds");
    }

    #[test]
    fn panic_remains_contained_by_the_join_handle() {
        let handle = spawn_named(
            "platform-thread-panic-probe",
            Box::new(|| panic!("thread panic probe")),
        )
        .expect("spawn panic probe");
        assert!(handle.join().is_err());
    }
}
