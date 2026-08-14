#![cfg(windows)]

use std::ffi::c_void;
use std::ptr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use agenterm_platform::CapabilityStatus;
use agenterm_platform::accessibility_tree::{
    AccessibilityNodeAction, AccessibilityTreeError, capability_status, get_node_text,
    last_text_write_via, perform_node_action, set_node_text, tree_for_window,
};
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CREATESTRUCTW, CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetDlgItem,
    GetMessageW, MSG, PostMessageW, PostQuitMessage, RegisterClassW, SW_SHOWNOACTIVATE,
    SendMessageW, ShowWindow, TranslateMessage, UnregisterClassW, WM_APP, WM_CLOSE, WM_COMMAND,
    WM_DESTROY, WM_NCCREATE, WNDCLASSW, WS_CHILD, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
    WS_OVERLAPPEDWINDOW, WS_TABSTOP, WS_VISIBLE,
};

const BUTTON_ID: usize = 1001;
const EDIT_ID: usize = 1002;
const WM_REMOVE_BUTTON: u32 = WM_APP + 41;
static BUTTON_CLICKS: AtomicUsize = AtomicUsize::new(0);

struct Fixture {
    hwnd: isize,
    thread: Option<JoinHandle<()>>,
}

impl Fixture {
    fn start() -> Self {
        BUTTON_CLICKS.store(0, Ordering::SeqCst);
        let (sender, receiver) = mpsc::sync_channel(1);
        let thread = thread::spawn(move || unsafe {
            let class_name = wide("AgenTermPlatformUiaFixture");
            let module = GetModuleHandleW(ptr::null());
            let mut class: WNDCLASSW = std::mem::zeroed();
            class.lpfnWndProc = Some(fixture_wnd_proc);
            class.hInstance = module;
            class.lpszClassName = class_name.as_ptr();
            if RegisterClassW(&class) == 0 {
                let _ = sender.send(Err("RegisterClassW failed".to_owned()));
                return;
            }

            let title = wide("AgenTerm UIA fixture");
            let hwnd = CreateWindowExW(
                WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
                class_name.as_ptr(),
                title.as_ptr(),
                WS_OVERLAPPEDWINDOW | WS_VISIBLE,
                40,
                40,
                420,
                180,
                ptr::null_mut(),
                ptr::null_mut(),
                module,
                ptr::null(),
            );
            if hwnd.is_null() {
                let _ = sender.send(Err("CreateWindowExW fixture failed".to_owned()));
                UnregisterClassW(class_name.as_ptr(), module);
                return;
            }

            let edit_class = wide("EDIT");
            let seed = wide("seed");
            let edit = CreateWindowExW(
                0,
                edit_class.as_ptr(),
                seed.as_ptr(),
                WS_CHILD | WS_VISIBLE | WS_TABSTOP,
                16,
                20,
                240,
                28,
                hwnd,
                EDIT_ID as *mut c_void,
                module,
                ptr::null(),
            );
            let button_class = wide("BUTTON");
            let button_text = wide("Fixture Invoke");
            let button = CreateWindowExW(
                0,
                button_class.as_ptr(),
                button_text.as_ptr(),
                WS_CHILD | WS_VISIBLE | WS_TABSTOP,
                270,
                20,
                120,
                28,
                hwnd,
                BUTTON_ID as *mut c_void,
                module,
                ptr::null(),
            );
            if edit.is_null() || button.is_null() {
                let _ = sender.send(Err("fixture child creation failed".to_owned()));
                DestroyWindow(hwnd);
                UnregisterClassW(class_name.as_ptr(), module);
                return;
            }

            ShowWindow(hwnd, SW_SHOWNOACTIVATE);
            if sender.send(Ok(hwnd as isize)).is_err() {
                DestroyWindow(hwnd);
                UnregisterClassW(class_name.as_ptr(), module);
                return;
            }

            let mut message: MSG = std::mem::zeroed();
            while GetMessageW(&mut message, ptr::null_mut(), 0, 0) > 0 {
                TranslateMessage(&message);
                DispatchMessageW(&message);
            }
            UnregisterClassW(class_name.as_ptr(), module);
        });
        let hwnd = receiver
            .recv()
            .expect("fixture thread ended before publishing HWND")
            .expect("fixture creation failed");
        Self {
            hwnd,
            thread: Some(thread),
        }
    }

    fn remove_button(&self) {
        unsafe {
            SendMessageW(self.hwnd as HWND, WM_REMOVE_BUTTON, 0, 0);
        }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        unsafe {
            PostMessageW(self.hwnd as HWND, WM_CLOSE, 0, 0);
        }
        if let Some(thread) = self.thread.take() {
            thread.join().expect("fixture thread panicked");
        }
    }
}

unsafe extern "system" fn fixture_wnd_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match message {
        WM_NCCREATE => {
            let _create = lparam as *const CREATESTRUCTW;
            1
        }
        WM_COMMAND => {
            if wparam & 0xffff == BUTTON_ID {
                BUTTON_CLICKS.fetch_add(1, Ordering::SeqCst);
            }
            0
        }
        WM_REMOVE_BUTTON => {
            let button = unsafe { GetDlgItem(hwnd, BUTTON_ID as i32) };
            if !button.is_null() {
                unsafe { DestroyWindow(button) };
            }
            0
        }
        WM_DESTROY => {
            unsafe { PostQuitMessage(0) };
            0
        }
        _ => unsafe { DefWindowProcW(hwnd, message, wparam, lparam) },
    }
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

#[test]
fn native_fixture_exposes_tree_patterns_text_focus_and_recycling() {
    BUTTON_CLICKS.store(0, Ordering::SeqCst);
    assert_eq!(capability_status(), CapabilityStatus::Available);
    let fixture = Fixture::start();
    let tree = tree_for_window(Some(fixture.hwnd)).expect("UIA fixture tree");
    assert_eq!(tree.backend, "uia");
    assert_eq!(tree.window_handle, Some(fixture.hwnd));
    assert!(tree.nodes.len() >= 3, "tree: {tree:#?}");
    assert!(tree.nodes.len() <= 1_000);

    let edit = tree
        .nodes
        .iter()
        .find(|node| node.role == "edit" && node.actions.iter().any(|action| action == "set-text"))
        .expect("fixture edit node");
    let button = tree
        .nodes
        .iter()
        .find(|node| node.name == "Fixture Invoke")
        .expect("fixture button node");
    assert_eq!(button.parent_id.as_deref(), Some(tree.root_id.as_str()));
    assert!(button.actions.iter().any(|action| action == "click"));

    set_node_text(Some(fixture.hwnd), &edit.id, "UIA value round trip").expect("Value.SetValue");
    assert_eq!(last_text_write_via(), "value-pattern");
    assert_eq!(
        get_node_text(Some(fixture.hwnd), &edit.id).expect("Value.CurrentValue"),
        "UIA value round trip"
    );
    perform_node_action(Some(fixture.hwnd), &edit.id, AccessibilityNodeAction::Focus)
        .expect("UIA SetFocus");
    perform_node_action(
        Some(fixture.hwnd),
        &button.id,
        AccessibilityNodeAction::Click,
    )
    .expect("UIA Invoke");
    let invoke_deadline = Instant::now() + Duration::from_secs(1);
    while BUTTON_CLICKS.load(Ordering::SeqCst) == 0 && Instant::now() < invoke_deadline {
        thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(BUTTON_CLICKS.load(Ordering::SeqCst), 1);

    fixture.remove_button();
    let error = perform_node_action(
        Some(fixture.hwnd),
        &button.id,
        AccessibilityNodeAction::Click,
    )
    .expect_err("removed node must not resolve to a replacement");
    assert!(matches!(
        error,
        AccessibilityTreeError::Failed { code, .. } if code == "a11y_node_recycled"
    ));
}

#[test]
fn zero_hwnd_is_a_typed_disappeared_window() {
    assert!(matches!(
        tree_for_window(Some(0)),
        Err(AccessibilityTreeError::Failed { code, .. }) if code == "a11y_window_gone"
    ));
}
