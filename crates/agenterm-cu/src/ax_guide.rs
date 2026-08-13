//! Accessibility helpers for the macOS hotkey host.
//!
//! No popup card and no background poll. We only:
//! - report whether *this* process is trusted
//! - open the Accessibility pane on demand
//! - optionally show Apple's one-shot trust prompt
//! - write a status file so install/self-test can verify without guessing

#![cfg(target_os = "macos")]

use std::ffi::c_void;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use objc2_app_kit::NSWorkspace;
use objc2_foundation::{NSDictionary, NSNumber, NSString, NSURL};

const SETTINGS_URLS: &[&str] = &[
    "x-apple.systempreferences:com.apple.settings.PrivacySecurity.extension?Privacy_Accessibility",
    "x-apple.systempreferences:com.apple.Settings.PrivacySecurity.extension?Privacy_Accessibility",
    "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility",
];

pub fn ax_trusted() -> bool {
    #[link(name = "ApplicationServices", kind = "framework")]
    unsafe extern "C" {
        fn AXIsProcessTrusted() -> u8;
    }
    unsafe { AXIsProcessTrusted() != 0 }
}

pub fn status_path() -> PathBuf {
    let home = std::env::var_os("HOME").unwrap_or_default();
    PathBuf::from(home).join(".local/share/agenterm/ax-status")
}

pub fn write_status(trusted: bool) {
    let path = status_path();
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let body = format!(
        "trusted={}\npid={}\nts={}\n",
        if trusted { "1" } else { "0" },
        std::process::id(),
        ts
    );
    let _ = std::fs::write(path, body);
}

pub fn read_status_trusted() -> Option<bool> {
    let body = std::fs::read_to_string(status_path()).ok()?;
    for line in body.lines() {
        if let Some(v) = line.strip_prefix("trusted=") {
            return Some(v.trim() == "1");
        }
    }
    None
}

/// Show Apple's TCC prompt once and open the Accessibility pane when untrusted.
pub fn ensure_accessibility_surface() {
    let trusted = ax_trusted();
    write_status(trusted);
    if trusted {
        return;
    }
    prompt_system_dialog();
    open_accessibility_settings();
}

pub fn open_accessibility_settings() {
    unsafe {
        let workspace = NSWorkspace::sharedWorkspace();
        for raw in SETTINGS_URLS {
            let s = NSString::from_str(raw);
            if let Some(url) = NSURL::URLWithString(&s)
                && workspace.openURL(&url)
            {
                return;
            }
        }
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
