use agenterm_cc_web::{
    ASSET_VERSION, CONTENT_SECURITY_POLICY, CONTRACT_VERSION, LOCAL_URL, asset_for_path,
    canonical_local_path, is_allowed_navigation,
};
use serde::Serialize;
use std::borrow::Cow;
use std::env;
use std::process::ExitCode;
use std::time::Instant;
use tao::dpi::LogicalSize;
use tao::event::{Event, WindowEvent};
use tao::event_loop::{ControlFlow, EventLoopBuilder};
use tao::window::WindowBuilder;
use wry::http::{Response, StatusCode};
use wry::{NewWindowResponse, PageLoadEvent, WebViewBuilder};

#[derive(Clone, Copy, Debug)]
enum HostEvent {
    PageLoaded,
}

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
    active_renderer: &'static str,
    no_activate: bool,
    load_complete_ms: Option<u128>,
}

fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();
    if args.iter().any(|arg| arg == "--help" || arg == "-h") {
        println!(
            "agenterm-cc-web-direct-wry (experimental)\n\nUSAGE:\n  agenterm-cc-web-direct-wry --probe\n  agenterm-cc-web-direct-wry [--smoke] [--no-activate]"
        );
        return ExitCode::SUCCESS;
    }
    if args
        .iter()
        .any(|arg| !matches!(arg.as_str(), "--probe" | "--smoke" | "--no-activate"))
    {
        eprintln!("unsupported argument; use --help");
        return ExitCode::from(64);
    }
    let no_activate = env::var_os("AGENTERM_NO_ACTIVATE").is_some()
        || args.iter().any(|arg| arg == "--no-activate");
    let runtime_version = match wry::webview_version() {
        Ok(version) => version,
        Err(error) => {
            return print_receipt(
                unavailable(no_activate, format!("system runtime unavailable: {error}")),
                ExitCode::from(69),
            );
        }
    };
    if args.iter().any(|arg| arg == "--probe") {
        return print_receipt(
            HostReceipt {
                schema: CONTRACT_VERSION,
                implementation: "direct-wry",
                status: "available",
                reason: "system runtime version query succeeded".into(),
                runtime: platform_runtime(),
                runtime_version: Some(runtime_version),
                packaged_assets: ASSET_VERSION,
                local_url: LOCAL_URL,
                bridge: "absent",
                active_renderer: "experimental-direct-wry",
                no_activate,
                load_complete_ms: None,
            },
            ExitCode::SUCCESS,
        );
    }
    run_host(
        runtime_version,
        no_activate,
        args.iter().any(|arg| arg == "--smoke"),
    )
}

fn run_host(runtime_version: String, no_activate: bool, smoke: bool) -> ExitCode {
    let started = Instant::now();
    let event_loop = EventLoopBuilder::<HostEvent>::with_user_event().build();
    let proxy = event_loop.create_proxy();
    let window = match WindowBuilder::new()
        .with_title("AgenTerm Cockpit — experimental system WebView")
        .with_inner_size(LogicalSize::new(980.0, 680.0))
        .with_visible(false)
        .with_focused(false)
        .build(&event_loop)
    {
        Ok(window) => window,
        Err(error) => {
            return print_receipt(
                unavailable(
                    no_activate,
                    format!("native host window unavailable: {error}"),
                ),
                ExitCode::from(69),
            );
        }
    };

    let builder = WebViewBuilder::new()
        .with_custom_protocol("agenterm".into(), |_webview_id, request| {
            protocol_response(request.uri().path())
        })
        .with_navigation_handler(|url| is_allowed_navigation(&url))
        .with_new_window_req_handler(|_, _| NewWindowResponse::Deny)
        .with_download_started_handler(|_, _| false)
        .with_clipboard(false)
        .with_devtools(false)
        .with_hotkeys_zoom(false)
        .with_incognito(true)
        .with_focused(false)
        .with_on_page_load_handler(move |event, url| {
            if matches!(event, PageLoadEvent::Finished) && is_allowed_navigation(&url) {
                let _ = proxy.send_event(HostEvent::PageLoaded);
            }
        })
        .with_url(LOCAL_URL);

    #[cfg(not(target_os = "linux"))]
    let webview = builder.build(&window);

    #[cfg(target_os = "linux")]
    let webview = {
        use tao::platform::unix::WindowExtUnix;
        use wry::WebViewBuilderExtUnix;
        builder.build_gtk(window.gtk_window())
    };

    let _webview = match webview {
        Ok(webview) => webview,
        Err(error) => {
            return print_receipt(
                unavailable(
                    no_activate,
                    format!("system WebView creation failed: {error}"),
                ),
                ExitCode::from(69),
            );
        }
    };
    if !no_activate {
        window.set_visible(true);
    }

    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Wait;
        match event {
            Event::UserEvent(HostEvent::PageLoaded) if smoke => {
                print_receipt(
                    HostReceipt {
                        schema: CONTRACT_VERSION,
                        implementation: "direct-wry",
                        status: "loaded",
                        reason: "packaged Cockpit load completed".into(),
                        runtime: platform_runtime(),
                        runtime_version: Some(runtime_version.clone()),
                        packaged_assets: ASSET_VERSION,
                        local_url: LOCAL_URL,
                        bridge: "absent",
                        active_renderer: "experimental-direct-wry",
                        no_activate,
                        load_complete_ms: Some(started.elapsed().as_millis()),
                    },
                    ExitCode::SUCCESS,
                );
                *control_flow = ControlFlow::Exit;
            }
            Event::WindowEvent {
                event: WindowEvent::CloseRequested,
                ..
            } => *control_flow = ControlFlow::Exit,
            _ => {}
        }
    })
}

fn protocol_response(path: &str) -> Response<Cow<'static, [u8]>> {
    let asset =
        canonical_local_path(&format!("agenterm://localhost{path}")).and_then(asset_for_path);
    match asset {
        Some(asset) => Response::builder()
            .status(StatusCode::OK)
            .header("Content-Type", asset.media_type)
            .header("Content-Security-Policy", CONTENT_SECURITY_POLICY)
            .header("X-Content-Type-Options", "nosniff")
            .header("Referrer-Policy", "no-referrer")
            .header("Cache-Control", "no-store")
            .body(Cow::Borrowed(asset.bytes))
            .expect("static response headers are valid"),
        None => Response::builder()
            .status(StatusCode::NOT_FOUND)
            .header("Content-Type", "text/plain; charset=utf-8")
            .header("Content-Security-Policy", "default-src 'none'")
            .body(Cow::Borrowed(&b"not found"[..]))
            .expect("static response headers are valid"),
    }
}

fn unavailable(no_activate: bool, reason: String) -> HostReceipt {
    HostReceipt {
        schema: CONTRACT_VERSION,
        implementation: "direct-wry",
        status: "unavailable",
        reason,
        runtime: platform_runtime(),
        runtime_version: None,
        packaged_assets: ASSET_VERSION,
        local_url: LOCAL_URL,
        bridge: "absent",
        active_renderer: "native",
        no_activate,
        load_complete_ms: None,
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
