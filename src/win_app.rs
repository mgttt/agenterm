use std::{env, fs::OpenOptions, io::Write, mem};

use anyhow::{Context as _, Result};
use windows_sys::Win32::{
    Foundation::{HWND, INVALID_HANDLE_VALUE, RECT},
    Graphics::Gdi::{
        BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BitBlt, CreateCompatibleBitmap, CreateCompatibleDC,
        DIB_RGB_COLORS, DeleteDC, DeleteObject, GetDC, GetDIBits, GetWindowDC, HGDIOBJ, ReleaseDC,
        SRCCOPY, SelectObject,
    },
    System::Console::{
        ATTACH_PARENT_PROCESS, AttachConsole, FreeConsole, GetStdHandle, STD_ERROR_HANDLE,
    },
    UI::WindowsAndMessaging::{GetClientRect, GetWindowRect, PostMessageW, WM_APP},
};

use crate::{ui_geometry::PixelRect, wake_signal::WakeSignal};

const WM_APP_WAKE: u32 = WM_APP + 1;

/// Wake the Win32 message loop without posting one message per producer event.
pub(crate) fn request_gui_wake(wake_window: isize, wake_signal: &WakeSignal) {
    if wake_signal.request() {
        unsafe {
            PostMessageW(wake_window as HWND, WM_APP_WAKE, 0, 0);
        }
    }
}

/// Windows-subsystem launcher entry point.
///
/// The GUI owns only HWND/layout/render/input state. Session, tab, PTY and
/// event truth live in the independently replaceable server process.
pub fn run_gui_entry() -> i32 {
    let arguments = env::args().skip(1).collect::<Vec<_>>();
    if arguments
        .first()
        .is_some_and(|argument| !argument.starts_with("--"))
    {
        write_best_effort_stderr(&gui_cli_guidance(&arguments));
        return 2;
    }
    let launch_options = match configure_gui_launch(&arguments) {
        Ok(options) => options,
        Err(error) => {
            write_best_effort_stderr(&format!(
                "AgenTerm GUI argument error: {error:#}\n\
                 No GUI server was started by this invocation.\n\
                 More CLI commands: agenterm-cli.exe -h"
            ));
            return 2;
        }
    };
    let no_activate = launch_options.no_activate || crate::client::no_activate_from_environment();
    write_best_effort_stderr(&gui_console_summary(&crate::ipc_address()));

    // Preserve the historical launcher handoff when a compatible UI already
    // owns this server. A headless server explicitly asks us to create the
    // replaceable client instead.
    if env::var_os("AGENTERM_SERVER").is_none() && !launch_options.ui_client {
        let handoff = if no_activate {
            "__show-no-activate"
        } else {
            "__focus"
        };
        match crate::client::send_ipc_request(vec![handoff.to_owned()]) {
            Ok(response) if response.ok => return 0,
            Ok(response) if response.error_code == "ui_client_unavailable" => {}
            Ok(response) => {
                write_best_effort_stderr(&format!(
                    "The running AgenTerm server rejected the launcher handoff: {}\n\
                     Restart that server to use this launcher capability.",
                    response.error
                ));
                return 1;
            }
            Err(_) => {}
        }
    }

    if let Err(error) = crate::remote_win_app::run_remote_gui(no_activate) {
        show_startup_error(&error);
        return 1;
    }
    0
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct GuiLaunchOptions {
    no_activate: bool,
    ui_client: bool,
}

fn configure_gui_launch(arguments: &[String]) -> Result<GuiLaunchOptions> {
    let (options, address) = parse_gui_launch(arguments)?;
    crate::client::set_ipc_address_override(address);
    Ok(options)
}

fn parse_gui_launch(arguments: &[String]) -> Result<(GuiLaunchOptions, Option<String>)> {
    let mut options = GuiLaunchOptions::default();
    let mut address = None;
    let mut position = 0;
    while position < arguments.len() {
        match arguments[position].as_str() {
            "--no-activate" | "--not-foreground" => {
                if options.no_activate {
                    anyhow::bail!(
                        "agenterm.exe --no-activate/--not-foreground may be specified only once"
                    );
                }
                options.no_activate = true;
                position += 1;
            }
            "--ui-client" => {
                if options.ui_client {
                    anyhow::bail!("agenterm.exe --ui-client may be specified only once");
                }
                options.ui_client = true;
                position += 1;
            }
            "--address" => {
                if address.is_some() {
                    anyhow::bail!("agenterm.exe --address may be specified only once");
                }
                let value = arguments
                    .get(position + 1)
                    .context("agenterm.exe --address requires HOST:PORT")?;
                if value.starts_with("--") {
                    anyhow::bail!("agenterm.exe --address requires HOST:PORT");
                }
                crate::client::parse_loopback_ipc_address(value)?;
                address = Some(value.clone());
                position += 2;
            }
            argument => {
                anyhow::bail!("unsupported AgenTerm GUI argument: {argument}")
            }
        }
    }
    Ok((options, address))
}

fn quote_argument_for_display(argument: &str) -> String {
    if argument.is_empty() || argument.chars().any(char::is_whitespace) {
        format!("\"{}\"", argument.replace('"', "\\\""))
    } else {
        argument.to_owned()
    }
}

fn gui_cli_guidance(arguments: &[String]) -> String {
    let forwarded = arguments
        .iter()
        .map(|argument| quote_argument_for_display(argument))
        .collect::<Vec<_>>()
        .join(" ");
    format!(
        "AgenTerm GUI entry point\n\n\
         No CLI command was executed and no GUI server was started by this invocation.\n\n\
         Use instead:\nagenterm-cli.exe {forwarded}\n\n\
         Launcher PID: {}\nConfigured server address: {}\n\n\
         List running server PID and port: agenterm-cli.exe server-list\n\
         More CLI commands: agenterm-cli.exe -h",
        std::process::id(),
        crate::ipc_address()
    )
}

fn gui_console_summary(address: &str) -> String {
    format!(
        "Launcher PID: {}\n\
         Configured server address: {address}\n\n\
         List running server PID and port: agenterm-cli.exe server-list\n\
         More CLI commands: agenterm-cli.exe -h",
        std::process::id()
    )
}

fn write_best_effort_stderr(message: &str) {
    let payload = format!("{message}\n");
    let stderr_handle = unsafe { GetStdHandle(STD_ERROR_HANDLE) };
    if !stderr_handle.is_null() && stderr_handle != INVALID_HANDLE_VALUE {
        let mut stderr = std::io::stderr().lock();
        if stderr.write_all(payload.as_bytes()).is_ok() && stderr.flush().is_ok() {
            return;
        }
    }

    // A /SUBSYSTEM:WINDOWS process normally has no standard handles. Attach
    // only to an existing parent console, never allocate or read one.
    if unsafe { AttachConsole(ATTACH_PARENT_PROCESS) } == 0 {
        return;
    }
    if let Ok(mut console) = OpenOptions::new().write(true).open("CONOUT$") {
        let _ = console.write_all(payload.as_bytes());
        let _ = console.flush();
    }
    unsafe {
        FreeConsole();
    }
}

fn show_startup_error(error: &anyhow::Error) {
    write_best_effort_stderr(&format!("AgenTerm failed to start:\n\n{error:#}"));
}

pub(crate) fn save_window_png(
    window: HWND,
    path: &std::path::Path,
    pane: Option<PixelRect>,
) -> Result<()> {
    let mut client: RECT = unsafe { mem::zeroed() };
    let mut outer: RECT = unsafe { mem::zeroed() };
    unsafe {
        GetClientRect(window, &mut client);
        GetWindowRect(window, &mut outer);
    }
    let (source, source_x, source_y, width, height) = if let Some(pane) = pane {
        (
            unsafe { GetDC(window) },
            pane.left,
            pane.top,
            pane.width().max(1),
            pane.height().max(1),
        )
    } else {
        (
            unsafe { GetWindowDC(window) },
            0,
            0,
            (outer.right - outer.left).max(1),
            (outer.bottom - outer.top).max(1),
        )
    };
    if source.is_null() {
        anyhow::bail!("failed to acquire window device context");
    }
    let memory_dc = unsafe { CreateCompatibleDC(source) };
    let bitmap = unsafe { CreateCompatibleBitmap(source, width, height) };
    if memory_dc.is_null() || bitmap.is_null() {
        if !memory_dc.is_null() {
            unsafe { DeleteDC(memory_dc) };
        }
        unsafe { ReleaseDC(window, source) };
        anyhow::bail!("failed to allocate screenshot bitmap");
    }

    let previous = unsafe { SelectObject(memory_dc, bitmap as HGDIOBJ) };
    let copied = unsafe {
        BitBlt(
            memory_dc, 0, 0, width, height, source, source_x, source_y, SRCCOPY,
        )
    };
    let mut info: BITMAPINFO = unsafe { mem::zeroed() };
    info.bmiHeader = BITMAPINFOHEADER {
        biSize: mem::size_of::<BITMAPINFOHEADER>() as u32,
        biWidth: width,
        biHeight: -height,
        biPlanes: 1,
        biBitCount: 32,
        biCompression: BI_RGB,
        ..unsafe { mem::zeroed() }
    };
    let mut bgra = vec![0_u8; width as usize * height as usize * 4];
    let scanlines = if copied != 0 {
        unsafe {
            GetDIBits(
                memory_dc,
                bitmap,
                0,
                height as u32,
                bgra.as_mut_ptr().cast(),
                &mut info,
                DIB_RGB_COLORS,
            )
        }
    } else {
        0
    };
    unsafe {
        SelectObject(memory_dc, previous);
        DeleteObject(bitmap as HGDIOBJ);
        DeleteDC(memory_dc);
        ReleaseDC(window, source);
    }
    if copied == 0 || scanlines == 0 {
        anyhow::bail!("BitBlt/GetDIBits failed while capturing the window");
    }

    let mut rgba = Vec::with_capacity(bgra.len());
    for pixel in bgra.chunks_exact(4) {
        rgba.extend_from_slice(&[pixel[2], pixel[1], pixel[0], 255]);
    }
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    let file = std::fs::File::create(path)
        .with_context(|| format!("failed to create {}", path.display()))?;
    let mut encoder = png::Encoder::new(file, width as u32, height as u32);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder
        .write_header()
        .context("failed to start PNG encoder")?;
    writer
        .write_image_data(&rgba)
        .context("failed to write PNG pixels")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{gui_cli_guidance, parse_gui_launch};

    #[test]
    fn gui_cli_guidance_preserves_arguments_and_names_the_real_cli() {
        let guidance = gui_cli_guidance(&[
            "list-windows".to_owned(),
            "-F".to_owned(),
            "#{window_id} #{window_name}".to_owned(),
        ]);
        assert!(guidance.contains("No CLI command was executed"));
        assert!(
            guidance.contains("agenterm-cli.exe list-windows -F \"#{window_id} #{window_name}\"")
        );
        assert!(guidance.contains("Launcher PID:"));
        assert!(guidance.contains("Configured server address:"));
        assert!(guidance.contains("agenterm-cli.exe server-list"));
        assert!(guidance.contains("agenterm-cli.exe -h"));
    }

    #[test]
    fn gui_launcher_accepts_no_activate_and_address_in_either_order() {
        let (options, address) = parse_gui_launch(&[
            "--no-activate".to_owned(),
            "--address".to_owned(),
            "127.0.0.1:48815".to_owned(),
        ])
        .unwrap();
        assert!(options.no_activate);
        assert!(!options.ui_client);
        assert_eq!(address.as_deref(), Some("127.0.0.1:48815"));

        let (options, address) = parse_gui_launch(&[
            "--address".to_owned(),
            "127.0.0.1:48816".to_owned(),
            "--not-foreground".to_owned(),
        ])
        .unwrap();
        assert!(options.no_activate);
        assert!(!options.ui_client);
        assert_eq!(address.as_deref(), Some("127.0.0.1:48816"));

        let (options, address) = parse_gui_launch(&[
            "--ui-client".to_owned(),
            "--address".to_owned(),
            "127.0.0.1:48817".to_owned(),
            "--no-activate".to_owned(),
        ])
        .unwrap();
        assert!(options.ui_client);
        assert!(options.no_activate);
        assert_eq!(address.as_deref(), Some("127.0.0.1:48817"));
    }

    #[test]
    fn gui_launcher_rejects_duplicate_unknown_and_missing_options() {
        for arguments in [
            vec!["--no-activate", "--no-activate"],
            vec!["--no-activate", "--not-foreground"],
            vec!["--not-foreground", "--not-foreground"],
            vec!["--ui-client", "--ui-client"],
            vec![
                "--address",
                "127.0.0.1:48815",
                "--address",
                "127.0.0.1:48816",
            ],
            vec!["--address"],
            vec!["--address", "--no-activate"],
            vec!["--unknown"],
        ] {
            assert!(
                parse_gui_launch(&arguments.into_iter().map(str::to_owned).collect::<Vec<_>>())
                    .is_err()
            );
        }
    }
}
