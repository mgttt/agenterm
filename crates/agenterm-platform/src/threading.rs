//! Host-neutral detached-thread launch contract.

use std::io;

pub type ThreadTask = Box<dyn FnOnce() + Send + 'static>;

#[inline(never)]
pub fn spawn_named_detached(name: &'static str, task: ThreadTask) -> io::Result<()> {
    crate::selected::threading::spawn_named_detached(name, task)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        sync::mpsc,
        time::{Duration, Instant},
    };

    #[test]
    fn detached_task_runs_with_the_requested_os_name() {
        let (send, receive) = mpsc::channel();
        spawn_named_detached(
            "platform-thread-name-probe",
            Box::new(move || {
                let _ = send.send(crate::selected::threading::current_name());
            }),
        )
        .expect("spawn named thread");
        assert_eq!(
            receive.recv_timeout(Duration::from_secs(2)).unwrap(),
            Some("platform-thread-name-probe".to_owned())
        );
    }

    #[test]
    fn panic_is_contained_and_unwinds_the_detached_task() {
        struct Completion(mpsc::Sender<()>);
        impl Drop for Completion {
            fn drop(&mut self) {
                let _ = self.0.send(());
            }
        }

        let (send, receive) = mpsc::channel();
        let started = Instant::now();
        spawn_named_detached(
            "platform-thread-panic-probe",
            Box::new(move || {
                let _completion = Completion(send);
                panic!("thread panic probe");
            }),
        )
        .expect("spawn panic probe");
        receive
            .recv_timeout(Duration::from_secs(2))
            .expect("panic unwound the task");
        assert!(started.elapsed() < Duration::from_secs(2));
    }
}
