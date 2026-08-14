use std::collections::HashSet;
use std::mem::{size_of, zeroed};
use std::ptr::{null, null_mut};
use std::sync::atomic::{AtomicU32, Ordering};
use std::thread::{self, ThreadId};
use std::time::Duration;

use windows_sys::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    MOD_ALT, MOD_CONTROL, MOD_NOREPEAT, MOD_SHIFT, MOD_WIN, RegisterHotKey, UnregisterHotKey,
    VK_BACK, VK_DELETE, VK_DOWN, VK_END, VK_ESCAPE, VK_F1, VK_HOME, VK_INSERT, VK_LEFT, VK_NEXT,
    VK_PRIOR, VK_RETURN, VK_RIGHT, VK_SPACE, VK_TAB, VK_UP,
};
use windows_sys::Win32::UI::Shell::{
    NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NOTIFYICONDATAW, Shell_NotifyIconW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyMenu, DestroyWindow,
    DispatchMessageW, GetCursorPos, GetMessageW, IDI_APPLICATION, KillTimer, LoadIconW, MF_STRING,
    MSG, PM_REMOVE, PeekMessageW, PostMessageW, RegisterClassExW, RegisterWindowMessageW,
    SetForegroundWindow, SetTimer, TPM_BOTTOMALIGN, TPM_LEFTALIGN, TPM_RETURNCMD, TPM_RIGHTBUTTON,
    TrackPopupMenuEx, TranslateMessage, UnregisterClassW, WM_APP, WM_CONTEXTMENU, WM_HOTKEY,
    WM_LBUTTONUP, WM_NULL, WM_RBUTTONUP, WM_TIMER, WNDCLASSEXW,
};

use crate::contract::desktop_host::{DesktopActionSpec, DesktopHostError};

const TRAY_ID: u32 = 1;
const TIMER_ID: usize = 1;
const WM_TRAY: u32 = WM_APP + 0x41;
const WM_SHOW_MENU: u32 = WM_APP + 0x42;
static CLASS_SEQUENCE: AtomicU32 = AtomicU32::new(1);

struct NativeAction {
    action_id: u32,
    menu_text: Vec<u16>,
    hotkey: Option<(u32, u32)>,
}

pub(crate) struct DesktopHost {
    owner: ThreadId,
    instance: HINSTANCE,
    hwnd: HWND,
    class_name: Vec<u16>,
    actions: Vec<NativeAction>,
    registered_hotkeys: Vec<i32>,
    taskbar_created_message: u32,
    icon_added: bool,
    closed: bool,
}

impl DesktopHost {
    pub(crate) fn open(actions: Vec<DesktopActionSpec>) -> Result<Self, DesktopHostError> {
        let native_actions = prepare_actions(actions)?;
        let instance = unsafe { GetModuleHandleW(null()) };
        if instance.is_null() {
            return Err(last_error("desktop_host_get_module"));
        }
        let sequence = CLASS_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let taskbar_created_name = wide("TaskbarCreated");
        let taskbar_created_message =
            unsafe { RegisterWindowMessageW(taskbar_created_name.as_ptr()) };
        if taskbar_created_message == 0 {
            return Err(last_error("desktop_host_register_taskbar_created"));
        }
        let class_name = wide(&format!(
            "AgenTermDesktopHost.{}.{}.{}",
            std::process::id(),
            format_args!("{:?}", thread::current().id()),
            sequence
        ));
        let class = WNDCLASSEXW {
            cbSize: size_of::<WNDCLASSEXW>() as u32,
            style: 0,
            lpfnWndProc: Some(window_proc),
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: instance,
            hIcon: null_mut(),
            hCursor: null_mut(),
            hbrBackground: null_mut(),
            lpszMenuName: null(),
            lpszClassName: class_name.as_ptr(),
            hIconSm: null_mut(),
        };
        if unsafe { RegisterClassExW(&class) } == 0 {
            return Err(last_error("desktop_host_register_class"));
        }
        let hwnd = unsafe {
            CreateWindowExW(
                0,
                class_name.as_ptr(),
                class_name.as_ptr(),
                0,
                0,
                0,
                0,
                0,
                null_mut(),
                null_mut(),
                instance,
                null(),
            )
        };
        if hwnd.is_null() {
            unsafe { UnregisterClassW(class_name.as_ptr(), instance) };
            return Err(last_error("desktop_host_create_window"));
        }
        let mut host = Self {
            owner: thread::current().id(),
            instance,
            hwnd,
            class_name,
            actions: native_actions,
            registered_hotkeys: Vec::new(),
            taskbar_created_message,
            icon_added: false,
            closed: false,
        };
        if let Err(error) = host.initialize_native_resources() {
            host.cleanup_native();
            return Err(error);
        }
        Ok(host)
    }

    fn initialize_native_resources(&mut self) -> Result<(), DesktopHostError> {
        for (index, action) in self.actions.iter().enumerate() {
            if let Some((modifiers, key)) = action.hotkey {
                let id = i32::try_from(index + 1).expect("bounded action count fits i32");
                if unsafe { RegisterHotKey(self.hwnd, id, modifiers, key) } == 0 {
                    return Err(DesktopHostError::failed(
                        "desktop_host_hotkey_unavailable",
                        format!(
                            "global shortcut for action {} is unavailable",
                            action.action_id
                        ),
                    ));
                }
                self.registered_hotkeys.push(id);
            }
        }

        self.add_notification_icon()
    }

    fn add_notification_icon(&mut self) -> Result<(), DesktopHostError> {
        let mut icon: NOTIFYICONDATAW = unsafe { zeroed() };
        icon.cbSize = size_of::<NOTIFYICONDATAW>() as u32;
        icon.hWnd = self.hwnd;
        icon.uID = TRAY_ID;
        icon.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP;
        icon.uCallbackMessage = WM_TRAY;
        icon.hIcon = unsafe { LoadIconW(null_mut(), IDI_APPLICATION) };
        let tip = "Desktop actions".encode_utf16();
        for (slot, value) in icon.szTip.iter_mut().zip(tip) {
            *slot = value;
        }
        if unsafe { Shell_NotifyIconW(NIM_ADD, &icon) } == 0 {
            return Err(last_error("desktop_host_add_icon"));
        }
        self.icon_added = true;
        Ok(())
    }

    pub(crate) fn poll_action(
        &mut self,
        timeout: Duration,
    ) -> Result<Option<u32>, DesktopHostError> {
        self.require_owner()?;
        if self.closed {
            return Err(DesktopHostError::failed(
                "desktop_host_closed",
                "desktop host is already closed",
            ));
        }
        let timeout_ms = timeout.as_millis().min(u32::MAX as u128) as u32;
        if timeout_ms == 0 {
            return self.drain_ready_messages();
        }
        self.purge_timer_messages();
        if unsafe { SetTimer(self.hwnd, TIMER_ID, timeout_ms, None) } == 0 {
            return Err(last_error("desktop_host_set_timer"));
        }
        let result = self.wait_for_action();
        unsafe { KillTimer(self.hwnd, TIMER_ID) };
        self.purge_timer_messages();
        result
    }

    fn purge_timer_messages(&self) {
        let mut message: MSG = unsafe { zeroed() };
        while unsafe { PeekMessageW(&mut message, self.hwnd, WM_TIMER, WM_TIMER, PM_REMOVE) } != 0 {
        }
    }

    fn drain_ready_messages(&mut self) -> Result<Option<u32>, DesktopHostError> {
        loop {
            let mut message: MSG = unsafe { zeroed() };
            if unsafe { PeekMessageW(&mut message, self.hwnd, 0, 0, PM_REMOVE) } == 0 {
                return Ok(None);
            }
            if let Some(action) = self.process_message(message)? {
                return Ok(Some(action));
            }
        }
    }

    fn wait_for_action(&mut self) -> Result<Option<u32>, DesktopHostError> {
        loop {
            let mut message: MSG = unsafe { zeroed() };
            let result = unsafe { GetMessageW(&mut message, self.hwnd, 0, 0) };
            if result == -1 {
                return Err(last_error("desktop_host_get_message"));
            }
            if result == 0 || message.message == WM_TIMER {
                return Ok(None);
            }
            if let Some(action) = self.process_message(message)? {
                return Ok(Some(action));
            }
        }
    }

    fn process_message(&mut self, message: MSG) -> Result<Option<u32>, DesktopHostError> {
        if message.message == WM_HOTKEY {
            let index = message.wParam.saturating_sub(1);
            return self
                .actions
                .get(index)
                .map(|action| Some(action.action_id))
                .ok_or_else(|| {
                    DesktopHostError::failed(
                        "desktop_host_bad_native_action",
                        "received an unknown native hotkey id",
                    )
                });
        }
        if message.message == WM_SHOW_MENU {
            return self.show_menu();
        }
        if message.message == self.taskbar_created_message {
            self.icon_added = false;
            self.add_notification_icon()?;
            return Ok(None);
        }
        unsafe {
            TranslateMessage(&message);
            DispatchMessageW(&message);
        }
        Ok(None)
    }

    fn show_menu(&self) -> Result<Option<u32>, DesktopHostError> {
        let menu = unsafe { CreatePopupMenu() };
        if menu.is_null() {
            return Err(last_error("desktop_host_create_menu"));
        }
        struct MenuGuard(windows_sys::Win32::UI::WindowsAndMessaging::HMENU);
        impl Drop for MenuGuard {
            fn drop(&mut self) {
                unsafe { DestroyMenu(self.0) };
            }
        }
        let _guard = MenuGuard(menu);
        for (index, action) in self.actions.iter().enumerate() {
            if unsafe { AppendMenuW(menu, MF_STRING, index + 1, action.menu_text.as_ptr()) } == 0 {
                return Err(last_error("desktop_host_append_menu"));
            }
        }
        let mut point = POINT { x: 0, y: 0 };
        if unsafe { GetCursorPos(&mut point) } == 0 {
            return Err(last_error("desktop_host_cursor_position"));
        }
        unsafe { SetForegroundWindow(self.hwnd) };
        let command = unsafe {
            TrackPopupMenuEx(
                menu,
                TPM_RETURNCMD | TPM_RIGHTBUTTON | TPM_LEFTALIGN | TPM_BOTTOMALIGN,
                point.x,
                point.y,
                self.hwnd,
                null(),
            )
        } as usize;
        unsafe { PostMessageW(self.hwnd, WM_NULL, 0, 0) };
        if command == 0 {
            return Ok(None);
        }
        self.actions
            .get(command - 1)
            .map(|action| Some(action.action_id))
            .ok_or_else(|| {
                DesktopHostError::failed(
                    "desktop_host_bad_native_action",
                    "menu returned an unknown command id",
                )
            })
    }

    pub(crate) fn close(&mut self) -> Result<(), DesktopHostError> {
        self.require_owner()?;
        self.cleanup_native();
        Ok(())
    }

    fn require_owner(&self) -> Result<(), DesktopHostError> {
        if thread::current().id() == self.owner {
            Ok(())
        } else {
            Err(DesktopHostError::failed(
                "desktop_host_wrong_thread",
                "desktop host must be polled and closed on its creating thread",
            ))
        }
    }

    fn cleanup_native(&mut self) {
        if self.closed {
            return;
        }
        if self.icon_added {
            let mut icon: NOTIFYICONDATAW = unsafe { zeroed() };
            icon.cbSize = size_of::<NOTIFYICONDATAW>() as u32;
            icon.hWnd = self.hwnd;
            icon.uID = TRAY_ID;
            unsafe { Shell_NotifyIconW(NIM_DELETE, &icon) };
            self.icon_added = false;
        }
        for id in self.registered_hotkeys.drain(..) {
            unsafe { UnregisterHotKey(self.hwnd, id) };
        }
        if !self.hwnd.is_null() {
            unsafe { DestroyWindow(self.hwnd) };
            self.hwnd = null_mut();
        }
        if !self.class_name.is_empty() {
            unsafe { UnregisterClassW(self.class_name.as_ptr(), self.instance) };
        }
        self.closed = true;
    }
}

impl Drop for DesktopHost {
    fn drop(&mut self) {
        if thread::current().id() == self.owner {
            self.cleanup_native();
        }
    }
}

fn prepare_actions(actions: Vec<DesktopActionSpec>) -> Result<Vec<NativeAction>, DesktopHostError> {
    let mut hotkeys = HashSet::new();
    actions
        .into_iter()
        .map(|action| {
            let hotkey = action.shortcut.as_deref().map(parse_shortcut).transpose()?;
            if hotkey.is_some_and(|value| !hotkeys.insert(value)) {
                return Err(DesktopHostError::failed(
                    "desktop_host_duplicate_hotkey",
                    format!(
                        "shortcut for action {} duplicates another hotkey",
                        action.action_id
                    ),
                ));
            }
            let mut text = action.label;
            if let Some(shortcut) = action.shortcut {
                text.push('\t');
                text.push_str(&shortcut);
            }
            Ok(NativeAction {
                action_id: action.action_id,
                menu_text: wide(&text),
                hotkey,
            })
        })
        .collect()
}

fn parse_shortcut(text: &str) -> Result<(u32, u32), DesktopHostError> {
    let mut modifiers = MOD_NOREPEAT;
    let mut key = None;
    for part in text.split('+').map(str::trim) {
        let lower = part.to_ascii_lowercase();
        match lower.as_str() {
            "alt" => modifiers |= MOD_ALT,
            "ctrl" | "control" => modifiers |= MOD_CONTROL,
            "shift" => modifiers |= MOD_SHIFT,
            "win" | "super" | "meta" => modifiers |= MOD_WIN,
            _ => {
                if key.is_some() || part.is_empty() {
                    return Err(bad_shortcut(text));
                }
                key = Some(virtual_key(&lower).ok_or_else(|| bad_shortcut(text))?);
            }
        }
    }
    key.map(|key| (modifiers, key))
        .ok_or_else(|| bad_shortcut(text))
}

fn bad_shortcut(text: &str) -> DesktopHostError {
    DesktopHostError::failed(
        "desktop_host_bad_shortcut",
        format!("unsupported shortcut syntax {text:?}"),
    )
}

fn virtual_key(key: &str) -> Option<u32> {
    let named = match key {
        "left" => VK_LEFT,
        "right" => VK_RIGHT,
        "up" => VK_UP,
        "down" => VK_DOWN,
        "home" => VK_HOME,
        "end" => VK_END,
        "pageup" => VK_PRIOR,
        "pagedown" => VK_NEXT,
        "insert" => VK_INSERT,
        "delete" => VK_DELETE,
        "backspace" => VK_BACK,
        "enter" | "return" => VK_RETURN,
        "escape" | "esc" => VK_ESCAPE,
        "space" => VK_SPACE,
        "tab" => VK_TAB,
        _ => 0,
    };
    if named != 0 {
        return Some(named as u32);
    }
    if key.len() == 1 {
        let byte = key.as_bytes()[0];
        if byte.is_ascii_alphanumeric() {
            return Some(byte.to_ascii_uppercase() as u32);
        }
    }
    let number = key.strip_prefix('f')?.parse::<u32>().ok()?;
    (1..=24)
        .contains(&number)
        .then_some(VK_F1 as u32 + number - 1)
}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

fn last_error(code: &'static str) -> DesktopHostError {
    DesktopHostError::failed(code, std::io::Error::last_os_error().to_string())
}

unsafe extern "system" fn window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    std::panic::catch_unwind(|| {
        if message == WM_TRAY {
            let native_message = lparam as u32;
            if matches!(native_message, WM_LBUTTONUP | WM_RBUTTONUP | WM_CONTEXTMENU) {
                unsafe { PostMessageW(hwnd, WM_SHOW_MENU, 0, 0) };
            }
            0
        } else {
            unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
        }
    })
    .unwrap_or(0)
}
