use agenterm_cc_web::{ASSET_VERSION, CONTRACT_VERSION, asset_for_path};
use serde::Serialize;
use std::env;
use std::process::ExitCode;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;
use tauri::WebviewUrl;
use tauri::webview::{NewWindowResponse, PageLoadEvent, WebviewWindowBuilder};

const TAURI_LOCAL_URL: &str = "tauri://localhost/index.html";

#[derive(Debug, Serialize)]
struct HostReceipt {
    schema: &'static str,
    implementation: &'static str,
    status: &'static str,
    reason: String,
    runtime: &'static str,
    runtime_version: Option<String>,
    packaged_assets: &'static str,
    local_url: &'static str,
    bridge: &'static str,
    registered_commands: usize,
    registered_plugins: usize,
    capabilities: usize,
    runtime_download: bool,
    active_renderer: &'static str,
    no_activate: bool,
    load_complete_ms: Option<u128>,
}

fn main() -> ExitCode {
    let arguments: Vec<String> = env::args().skip(1).collect();
    if arguments
        .iter()
        .any(|argument| argument == "--help" || argument == "-h")
    {
        println!(
            "agenterm-cc-web-tauri (experimental reference)\n\nUSAGE:\n  agenterm-cc-web-tauri --probe\n  agenterm-cc-web-tauri [--smoke] [--no-activate]"
        );
        return ExitCode::SUCCESS;
    }
    if arguments
        .iter()
        .any(|argument| !matches!(argument.as_str(), "--probe" | "--smoke" | "--no-activate"))
    {
        eprintln!("unsupported argument; use --help");
        return ExitCode::from(64);
    }
    let no_activate = env::var_os("AGENTERM_NO_ACTIVATE").is_some()
        || arguments.iter().any(|argument| argument == "--no-activate");
    let runtime_version = match tauri::webview_version() {
        Ok(version) => version,
        Err(error) => {
            return print_receipt(
                unavailable(no_activate, format!("system runtime unavailable: {error}")),
                ExitCode::from(69),
            );
        }
    };
    if arguments.iter().any(|argument| argument == "--probe") {
        return print_receipt(
            receipt(
                "available",
                "system runtime version query succeeded".to_string(),
                Some(runtime_version),
                no_activate,
                None,
            ),
            ExitCode::SUCCESS,
        );
    }
    run_host(
        runtime_version,
        no_activate,
        arguments.iter().any(|argument| argument == "--smoke"),
    )
}

fn run_host(runtime_version: String, no_activate: bool, smoke: bool) -> ExitCode {
    let started = Instant::now();
    let reported = Arc::new(AtomicBool::new(false));
    let loaded = Arc::clone(&reported);
    let version_for_load = runtime_version.clone();
    let app = match tauri::Builder::default()
        .setup(move |app| {
            let app_handle = app.handle().clone();
            WebviewWindowBuilder::new(app, "main", WebviewUrl::App("index.html".into()))
                .title("AgenTerm Cockpit — experimental Tauri reference")
                .inner_size(980.0, 680.0)
                .visible(!no_activate)
                .focused(!no_activate)
                .incognito(true)
                .devtools(false)
                .zoom_hotkeys_enabled(false)
                .on_navigation(|url| is_allowed_tauri_navigation(url.as_str()))
                .on_new_window(|_, _| NewWindowResponse::Deny)
                .on_download(|_, _| false)
                .on_page_load(move |_window, payload| {
                    if smoke
                        && matches!(payload.event(), PageLoadEvent::Finished)
                        && is_allowed_tauri_navigation(payload.url().as_str())
                        && !loaded.swap(true, Ordering::AcqRel)
                    {
                        let _ = print_receipt(
                            receipt(
                                "loaded",
                                "packaged Cockpit load completed".to_string(),
                                Some(version_for_load.clone()),
                                no_activate,
                                Some(started.elapsed().as_millis()),
                            ),
                            ExitCode::SUCCESS,
                        );
                        app_handle.exit(0);
                    }
                })
                .build()?;
            Ok(())
        })
        .build(tauri::generate_context!())
    {
        Ok(app) => app,
        Err(error) => {
            return print_receipt(
                unavailable(no_activate, format!("Tauri host creation failed: {error}")),
                ExitCode::from(69),
            );
        }
    };
    let exit_code = app.run_return(|_, _| {});
    if smoke && !reported.load(Ordering::Acquire) {
        return print_receipt(
            unavailable(
                no_activate,
                format!("Tauri event loop exited before packaged page load: {exit_code}"),
            ),
            ExitCode::from(69),
        );
    }
    ExitCode::from(u8::try_from(exit_code).unwrap_or(1))
}

fn is_allowed_tauri_navigation(url: &str) -> bool {
    let Some((origin, path)) = url
        .strip_prefix("tauri://localhost")
        .map(|path| ("tauri", path))
        .or_else(|| {
            url.strip_prefix("http://tauri.localhost")
                .map(|path| ("http", path))
        })
    else {
        return false;
    };
    let _ = origin;
    if path.contains(['?', '#', '\\']) || path.contains("..") {
        return false;
    }
    let path = if path.is_empty() || path == "/" {
        "/index.html"
    } else {
        path
    };
    asset_for_path(path).is_some()
}

fn unavailable(no_activate: bool, reason: String) -> HostReceipt {
    receipt("unavailable", reason, None, no_activate, None)
}

fn receipt(
    status: &'static str,
    reason: String,
    runtime_version: Option<String>,
    no_activate: bool,
    load_complete_ms: Option<u128>,
) -> HostReceipt {
    HostReceipt {
        schema: CONTRACT_VERSION,
        implementation: "tauri-v2-reference",
        status,
        reason,
        runtime: platform_runtime(),
        runtime_version,
        packaged_assets: ASSET_VERSION,
        local_url: TAURI_LOCAL_URL,
        bridge: "absent",
        registered_commands: 0,
        registered_plugins: 0,
        capabilities: 0,
        runtime_download: false,
        active_renderer: if status == "unavailable" {
            "native"
        } else {
            "experimental-tauri-v2"
        },
        no_activate,
        load_complete_ms,
    }
}

fn platform_runtime() -> &'static str {
    if cfg!(target_os = "windows") {
        "webview2"
    } else if cfg!(target_os = "macos") {
        "wkwebview"
    } else if cfg!(target_os = "linux") {
        "webkitgtk-4.1"
    } else {
        "unsupported"
    }
}

fn print_receipt(receipt: HostReceipt, code: ExitCode) -> ExitCode {
    println!(
        "{}",
        serde_json::to_string_pretty(&receipt).expect("receipt serializes")
    );
    code
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn navigation_is_restricted_to_exact_tauri_asset_routes() {
        for allowed in [
            "tauri://localhost/",
            "tauri://localhost/index.html",
            "http://tauri.localhost/app.css",
            "http://tauri.localhost/app.js",
        ] {
            assert!(is_allowed_tauri_navigation(allowed), "{allowed}");
        }
        for denied in [
            "https://example.com/",
            "tauri://evil/index.html",
            "tauri://localhost/missing",
            "tauri://localhost/../index.html",
            "tauri://localhost/index.html?remote=true",
            "http://tauri.localhost.evil/app.js",
        ] {
            assert!(!is_allowed_tauri_navigation(denied), "{denied}");
        }
    }
}
