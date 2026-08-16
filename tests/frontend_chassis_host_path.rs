//! Both real host adapters continue from a checked image into PTY/IPC-backed L2.

const UNIX_FRONTEND: &str = include_str!("../src/platform/adapters/unix/frontend/mod.rs");
const WINDOWS_FRONTEND: &str = include_str!("../src/platform/adapters/windows/frontend.rs");

#[test]
fn unix_first_window_checks_image_then_starts_ipc_pty_and_l2() {
    let load = UNIX_FRONTEND
        .find("chassis_image::load_selected_image")
        .expect("validate composed image");
    let ipc = UNIX_FRONTEND[load..]
        .find("start_ipc_server(0")
        .map(|offset| load + offset)
        .expect("start real IPC server after image check");
    let pty = UNIX_FRONTEND[ipc..]
        .find("TerminalTab::spawn")
        .map(|offset| ipc + offset)
        .expect("spawn real PTY after IPC");
    let l2 = UNIX_FRONTEND[pty..]
        .find("chassis_image::eval_active_tab")
        .map(|offset| pty + offset)
        .expect("dispatch checked L2 after first PTY");

    assert!(load < ipc && ipc < pty && pty < l2);
    assert!(UNIX_FRONTEND.contains("capability != \"tabs.active\""));
    assert!(UNIX_FRONTEND.contains("live workbench has no active PTY tab"));
}

#[test]
fn windows_first_window_checks_image_then_uses_real_server_ipc_for_l2() {
    assert!(WINDOWS_FRONTEND.contains("chassis_image::load_selected_image"));
    assert!(WINDOWS_FRONTEND.contains("connect_or_start_frontend_gui_client"));
    assert!(WINDOWS_FRONTEND.contains("client.snapshot().active_tab_id.clone()"));
    assert!(WINDOWS_FRONTEND.contains("chassis_image::eval_active_tab"));
    assert!(WINDOWS_FRONTEND.contains("capability != \"tabs.active\""));
    assert!(WINDOWS_FRONTEND.contains("run_remote_gui(no_activate)"));
}

#[test]
fn both_hosts_reject_the_old_fat_spawn_and_return_fallback() {
    for source in [UNIX_FRONTEND, WINDOWS_FRONTEND] {
        assert!(!source.contains("run_selected_chassis_loader"));
        assert!(!source.contains("std::process::Command::new(loader)"));
        assert!(!source.contains("return GuiLaunchResult::Launched;"));
    }
}
