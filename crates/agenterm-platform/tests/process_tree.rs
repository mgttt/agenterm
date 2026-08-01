#![cfg(all(feature = "process", unix))]

use std::{
    path::Path,
    process::{Child, Command},
    time::{Duration, Instant},
};

use agenterm_platform::{
    contract::process::ProcessObservation,
    process::{self, ProcessTreeGuard},
};

const MODE: &str = "AGENTERM_PLATFORM_PROCESS_TREE_MODE";
const MARKER: &str = "AGENTERM_PLATFORM_PROCESS_TREE_MARKER";

#[test]
fn owned_tree_terminates_descendants_that_create_new_process_groups() {
    let marker = std::env::var_os(MARKER);
    match std::env::var(MODE).ok().as_deref() {
        Some("grandchild") => {
            std::fs::write(
                Path::new(&marker.expect("grandchild marker path")),
                std::process::id().to_string(),
            )
            .expect("publish grandchild PID");
            std::thread::sleep(Duration::from_secs(30));
            return;
        }
        Some("child") => {
            let mut command = test_command("grandchild", marker.as_deref());
            process::configure_owned_command(&mut command).expect("configure grandchild group");
            let mut grandchild = command.spawn().expect("spawn grouped grandchild");
            let _ = grandchild.wait();
            return;
        }
        Some(other) => panic!("unknown process-tree helper mode: {other}"),
        None => {}
    }

    let marker = std::env::temp_dir().join(format!(
        "agenterm-platform-process-tree-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&marker);
    let mut command = test_command("child", Some(marker.as_os_str()));
    process::configure_owned_command(&mut command).expect("configure child group");
    let mut child = command.spawn().expect("spawn owned child");
    let mut guard = GuardCleanup(Some(
        ProcessTreeGuard::attach(&child).expect("attach process-tree guard"),
    ));

    let grandchild_id = wait_for_marker(&marker, &mut child);
    let grandchild_identity =
        process::start_identity(grandchild_id).expect("capture grouped grandchild start identity");
    guard
        .0
        .as_mut()
        .expect("active guard")
        .terminate()
        .expect("terminate complete owned tree");
    wait_for_child_exit(&mut child);
    wait_for_original_exit(grandchild_id, &grandchild_identity);
    guard.0.take();
    std::fs::remove_file(marker).expect("remove process-tree marker");
}

struct GuardCleanup(Option<ProcessTreeGuard>);

impl Drop for GuardCleanup {
    fn drop(&mut self) {
        if let Some(guard) = self.0.as_mut() {
            let _ = guard.terminate();
        }
    }
}

fn test_command(mode: &str, marker: Option<&std::ffi::OsStr>) -> Command {
    let mut command = Command::new(std::env::current_exe().expect("integration test executable"));
    command
        .args([
            "--exact",
            "owned_tree_terminates_descendants_that_create_new_process_groups",
        ])
        .env(MODE, mode);
    if let Some(marker) = marker {
        command.env(MARKER, marker);
    }
    command
}

fn wait_for_marker(marker: &Path, child: &mut Child) -> u32 {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Ok(value) = std::fs::read_to_string(marker) {
            return value.trim().parse().expect("grandchild PID marker");
        }
        if let Some(status) = child.try_wait().expect("observe child helper") {
            panic!("child helper exited before publishing grandchild: {status}");
        }
        assert!(Instant::now() < deadline, "grandchild marker timed out");
        std::thread::yield_now();
    }
}

fn wait_for_child_exit(child: &mut Child) {
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        if child
            .try_wait()
            .expect("observe terminated child")
            .is_some()
        {
            return;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!("owned child survived process-tree termination");
        }
        std::thread::yield_now();
    }
}

fn wait_for_original_exit(id: u32, identity: &str) {
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        let observation = process::observe(id);
        match &observation {
            ProcessObservation::Dead { .. } => return,
            ProcessObservation::Live {
                start_identity: Some(current),
            } if current != identity => return,
            _ if Instant::now() < deadline => std::thread::yield_now(),
            _ => panic!(
                "grouped grandchild {id} with identity {identity:?} survived: {observation:?}"
            ),
        }
    }
}
