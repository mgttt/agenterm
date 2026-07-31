//! Linux passive WebKitGTK runtime discovery.

use std::path::PathBuf;

use crate::platform::contract::webview::{SystemWebViewProbe, probe_files};

pub(crate) fn probe_system_webview() -> SystemWebViewProbe {
    const DIRECTORIES: [&str; 6] = [
        "/usr/lib",
        "/usr/lib64",
        "/usr/lib/x86_64-linux-gnu",
        "/usr/lib/aarch64-linux-gnu",
        "/lib",
        "/lib64",
    ];
    const NAMES: [&str; 3] = [
        "libwebkit2gtk-4.1.so.0",
        "libwebkit2gtk-4.0.so.37",
        "libwebkit2gtk-4.0.so.0",
    ];
    let paths = DIRECTORIES.into_iter().flat_map(|directory| {
        NAMES
            .into_iter()
            .map(move |name| PathBuf::from(directory).join(name))
    });
    probe_files(
        "webkitgtk",
        paths,
        "system_library",
        "webkitgtk_runtime_not_found",
    )
}
