//! Fool-proof Accessibility onboarding for the macOS hotkey host.
//!
//! Users do not know what TCC is. We open the Accessibility pane, put a
//! highlighted always-on-top card beside it, and keep checking. If they turn
//! the switch off later, the card comes back.
//!
//! The card must never become key on a timer. Activating ourselves steals the
//! click they need to flip the switch in System Settings.

#![cfg(target_os = "macos")]

use std::ffi::c_void;
use std::time::Instant;

use dispatch::Queue;
use objc2_app_kit::{
    NSBackingStoreType, NSColor, NSFont, NSScreen, NSTextField, NSWindow,
    NSWindowCollectionBehavior, NSWindowStyleMask, NSWorkspace,
};
use objc2_foundation::{
    MainThreadMarker, NSDictionary, NSNumber, NSPoint, NSRect, NSSize, NSString, NSURL,
};

use crate::ax_guide_policy::{GuideState, TickOut};

const SETTINGS_URLS: &[&str] = &[
    "x-apple.systempreferences:com.apple.settings.PrivacySecurity.extension?Privacy_Accessibility",
    "x-apple.systempreferences:com.apple.Settings.PrivacySecurity.extension?Privacy_Accessibility",
    "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility",
];

const SETTINGS_BUNDLES: &[&str] = &["com.apple.systempreferences", "com.apple.Settings"];

const CARD_W: f64 = 420.0;
const CARD_H: f64 = 520.0;
const NS_FLOATING_WINDOW_LEVEL: isize = 3;

static mut GUIDE: *mut Guide = std::ptr::null_mut();

struct Guide {
    window: Option<objc2::rc::Retained<NSWindow>>,
    origin: Instant,
    state: GuideState,
}

pub fn start() {
    let boxed = Box::new(Guide {
        window: None,
        origin: Instant::now(),
        state: GuideState::default(),
    });
    unsafe {
        GUIDE = Box::into_raw(boxed);
    }
    tick();
    let _ = std::thread::Builder::new()
        .name("ax-guide".into())
        .spawn(|| loop {
            std::thread::sleep(std::time::Duration::from_secs(2));
            Queue::main().exec_async(|| tick());
        });
}

/// A placement failed because AX is off. Drop cooldowns and help now.
pub fn nudge() {
    let run = || {
        if let Some(guide) = unsafe { GUIDE.as_mut() } {
            guide.state.force_help();
        }
        tick();
    };
    if MainThreadMarker::new().is_some() {
        run();
    } else {
        Queue::main().exec_async(run);
    }
}

pub fn ax_trusted() -> bool {
    #[link(name = "ApplicationServices", kind = "framework")]
    unsafe extern "C" {
        fn AXIsProcessTrusted() -> u8;
    }
    unsafe { AXIsProcessTrusted() != 0 }
}

fn tick() {
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    let Some(guide) = (unsafe { GUIDE.as_mut() }) else {
        return;
    };
    let now_s = guide.origin.elapsed().as_secs();
    if guide.state.visible {
        if let Some(window) = &guide.window {
            if !window.isVisible() {
                guide.state.note_closed(now_s);
            }
        }
    }
    let out = guide.state.tick(now_s, ax_trusted(), settings_is_front());
    apply(guide, mtm, out);
}

fn apply(guide: &mut Guide, mtm: MainThreadMarker, out: TickOut) {
    if !out.show {
        if let Some(window) = &guide.window {
            window.orderOut(None);
        }
        return;
    }
    if out.prompt_system {
        prompt_system_dialog();
    }
    if out.open_settings {
        open_accessibility_settings();
    }
    let window = guide.window.get_or_insert_with(|| build_window(mtm));
    place_beside_settings(window, mtm);
    // Visible, not key. Settings must keep the click.
    unsafe {
        window.orderFrontRegardless();
    }
}

pub fn open_accessibility_settings() {
    unsafe {
        let workspace = NSWorkspace::sharedWorkspace();
        for raw in SETTINGS_URLS {
            let s = NSString::from_str(raw);
            if let Some(url) = NSURL::URLWithString(&s) {
                if workspace.openURL(&url) {
                    return;
                }
            }
        }
    }
}

fn settings_is_front() -> bool {
    unsafe {
        let Some(app) = NSWorkspace::sharedWorkspace().frontmostApplication() else {
            return false;
        };
        let Some(id) = app.bundleIdentifier() else {
            return false;
        };
        SETTINGS_BUNDLES.iter().any(|b| id.to_string() == *b)
    }
}

fn prompt_system_dialog() {
    #[link(name = "ApplicationServices", kind = "framework")]
    unsafe extern "C" {
        fn AXIsProcessTrustedWithOptions(options: *const c_void) -> u8;
    }
    let key = NSString::from_str("AXTrustedCheckOptionPrompt");
    let yes = NSNumber::numberWithBool(true);
    let dict = NSDictionary::from_id_slice(&[&*key], &[yes]);
    unsafe {
        let _ = AXIsProcessTrustedWithOptions(objc2::rc::Retained::as_ptr(&dict).cast());
    }
}

fn place_beside_settings(window: &NSWindow, mtm: MainThreadMarker) {
    let Some(screen) = NSScreen::mainScreen(mtm) else {
        window.center();
        return;
    };
    let visible = screen.visibleFrame();
    let x = visible.origin.x + visible.size.width - CARD_W - 24.0;
    let y = visible.origin.y + ((visible.size.height - CARD_H) / 2.0).max(24.0);
    window.setFrame_display(
        NSRect::new(NSPoint::new(x, y), NSSize::new(CARD_W, CARD_H)),
        true,
    );
}

fn build_window(mtm: MainThreadMarker) -> objc2::rc::Retained<NSWindow> {
    let rect = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(CARD_W, CARD_H));
    let window = unsafe {
        NSWindow::initWithContentRect_styleMask_backing_defer(
            mtm.alloc::<NSWindow>(),
            rect,
            NSWindowStyleMask::Titled | NSWindowStyleMask::Closable,
            NSBackingStoreType::NSBackingStoreBuffered,
            false,
        )
    };
    window.setTitle(&NSString::from_str("AgentermCu — 还差一步"));
    unsafe {
        window.setReleasedWhenClosed(false);
        window.setLevel(NS_FLOATING_WINDOW_LEVEL);
        window.setHidesOnDeactivate(false);
        window.setCollectionBehavior(
            NSWindowCollectionBehavior::CanJoinAllSpaces
                | NSWindowCollectionBehavior::FullScreenAuxiliary,
        );
        window.setBackgroundColor(Some(&NSColor::whiteColor()));
    }

    let title = label(
        mtm,
        "已经帮你打开设置了。\n你只要拨一下开关。",
        22.0,
        true,
        unsafe { NSColor::blackColor() },
        20.0,
        420.0,
        380.0,
        80.0,
    );
    let highlight = label(
        mtm,
        "打开这个名字",
        13.0,
        false,
        unsafe { NSColor::blackColor() },
        20.0,
        268.0,
        380.0,
        22.0,
    );
    unsafe {
        highlight.setDrawsBackground(true);
        highlight.setBackgroundColor(Some(&NSColor::colorWithCalibratedRed_green_blue_alpha(
            1.0, 0.93, 0.2, 1.0,
        )));
    }
    let name = label(
        mtm,
        "AgentermCu",
        36.0,
        true,
        unsafe { NSColor::blackColor() },
        20.0,
        188.0,
        380.0,
        72.0,
    );
    unsafe {
        name.setDrawsBackground(true);
        name.setBackgroundColor(Some(&NSColor::colorWithCalibratedRed_green_blue_alpha(
            1.0, 0.93, 0.2, 1.0,
        )));
    }
    let toggle = label(
        mtm,
        "  开关拨到右边：开  ",
        16.0,
        true,
        unsafe { NSColor::whiteColor() },
        20.0,
        148.0,
        380.0,
        32.0,
    );
    unsafe {
        toggle.setDrawsBackground(true);
        toggle.setBackgroundColor(Some(&NSColor::colorWithCalibratedRed_green_blue_alpha(
            0.15, 0.68, 0.28, 1.0,
        )));
    }
    let steps = label(
        mtm,
        "1. 左边列表找到上面这个黄底名字\n\
         2. 把右边开关打开（变绿）\n\
         3. 不用关本窗口，打开后会自己消失",
        15.0,
        false,
        unsafe { NSColor::blackColor() },
        20.0,
        68.0,
        380.0,
        76.0,
    );
    let warn = label(
        mtm,
        "不要点旧的「agenterm-cu」。那个是过期的命令行，打了对快捷键也没用。",
        13.0,
        false,
        unsafe { NSColor::colorWithCalibratedRed_green_blue_alpha(0.75, 0.08, 0.08, 1.0) },
        20.0,
        20.0,
        380.0,
        44.0,
    );

    if let Some(content) = window.contentView() {
        unsafe {
            content.addSubview(&title);
            content.addSubview(&highlight);
            content.addSubview(&name);
            content.addSubview(&toggle);
            content.addSubview(&steps);
            content.addSubview(&warn);
        }
    }
    window
}

fn label(
    mtm: MainThreadMarker,
    text: &str,
    size: f64,
    bold: bool,
    color: objc2::rc::Retained<NSColor>,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
) -> objc2::rc::Retained<NSTextField> {
    let field = unsafe { NSTextField::wrappingLabelWithString(&NSString::from_str(text), mtm) };
    unsafe {
        let font = if bold {
            NSFont::boldSystemFontOfSize(size)
        } else {
            NSFont::systemFontOfSize(size)
        };
        field.setFont(Some(&font));
        field.setTextColor(Some(&color));
        field.setFrame(NSRect::new(NSPoint::new(x, y), NSSize::new(w, h)));
    }
    field
}
