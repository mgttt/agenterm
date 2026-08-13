//! Local hotkey host that fires Spectacle-default placements.
//!
//! This is not a menu-bar app. It only registers global Carbon hotkeys and
//! runs `window-place` in-process. macOS only.

use crate::place::PlaceAction;

#[cfg(not(target_os = "macos"))]
pub fn run() -> i32 {
    eprintln!("cu hotkeys is only implemented on macOS");
    1
}

#[cfg(target_os = "macos")]
pub fn run() -> i32 {
    macos::run()
}

#[cfg(target_os = "macos")]
mod macos {
    use super::PlaceAction;
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
        action: PlaceAction,
        key: u32,
        modifiers: u32,
    }

    fn bindings() -> [Bind; 18] {
        [
            Bind {
                action: PlaceAction::Center,
                key: K_VK_ANSI_C,
                modifiers: OPTION | CMD,
            },
            Bind {
                action: PlaceAction::Fullscreen,
                key: K_VK_ANSI_F,
                modifiers: OPTION | CMD,
            },
            Bind {
                action: PlaceAction::LeftHalf,
                key: K_VK_LEFT,
                modifiers: OPTION | CMD,
            },
            Bind {
                action: PlaceAction::RightHalf,
                key: K_VK_RIGHT,
                modifiers: OPTION | CMD,
            },
            Bind {
                action: PlaceAction::TopHalf,
                key: K_VK_UP,
                modifiers: OPTION | CMD,
            },
            Bind {
                action: PlaceAction::BottomHalf,
                key: K_VK_DOWN,
                modifiers: OPTION | CMD,
            },
            Bind {
                action: PlaceAction::UpperLeft,
                key: K_VK_LEFT,
                modifiers: CONTROL | CMD,
            },
            Bind {
                action: PlaceAction::LowerLeft,
                key: K_VK_LEFT,
                modifiers: CONTROL | SHIFT | CMD,
            },
            Bind {
                action: PlaceAction::UpperRight,
                key: K_VK_RIGHT,
                modifiers: CONTROL | CMD,
            },
            Bind {
                action: PlaceAction::LowerRight,
                key: K_VK_RIGHT,
                modifiers: CONTROL | SHIFT | CMD,
            },
            Bind {
                action: PlaceAction::NextDisplay,
                key: K_VK_RIGHT,
                modifiers: CONTROL | OPTION | CMD,
            },
            Bind {
                action: PlaceAction::PreviousDisplay,
                key: K_VK_LEFT,
                modifiers: CONTROL | OPTION | CMD,
            },
            Bind {
                action: PlaceAction::NextThird,
                key: K_VK_RIGHT,
                modifiers: CONTROL | OPTION,
            },
            Bind {
                action: PlaceAction::PreviousThird,
                key: K_VK_LEFT,
                modifiers: CONTROL | OPTION,
            },
            Bind {
                action: PlaceAction::Larger,
                key: K_VK_RIGHT,
                modifiers: CONTROL | OPTION | SHIFT,
            },
            Bind {
                action: PlaceAction::Smaller,
                key: K_VK_LEFT,
                modifiers: CONTROL | OPTION | SHIFT,
            },
            Bind {
                action: PlaceAction::Undo,
                key: K_VK_ANSI_Z,
                modifiers: OPTION | CMD,
            },
            Bind {
                action: PlaceAction::Redo,
                key: K_VK_ANSI_Z,
                modifiers: OPTION | SHIFT | CMD,
            },
        ]
    }

    static mut HOST: *mut Host = std::ptr::null_mut();

    struct Host {
        executor: Executor,
        actions: Vec<PlaceAction>,
    }

    pub fn run() -> i32 {
        if bootstrap_nsapp().is_err() {
            eprintln!("cu hotkeys: failed to start NSApplication");
            return 1;
        }
        let auth = Authorization::new([Grant::Observe, Grant::Actuate].into_iter().collect());
        let mut host = Host {
            executor: Executor::new(auth),
            actions: bindings().iter().map(|b| b.action).collect(),
        };
        unsafe {
            HOST = &mut host;
            let app_target = GetApplicationEventTarget();
            let target = GetEventDispatcherTarget();
            if app_target.is_null() || target.is_null() {
                eprintln!("cu hotkeys: no event dispatcher");
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
                eprintln!("cu hotkeys: InstallEventHandler failed ({err})");
                return 1;
            }
            for (index, bind) in bindings().iter().enumerate() {
                let id = EventHotKeyId {
                    signature: SIGNATURE,
                    id: (index as u32) + 1,
                };
                let mut href = std::ptr::null_mut();
                let err = RegisterEventHotKey(bind.key, bind.modifiers, id, target, 0, &mut href);
                if err != 0 {
                    eprintln!(
                        "cu hotkeys: failed to register {} (err {err})",
                        bind.action.kebab()
                    );
                }
            }
        }
        eprintln!("cu hotkeys: listening with Spectacle defaults");
        crate::ax_guide::start();
        run_nsapp();
        0
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
        let index = (hot_id.id as usize).saturating_sub(1);
        let Some(action) = host.actions.get(index).copied() else {
            return 0;
        };
        let command = Command::WindowPlace {
            target: TargetRef::Current,
            action: action.kebab().to_string(),
            window: None,
        };
        let reply = host.executor.execute(&command);
        if !reply.ok {
            if let Some(error) = reply.error {
                eprintln!(
                    "cu hotkeys: {} failed: {} ({})",
                    action.kebab(),
                    error.message,
                    error.code
                );
                if error.code == "ax_api_disabled" {
                    crate::ax_guide::nudge();
                }
            }
        }
        0
    }
}
