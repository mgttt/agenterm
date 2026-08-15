//! Desktop host that fires the shared placement action catalog.
//!
//! Registers global Carbon hotkeys, runs `window-place` in-process, and shows
//! a menu-bar extra. Accessibility is checked only when that menu opens.

#[cfg(target_os = "macos")]
use crate::place::PlaceAction;

#[cfg(windows)]
pub fn run() -> i32 {
    windows::run()
}

#[cfg(all(not(windows), not(target_os = "macos")))]
pub fn run() -> i32 {
    eprintln!("agenterm-cu host is not implemented on this platform yet");
    1
}

#[cfg(target_os = "macos")]
pub fn run() -> i32 {
    let self_test = std::env::args().any(|a| a == "--self-test");
    if self_test {
        return macos::self_test();
    }
    macos::run()
}

#[cfg(target_os = "macos")]
mod macos {
    use super::PlaceAction;
    use crate::host_actions;
    use crate::{Authorization, Command, Executor, Grant, TargetRef};
    use std::os::raw::{c_uint, c_void};

    const CMD: u32 = 1 << 8;
    const SHIFT: u32 = 1 << 9;
    const OPTION: u32 = 1 << 11;
    const CONTROL: u32 = 1 << 12;

    const K_VK_ANSI_C: u32 = 0x08;
    const K_VK_ANSI_F: u32 = 0x03;
    const K_VK_ANSI_Z: u32 = 0x06;
    const K_VK_LEFT: u32 = 0x7B;
    const K_VK_RIGHT: u32 = 0x7C;
    const K_VK_DOWN: u32 = 0x7D;
    const K_VK_UP: u32 = 0x7E;

    const K_EVENT_CLASS_KEYBOARD: u32 = 0x6B65_7962; // 'keyb'
    const K_EVENT_HOT_KEY_PRESSED: u32 = 5;
    const K_EVENT_PARAM_DIRECT_OBJECT: u32 = 0x2D2D_2D2D; // '----'
    const TYPE_EVENT_HOT_KEY_ID: u32 = 0x686B_6964; // 'hkid'
    const SIGNATURE: u32 = 0x4355_484B; // 'CUHK'

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct EventHotKeyId {
        signature: u32,
        id: u32,
    }

    #[repr(C)]
    struct EventTypeSpec {
        event_class: u32,
        event_kind: u32,
    }

    type EventRef = *mut c_void;
    type EventTargetRef = *mut c_void;
    type EventHotKeyRef = *mut c_void;
    type EventHandlerRef = *mut c_void;
    type EventHandlerCallRef = *mut c_void;

    // One `#[link]` per framework is the documented way to link several of
    // them to a single extern block; clippy reads the repeated attribute name
    // as a copy-paste slip.
    #[allow(clippy::duplicated_attributes)]
    #[link(name = "Carbon", kind = "framework")]
    #[link(name = "CoreFoundation", kind = "framework")]
    #[link(name = "ApplicationServices", kind = "framework")]
    unsafe extern "C" {
        fn RegisterEventHotKey(
            key_code: u32,
            modifiers: u32,
            hot_key_id: EventHotKeyId,
            target: EventTargetRef,
            options: u32,
            out: *mut EventHotKeyRef,
        ) -> i32;
        fn GetEventDispatcherTarget() -> EventTargetRef;
        fn GetApplicationEventTarget() -> EventTargetRef;
        fn InstallEventHandler(
            target: EventTargetRef,
            handler: unsafe extern "C" fn(EventHandlerCallRef, EventRef, *mut c_void) -> i32,
            num_types: c_uint,
            type_list: *const EventTypeSpec,
            user_data: *mut c_void,
            out: *mut EventHandlerRef,
        ) -> i32;
        fn GetEventParameter(
            event: EventRef,
            name: u32,
            desired_type: u32,
            actual_type: *mut u32,
            size: u32,
            actual_size: *mut u32,
            data: *mut c_void,
        ) -> i32;
    }

    struct Bind {
        id: u32,
        action: PlaceAction,
        key: u32,
        modifiers: u32,
    }

    fn bindings() -> [Bind; 18] {
        [
            Bind {
                id: 1,
                action: PlaceAction::Center,
                key: K_VK_ANSI_C,
                modifiers: OPTION | CMD,
            },
            Bind {
                id: 2,
                action: PlaceAction::Fullscreen,
                key: K_VK_ANSI_F,
                modifiers: OPTION | CMD,
            },
            Bind {
                id: 3,
                action: PlaceAction::LeftHalf,
                key: K_VK_LEFT,
                modifiers: OPTION | CMD,
            },
            Bind {
                id: 4,
                action: PlaceAction::RightHalf,
                key: K_VK_RIGHT,
                modifiers: OPTION | CMD,
            },
            Bind {
                id: 5,
                action: PlaceAction::TopHalf,
                key: K_VK_UP,
                modifiers: OPTION | CMD,
            },
            Bind {
                id: 6,
                action: PlaceAction::BottomHalf,
                key: K_VK_DOWN,
                modifiers: OPTION | CMD,
            },
            Bind {
                id: 7,
                action: PlaceAction::UpperLeft,
                key: K_VK_LEFT,
                modifiers: CONTROL | CMD,
            },
            Bind {
                id: 8,
                action: PlaceAction::LowerLeft,
                key: K_VK_LEFT,
                modifiers: CONTROL | SHIFT | CMD,
            },
            Bind {
                id: 9,
                action: PlaceAction::UpperRight,
                key: K_VK_RIGHT,
                modifiers: CONTROL | CMD,
            },
            Bind {
                id: 10,
                action: PlaceAction::LowerRight,
                key: K_VK_RIGHT,
                modifiers: CONTROL | SHIFT | CMD,
            },
            Bind {
                id: 11,
                action: PlaceAction::NextDisplay,
                key: K_VK_RIGHT,
                modifiers: CONTROL | OPTION | CMD,
            },
            Bind {
                id: 12,
                action: PlaceAction::PreviousDisplay,
                key: K_VK_LEFT,
                modifiers: CONTROL | OPTION | CMD,
            },
            Bind {
                id: 13,
                action: PlaceAction::NextThird,
                key: K_VK_RIGHT,
                modifiers: CONTROL | OPTION,
            },
            Bind {
                id: 14,
                action: PlaceAction::PreviousThird,
                key: K_VK_LEFT,
                modifiers: CONTROL | OPTION,
            },
            Bind {
                id: 15,
                action: PlaceAction::Larger,
                key: K_VK_RIGHT,
                modifiers: CONTROL | OPTION | SHIFT,
            },
            Bind {
                id: 16,
                action: PlaceAction::Smaller,
                key: K_VK_LEFT,
                modifiers: CONTROL | OPTION | SHIFT,
            },
            Bind {
                id: 17,
                action: PlaceAction::Undo,
                key: K_VK_ANSI_Z,
                modifiers: OPTION | CMD,
            },
            Bind {
                id: 18,
                action: PlaceAction::Redo,
                key: K_VK_ANSI_Z,
                modifiers: OPTION | SHIFT | CMD,
            },
        ]
    }

    static mut HOST: *mut Host = std::ptr::null_mut();

    struct Host {
        executor: Executor,
    }

    pub fn run() -> i32 {
        if bootstrap_nsapp().is_err() {
            eprintln!("agenterm-cu host: failed to start NSApplication");
            return 1;
        }
        crate::ax_guide::ensure_accessibility_surface();
        let auth = Authorization::new([Grant::Observe, Grant::Actuate].into_iter().collect());
        let mut host = Host {
            executor: Executor::new(auth),
        };
        unsafe {
            HOST = &mut host;
            let app_target = GetApplicationEventTarget();
            let target = GetEventDispatcherTarget();
            if app_target.is_null() || target.is_null() {
                eprintln!("agenterm-cu host: no event dispatcher");
                return 1;
            }
            let spec = EventTypeSpec {
                event_class: K_EVENT_CLASS_KEYBOARD,
                event_kind: K_EVENT_HOT_KEY_PRESSED,
            };
            let mut handler = std::ptr::null_mut();
            let err = InstallEventHandler(
                app_target,
                handle_event,
                1,
                &spec,
                std::ptr::null_mut(),
                &mut handler,
            );
            if err != 0 {
                eprintln!("agenterm-cu host: InstallEventHandler failed ({err})");
                return 1;
            }
            for bind in &bindings() {
                let id = EventHotKeyId {
                    signature: SIGNATURE,
                    id: bind.id,
                };
                let mut href = std::ptr::null_mut();
                let err = RegisterEventHotKey(bind.key, bind.modifiers, id, target, 0, &mut href);
                if err != 0 {
                    eprintln!(
                        "agenterm-cu host: failed to register {} (err {err})",
                        bind.action.kebab()
                    );
                }
            }
        }
        let trusted = crate::ax_guide::ax_trusted();
        crate::ax_guide::write_status(trusted);
        eprintln!("agenterm-cu host: listening with Spectacle defaults (ax_trusted={trusted})");
        let _status =
            objc2_foundation::MainThreadMarker::new().and_then(crate::status_menu::install);
        run_nsapp();
        0
    }

    pub fn self_test() -> i32 {
        if bootstrap_nsapp().is_err() {
            eprintln!("agenterm-cu host --self-test: NSApplication failed");
            return 1;
        }
        let trusted = crate::ax_guide::ax_trusted();
        crate::ax_guide::write_status(trusted);
        eprintln!("agenterm-cu host --self-test: ax_trusted={trusted}");
        if !trusted {
            return 2;
        }
        let auth = Authorization::new([Grant::Observe, Grant::Actuate].into_iter().collect());
        let executor = Executor::new(auth);
        let reply = executor.execute(&Command::WindowPlace {
            target: TargetRef::Current,
            action: "center".into(),
            window: None,
        });
        if reply.ok {
            eprintln!("agenterm-cu host --self-test: window-place center ok");
            0
        } else {
            let err = reply.error.as_ref();
            eprintln!(
                "agenterm-cu host --self-test: window-place failed: {} ({})",
                err.map(|e| e.message.as_str()).unwrap_or("?"),
                err.map(|e| e.code.as_str()).unwrap_or("?")
            );
            3
        }
    }

    fn relaunch_for_ax() {
        let Some(home) = std::env::var_os("HOME") else {
            return;
        };
        let stamp = std::path::PathBuf::from(home).join(".local/share/agenterm/ax-relaunch.stamp");
        if let Ok(meta) = std::fs::metadata(&stamp)
            && let Ok(modified) = meta.modified()
            && let Ok(age) = modified.elapsed()
            && age.as_secs() < 20
        {
            return;
        }
        if let Some(dir) = stamp.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(&stamp, b"1");
        eprintln!("agenterm-cu host: process is untrusted; restarting after Accessibility grant");
        // Non-zero so launchd KeepAlive (SuccessfulExit=false) brings us back.
        std::process::exit(1);
    }

    fn bootstrap_nsapp() -> Result<(), ()> {
        use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy};
        use objc2_foundation::MainThreadMarker;
        let mtm = MainThreadMarker::new().ok_or(())?;
        let app = NSApplication::sharedApplication(mtm);
        let _ = app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);
        unsafe {
            app.finishLaunching();
        }
        Ok(())
    }

    fn run_nsapp() {
        use objc2_app_kit::NSApplication;
        use objc2_foundation::MainThreadMarker;
        let Some(mtm) = MainThreadMarker::new() else {
            return;
        };
        unsafe {
            NSApplication::sharedApplication(mtm).run();
        }
    }

    unsafe extern "C" fn handle_event(
        _call: EventHandlerCallRef,
        event: EventRef,
        _data: *mut c_void,
    ) -> i32 {
        let mut hot_id = EventHotKeyId {
            signature: 0,
            id: 0,
        };
        let err = unsafe {
            GetEventParameter(
                event,
                K_EVENT_PARAM_DIRECT_OBJECT,
                TYPE_EVENT_HOT_KEY_ID,
                std::ptr::null_mut(),
                std::mem::size_of::<EventHotKeyId>() as u32,
                std::ptr::null_mut(),
                &mut hot_id as *mut EventHotKeyId as *mut c_void,
            )
        };
        if err != 0 || hot_id.id == 0 {
            return 0;
        }
        let host = unsafe { HOST.as_mut() };
        let Some(host) = host else {
            return 0;
        };
        let Some(action) = host_actions::by_id(hot_id.id) else {
            return 0;
        };
        let Some(reply) = host_actions::execute(&host.executor, hot_id.id) else {
            return 0;
        };
        if !reply.ok
            && let Some(error) = reply.error
        {
            eprintln!(
                "agenterm-cu host: {} failed: {} ({})",
                action.place.kebab(),
                error.message,
                error.code
            );
            if error.code == "ax_api_disabled" {
                relaunch_for_ax();
            }
        }
        0
    }
}

#[cfg(windows)]
mod windows {
    use crate::host_actions::{self, PLACEMENT_ACTIONS, QUIT_ACTION_ID};
    use crate::mechanism::MechanismError;
    use crate::mechanism::desktop_host::{ActionSpec, DesktopHost};
    use crate::{Authorization, Executor, Grant};

    pub fn run() -> i32 {
        let actions = action_specs(true);
        let (host, hotkeys_active) = match DesktopHost::open(&actions) {
            Ok(host) => (host, true),
            Err(MechanismError::Failed { code, message })
                if code == "desktop_host_hotkey_unavailable" =>
            {
                eprintln!(
                    "agenterm-cu host: global shortcuts degraded; menu remains available: {message}"
                );
                match DesktopHost::open(&action_specs(false)) {
                    Ok(host) => (host, false),
                    Err(error) => return report("open menu-only fallback", &error),
                }
            }
            Err(error) => return report("open", &error),
        };
        if std::env::args().any(|arg| arg == "--self-test") {
            return self_test(host, hotkeys_active);
        }
        event_loop(host)
    }

    fn action_specs(with_shortcuts: bool) -> Vec<ActionSpec<'static>> {
        PLACEMENT_ACTIONS
            .iter()
            .map(|action| ActionSpec {
                action_id: action.id,
                label: action.label,
                shortcut: with_shortcuts.then_some(action.windows_shortcut),
            })
            .chain(std::iter::once(ActionSpec {
                action_id: QUIT_ACTION_ID,
                label: "Exit AgentermCu",
                shortcut: None,
            }))
            .collect()
    }

    fn self_test(host: DesktopHost, hotkeys_active: bool) -> i32 {
        let executor = Executor::new(Authorization::new([Grant::Observe].into_iter().collect()));
        let dispatch = host_actions::execute(&executor, PLACEMENT_ACTIONS[0].id);
        let dispatch_shared = dispatch.as_ref().is_some_and(|reply| {
            !reply.ok
                && reply.command == "window-place"
                && reply.target == "current"
                && reply
                    .error
                    .as_ref()
                    .is_some_and(|error| error.code == "refused")
        });
        match host.close() {
            Ok(()) if dispatch_shared => {
                if std::env::args().any(|arg| arg == "--json") {
                    println!(
                        "{}",
                        serde_json::json!({
                            "ok": true,
                            "host": "windows-notification-area",
                            "actions": PLACEMENT_ACTIONS.len() + 1,
                            "hotkeys_active": hotkeys_active,
                            "shared_executor": true,
                            "dispatch_command": "window-place",
                            "dispatch_refused": true,
                            "cleaned_up": true
                        })
                    );
                } else {
                    eprintln!("agenterm-cu host --self-test: open and cleanup ok");
                }
                0
            }
            Ok(()) => {
                eprintln!(
                    "agenterm-cu host --self-test: host action did not traverse Command/Executor"
                );
                1
            }
            Err(error) => report("close", &error),
        }
    }

    fn event_loop(mut host: DesktopHost) -> i32 {
        let auth = Authorization::new([Grant::Observe, Grant::Actuate].into_iter().collect());
        let executor = Executor::new(auth);
        loop {
            let action_id = match host.poll(1_000) {
                Ok(Some(action_id)) => action_id,
                Ok(None) => continue,
                Err(error) => return report("poll", &error),
            };
            if action_id == QUIT_ACTION_ID {
                return match host.close() {
                    Ok(()) => 0,
                    Err(error) => report("close", &error),
                };
            }
            let Some(reply) = host_actions::execute(&executor, action_id) else {
                eprintln!("agenterm-cu host: unknown action id {action_id}");
                continue;
            };
            if !reply.ok
                && let Some(error) = reply.error
            {
                eprintln!(
                    "agenterm-cu host: action {action_id} failed: {} ({})",
                    error.message, error.code
                );
            }
        }
    }

    fn report(operation: &str, error: &MechanismError) -> i32 {
        match error {
            MechanismError::Unsupported { reason } => {
                eprintln!("agenterm-cu host: {operation} unsupported: {reason}");
            }
            MechanismError::Failed { code, message } => {
                eprintln!("agenterm-cu host: {operation} failed: {message} ({code})");
            }
        }
        1
    }
}
