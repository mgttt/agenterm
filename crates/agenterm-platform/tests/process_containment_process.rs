#![cfg(all(feature = "process-containment", windows))]

use std::{os::windows::io::AsHandle as _, process::Command, thread, time::Duration};

use agenterm_platform::{
    process_containment::{ProcessContainment, ProcessContainmentOptions},
    process_reference::ProcessReference,
};

const CHILD_NAME: &str = "AGENTERM_PLATFORM_CONTAINMENT_NAME";

#[test]
fn named_containment_is_reopenable_across_processes() {
    if let Ok(name) = std::env::var(CHILD_NAME) {
        thread::sleep(Duration::from_millis(250));
        let containment = ProcessContainment::open(&name).expect("child opens named containment");
        let process = ProcessReference::open(std::process::id()).expect("child retains itself");
        assert!(
            containment
                .contains(&process)
                .expect("child queries exact membership")
        );
        assert!(
            containment
                .process_ids()
                .expect("child queries containment members")
                .contains(&std::process::id())
        );
        return;
    }

    let name = format!(
        r"Local\agenterm-platform-cross-process-containment-{}",
        std::process::id()
    );
    let containment = ProcessContainment::create(
        Some(&name),
        ProcessContainmentOptions {
            terminate_on_last_close: true,
            ..ProcessContainmentOptions::default()
        },
    )
    .expect("parent creates named containment");
    let mut child = Command::new(std::env::current_exe().expect("test executable"))
        .args([
            "--exact",
            "named_containment_is_reopenable_across_processes",
            "--nocapture",
        ])
        .env(CHILD_NAME, &name)
        .spawn()
        .expect("spawn containment child");
    let process =
        ProcessReference::duplicate_from(child.as_handle()).expect("retain containment child");
    containment
        .assign(&process)
        .expect("assign containment child");
    assert!(child.wait().expect("wait containment child").success());
}
