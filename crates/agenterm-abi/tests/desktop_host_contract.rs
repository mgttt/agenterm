use std::mem::{align_of, size_of};

use agenterm::{
    agt_desktop_action, agt_desktop_host_close, agt_desktop_host_open, agt_desktop_host_poll,
    agt_desktop_host_t, agt_status,
};

#[test]
fn desktop_action_layout_is_stable_for_pointer_width() {
    let pointer = size_of::<usize>();
    assert_eq!(align_of::<agt_desktop_action>(), pointer);
    assert_eq!(
        size_of::<agt_desktop_action>(),
        if pointer == 8 { 40 } else { 20 }
    );
}

#[test]
fn zero_action_id_fails_before_platform_dispatch() {
    let label = b"Quit";
    let action = agt_desktop_action {
        action_id: 0,
        label: label.as_ptr(),
        label_len: label.len(),
        shortcut: std::ptr::null(),
        shortcut_len: 0,
    };
    let mut host: agt_desktop_host_t = std::ptr::null_mut();
    assert_eq!(
        agt_desktop_host_open(&action, 1, &mut host),
        agt_status::AGT_FAILED
    );
    assert!(host.is_null());
}

#[test]
fn open_poll_timeout_and_close_follow_host_contract() {
    let label = b"Quit";
    let action = agt_desktop_action {
        action_id: 7,
        label: label.as_ptr(),
        label_len: label.len(),
        shortcut: std::ptr::null(),
        shortcut_len: 0,
    };
    let mut host: agt_desktop_host_t = std::ptr::null_mut();
    let opened = agt_desktop_host_open(&action, 1, &mut host);
    if cfg!(windows) {
        assert_eq!(opened, agt_status::AGT_OK);
        assert!(!host.is_null());
        let mut action_id = u32::MAX;
        assert_eq!(
            agt_desktop_host_poll(host, 0, &mut action_id),
            agt_status::AGT_OK
        );
        assert_eq!(action_id, 0);
        assert_eq!(agt_desktop_host_close(host), agt_status::AGT_OK);
    } else {
        assert_eq!(opened, agt_status::AGT_UNSUPPORTED);
        assert!(host.is_null());
    }
}
