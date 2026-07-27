use std::{
    env,
    sync::{
        Arc,
        mpsc::{self, Receiver},
    },
    thread,
    time::Instant,
};

use anyhow::{Context as _, Result};
use rmux_pty::{
    ChildCommand, ProcessId, PtyChild, PtyMaster, TerminalSize, write_windows_console_mouse_drag,
};
use windows_sys::Win32::Foundation::HWND;

use crate::{
    SCROLLBACK_LINES, ipc_address, request_gui_wake,
    rmux_status::parse_status_windows,
    terminal_lifecycle::{BoundedByteRing, SubmissionState, TerminalLifecycle},
    terminal_observation::TerminalObservation,
    wake_signal::WakeSignal,
    working_context::{CwdTracker, ProxyState, SecretValue, ShellKind, parse_osc7},
    workspace::workspace_path,
};

const COMPOSER_SUBMIT_DELAY: std::time::Duration = std::time::Duration::from_millis(500);
const RAW_OUTPUT_LIMIT: usize = 1024 * 1024;
const PROXY_REDACTION_MAX_NEEDLES: usize = 8;
const PROXY_REDACTION_MAX_BYTES: usize = 32 * 1024;
const PROXY_REDACTION_MARKER: &[u8] = b"<redacted-proxy>";

pub(super) enum PtyEvent {
    Output(Vec<u8>),
    ReaderClosed,
    ReaderError(String),
    Exited(u32),
    ProcessError(String),
}

#[derive(Default)]
pub(super) struct TerminalCallbacks {
    pub(super) title: String,
    pub(super) pending_cwd: Option<String>,
    pub(super) local_hostname: Option<String>,
}

impl vt100::Callbacks for TerminalCallbacks {
    fn set_window_title(&mut self, _: &mut vt100::Screen, title: &[u8]) {
        self.title = String::from_utf8_lossy(title).trim().to_owned();
    }

    fn unhandled_osc(&mut self, _: &mut vt100::Screen, params: &[&[u8]]) {
        if let Some(path) = parse_osc7(params, self.local_hostname.as_deref()) {
            self.pending_cwd = Some(path);
        }
    }
}

pub(super) struct TerminalTab {
    pub(super) id: u64,
    pub(super) index: u32,
    pub(super) parent_id: Option<u64>,
    pub(super) title: String,
    pub(super) note: String,
    pub(super) command_name: String,
    pub(super) command_line: Vec<String>,
    pub(super) environment_names: Vec<String>,
    pub(super) process_id: Option<u32>,
    pub(super) shell_kind: ShellKind,
    pub(super) cwd: CwdTracker,
    pub(super) proxy: ProxyState,
    pub(super) composer: String,
    pub(super) sensitive_composer: Option<SecretValue>,
    pub(super) proxy_redaction_needles: Vec<SecretValue>,
    pub(super) proxy_redaction_pending: Vec<u8>,
    pub(super) parser: vt100::Parser<TerminalCallbacks>,
    pub(super) receiver: Receiver<PtyEvent>,
    pub(super) master: PtyMaster,
    pub(super) child: PtyChild,
    pub(super) exited: Option<u32>,
    pub(super) error: Option<String>,
    pub(super) last_size: (u16, u16),
    pub(super) input_bytes: usize,
    pub(super) input_writes: usize,
    pub(super) submission: SubmissionState,
    pub(super) submission_enter_written: Option<bool>,
    pub(super) lifecycle: TerminalLifecycle,
    pub(super) output_bytes: usize,
    pub(super) raw_output: BoundedByteRing,
}

pub(super) struct TerminalLaunch {
    pub(super) id: u64,
    pub(super) index: u32,
    pub(super) parent_id: Option<u64>,
    pub(super) title: Option<String>,
    pub(super) command_line: Vec<String>,
    pub(super) tab_environment: Vec<(String, String)>,
    pub(super) session_name: String,
    pub(super) window: HWND,
    pub(super) wake_signal: Arc<WakeSignal>,
    pub(super) initial_size: TerminalSize,
}

fn redact_proxy_stream_chunk(
    pending: &mut Vec<u8>,
    needles: &[&[u8]],
    bytes: &[u8],
    flush: bool,
) -> Vec<u8> {
    if needles.is_empty() {
        return bytes.to_vec();
    }
    pending.extend_from_slice(bytes);
    let mut ordered = needles
        .iter()
        .copied()
        .filter(|needle| !needle.is_empty())
        .collect::<Vec<_>>();
    ordered.sort_by_key(|needle| std::cmp::Reverse(needle.len()));
    let mut output = Vec::new();
    let mut consumed = 0;
    while consumed < pending.len() {
        let remaining = &pending[consumed..];
        if !flush
            && ordered
                .iter()
                .any(|needle| needle.len() > remaining.len() && needle.starts_with(remaining))
        {
            break;
        }
        if let Some(needle) = ordered.iter().find(|needle| remaining.starts_with(needle)) {
            output.extend_from_slice(PROXY_REDACTION_MARKER);
            consumed += needle.len();
        } else {
            output.push(remaining[0]);
            consumed += 1;
        }
    }
    pending.drain(..consumed);
    output
}

impl TerminalTab {
    pub(super) fn observation(&self) -> TerminalObservation {
        let process_finished = self.exited.is_some() || self.error.is_some();
        TerminalObservation {
            process_id: self.process_id,
            exit_code: self.exited,
            error: self.error.clone(),
            input_bytes: self.input_bytes,
            input_writes: self.input_writes,
            output_bytes: self.output_bytes,
            submit_pending: self.submission.is_pending(),
            submission_enter_written: self.submission_enter_written,
            reader_closed: self.lifecycle.reader_closed(),
            parser_drained: self.lifecycle.parser_drained(),
            finalized: self.lifecycle.finalized(process_finished),
        }
    }

    pub(super) fn register_proxy_redactions(&mut self, values: &[&str]) -> Result<()> {
        let mut additions = Vec::new();
        for value in values.iter().copied().filter(|value| !value.is_empty()) {
            if !self
                .proxy_redaction_needles
                .iter()
                .any(|needle| needle.expose() == value)
                && !additions.contains(&value)
            {
                additions.push(value);
            }
        }
        let next_count = self.proxy_redaction_needles.len() + additions.len();
        let next_bytes = self
            .proxy_redaction_needles
            .iter()
            .map(|needle| needle.expose_bytes().len())
            .sum::<usize>()
            + additions.iter().map(|value| value.len()).sum::<usize>();
        if next_count > PROXY_REDACTION_MAX_NEEDLES || next_bytes > PROXY_REDACTION_MAX_BYTES {
            anyhow::bail!(
                "proxy redaction history is full; restart this tab before changing proxy again"
            );
        }
        for value in additions {
            self.proxy_redaction_needles
                .push(SecretValue::new(value.to_owned())?);
        }
        Ok(())
    }

    fn redact_proxy_output(&mut self, bytes: &[u8], flush: bool) -> Vec<u8> {
        let needles = self
            .proxy_redaction_needles
            .iter()
            .map(SecretValue::expose_bytes)
            .collect::<Vec<_>>();
        redact_proxy_stream_chunk(&mut self.proxy_redaction_pending, &needles, bytes, flush)
    }

    fn finish_output_stream(&mut self) {
        if self.lifecycle.reader_closed() {
            return;
        }
        let tail = self.redact_proxy_output(&[], true);
        self.output_bytes += tail.len();
        self.parser.process(&tail);
        self.raw_output.extend(&tail);
        self.lifecycle.close_reader_after_parser_drain();
    }

    pub(super) fn spawn(launch: TerminalLaunch) -> Result<Self> {
        let TerminalLaunch {
            id,
            index,
            parent_id,
            title,
            command_line,
            tab_environment,
            session_name,
            window,
            wake_signal,
            initial_size,
        } = launch;
        let program = command_line.first().cloned().unwrap_or_else(|| {
            env::var("COMSPEC").unwrap_or_else(|_| r"C:\Windows\System32\cmd.exe".to_owned())
        });
        let persisted_command_line = if command_line.is_empty() {
            vec![program.clone()]
        } else {
            command_line.clone()
        };
        // Resolve once: this exact value both configures the child and becomes
        // the truthful launch observation shown by the host.
        let launch_cwd = env::current_dir().ok();
        let mut command = ChildCommand::new(&program)
            .size(initial_size)
            .env("TERM", "xterm-256color")
            .env("COLORTERM", "truecolor")
            .env("TERM_PROGRAM", "AgenTerm")
            .env("TERM_PROGRAM_VERSION", env!("CARGO_PKG_VERSION"))
            .env("AGENTERM_IPC_ADDRESS", ipc_address())
            .env("AGENTERM_TAB_ID", format!("@{id}"))
            .env("AGENTERM_SESSION", session_name)
            .env("AGENTERM_WORKSPACE_PATH", workspace_path());
        let proxy = ProxyState::from_environment(&tab_environment)?;
        let mut proxy_redaction_needles = Vec::new();
        for value in [proxy.http(), proxy.https()].into_iter().flatten() {
            if !proxy_redaction_needles
                .iter()
                .any(|needle: &SecretValue| needle.expose() == value)
            {
                proxy_redaction_needles.push(SecretValue::new(value.to_owned())?);
            }
        }
        let environment_names = tab_environment
            .iter()
            .map(|(name, _)| name.clone())
            .collect();
        for (name, value) in tab_environment {
            command = command.env(name, value);
        }
        for argument in command_line.iter().skip(1) {
            command = command.arg(argument);
        }
        if let Some(directory) = &launch_cwd {
            command = command.current_dir(directory);
        }

        let spawned = command
            .spawn()
            .context("failed to start terminal command")?;
        let (mut master, child) = spawned.into_parts();
        let process_id = Some(child.pid().as_u32());
        let reader = master
            .try_clone_for_startup_reader()
            .context("failed to clone ConPTY reader")?;
        let mut wait_child = child
            .try_clone_for_wait()
            .context("failed to clone ConPTY child wait handle")?;
        let (sender, receiver) = mpsc::channel();
        let wake_window = window as isize;

        let output_sender = sender.clone();
        let output_wake_signal = Arc::clone(&wake_signal);
        thread::spawn(move || {
            let mut buffer = [0_u8; 16 * 1024];
            loop {
                match reader.io().read(&mut buffer) {
                    Ok(0) => {
                        if output_sender.send(PtyEvent::ReaderClosed).is_ok() {
                            request_gui_wake(wake_window, &output_wake_signal);
                        }
                        break;
                    }
                    Ok(size) => {
                        if output_sender
                            .send(PtyEvent::Output(buffer[..size].to_vec()))
                            .is_err()
                        {
                            break;
                        }
                        request_gui_wake(wake_window, &output_wake_signal);
                    }
                    Err(error) => {
                        if output_sender
                            .send(PtyEvent::ReaderError(error.to_string()))
                            .is_ok()
                        {
                            request_gui_wake(wake_window, &output_wake_signal);
                        }
                        break;
                    }
                }
            }
        });

        let wait_wake_signal = wake_signal;
        thread::spawn(move || {
            let event = match wait_child.wait() {
                Ok(status) => {
                    wait_child.close_pseudoconsole();
                    PtyEvent::Exited(status.code().unwrap_or(1) as u32)
                }
                Err(error) => PtyEvent::ProcessError(format!("process wait failed: {error}")),
            };
            if sender.send(event).is_ok() {
                request_gui_wake(wake_window, &wait_wake_signal);
            }
        });

        let command_name = std::path::Path::new(&program)
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or(&program)
            .to_owned();
        Ok(Self {
            id,
            index,
            parent_id,
            title: title.unwrap_or_else(|| command_name.clone()),
            command_name,
            command_line: persisted_command_line,
            environment_names,
            process_id,
            shell_kind: ShellKind::from_program(&program),
            cwd: CwdTracker::launch(
                launch_cwd.and_then(|path| path.into_os_string().into_string().ok()),
            ),
            proxy,
            composer: String::new(),
            sensitive_composer: None,
            proxy_redaction_needles,
            proxy_redaction_pending: Vec::new(),
            note: String::new(),
            parser: vt100::Parser::new_with_callbacks(
                initial_size.rows,
                initial_size.cols,
                SCROLLBACK_LINES,
                TerminalCallbacks {
                    local_hostname: env::var("COMPUTERNAME").ok(),
                    ..TerminalCallbacks::default()
                },
            ),
            receiver,
            master,
            child,
            exited: None,
            error: None,
            last_size: (initial_size.rows, initial_size.cols),
            input_bytes: 0,
            input_writes: 0,
            submission: SubmissionState::default(),
            submission_enter_written: None,
            lifecycle: TerminalLifecycle::default(),
            output_bytes: 0,
            raw_output: BoundedByteRing::new(RAW_OUTPUT_LIMIT),
        })
    }

    pub(super) fn poll(&mut self) -> bool {
        let mut changed = false;
        while let Ok(event) = self.receiver.try_recv() {
            changed = true;
            match event {
                PtyEvent::Output(bytes) => {
                    if self.lifecycle.reader_closed() {
                        self.error = Some(
                            "terminal output arrived after the reader was finalized".to_owned(),
                        );
                        continue;
                    }
                    let bytes = self.redact_proxy_output(&bytes, false);
                    self.output_bytes += bytes.len();
                    self.parser.process(&bytes);
                    if let Some(path) = self.parser.callbacks_mut().pending_cwd.take() {
                        self.cwd.confirm_osc7(path);
                    }
                    self.raw_output.extend(&bytes);
                }
                PtyEvent::ReaderClosed => {
                    self.finish_output_stream();
                }
                PtyEvent::ReaderError(error) => {
                    self.finish_output_stream();
                    self.error = Some(format!("terminal output failed: {error}"));
                }
                PtyEvent::Exited(code) => {
                    self.exited = Some(code);
                }
                PtyEvent::ProcessError(error) => {
                    self.error = Some(error);
                }
            }
        }
        if self.submission.take_if_due(Instant::now()) {
            self.submission_enter_written = Some(self.send(b"\r"));
            changed = true;
        }
        changed
    }

    pub(super) fn resize(&mut self, rows: u16, cols: u16) {
        if self.last_size == (rows, cols) {
            return;
        }
        self.last_size = (rows, cols);
        self.parser.screen_mut().set_size(rows, cols);
        if let Err(error) = self.master.resize(TerminalSize { rows, cols }) {
            self.error = Some(format!("resize failed: {error}"));
        }
    }

    pub(super) fn scroll_viewport(&mut self, action: &str, count: Option<usize>) -> Result<usize> {
        let page = usize::from(self.last_size.0.saturating_sub(1)).max(1);
        let screen = self.parser.screen_mut();
        let current = screen.scrollback();
        let requested = match action {
            "up" => current.saturating_add(count.unwrap_or(1)),
            "down" => current.saturating_sub(count.unwrap_or(1)),
            "page-up" => current.saturating_add(count.unwrap_or(page)),
            "page-down" => current.saturating_sub(count.unwrap_or(page)),
            "top" if count.is_none() => usize::MAX,
            "bottom" if count.is_none() => 0,
            "top" | "bottom" => anyhow::bail!("{action} does not accept a row count"),
            _ => anyhow::bail!(
                "usage: scroll-pane [-t target] up|down|page-up|page-down|top|bottom [rows]"
            ),
        };
        screen.set_scrollback(requested);
        Ok(screen.scrollback())
    }

    pub(super) fn scrollback_bounds(&mut self) -> (usize, usize) {
        let screen = self.parser.screen_mut();
        let current = screen.scrollback();
        screen.set_scrollback(usize::MAX);
        let maximum = screen.scrollback();
        screen.set_scrollback(current);
        (screen.scrollback(), maximum)
    }

    pub(super) fn set_scrollback(&mut self, offset: usize) -> usize {
        let screen = self.parser.screen_mut();
        screen.set_scrollback(offset);
        screen.scrollback()
    }

    pub(super) fn send(&mut self, bytes: &[u8]) -> bool {
        if self.exited.is_some() || self.error.is_some() || self.lifecycle.reader_closed() {
            return false;
        }
        if let Err(error) = self.master.write_all(bytes) {
            self.error = Some(format!("input failed: {error}"));
            false
        } else {
            self.input_bytes += bytes.len();
            self.input_writes += 1;
            true
        }
    }

    pub(super) fn submit(&mut self, text: &str) -> bool {
        if self.submission.is_pending() || !self.send(text.as_bytes()) {
            return false;
        }
        // Interactive TUIs classify rapid character streams as paste and
        // temporarily reinterpret Enter as a pasted newline. Schedule Enter
        // after that suppression window without blocking the GUI thread.
        self.submission_enter_written = None;
        self.submission
            .schedule(Instant::now(), COMPOSER_SUBMIT_DELAY)
    }

    pub(super) fn submit_sensitive(&mut self, text: &str) -> bool {
        if self.submission.is_pending()
            || self.exited.is_some()
            || self.error.is_some()
            || self.lifecycle.reader_closed()
        {
            return false;
        }
        if let Err(error) = self.master.write_all(text.as_bytes()) {
            self.error = Some(format!("sensitive input failed: {error}"));
            return false;
        }
        self.input_bytes += PROXY_REDACTION_MARKER.len();
        self.input_writes += 1;
        self.submission_enter_written = None;
        self.submission
            .schedule(Instant::now(), COMPOSER_SUBMIT_DELAY)
    }

    pub(super) fn send_native_mouse_click(&mut self, x: u16, y: u16) -> Result<()> {
        if self.exited.is_some() {
            anyhow::bail!("process has exited");
        }
        let raw_pid = self.process_id.context("process id is unavailable")?;
        let process_id = ProcessId::new(raw_pid).context("invalid process id")?;
        let x = i16::try_from(x).context("mouse x coordinate is too large")?;
        let y = i16::try_from(y).context("mouse y coordinate is too large")?;
        write_windows_console_mouse_drag(process_id, x, y, x, y)
            .context("WriteConsoleInputW click failed")?;
        self.input_bytes += 3;
        Ok(())
    }

    pub(super) fn send_rmux_status_click(&mut self, x: u16, y: u16) -> bool {
        let (rows, cols) = self.last_size;
        if y >= rows {
            return false;
        }
        let (target, active, mut windows) = {
            let screen = self.parser.screen();
            let mut target = None;
            let mut active = None;
            let mut windows = Vec::new();
            for row in 0..rows {
                let mut line = String::with_capacity(cols as usize);
                for col in 0..cols {
                    let text = screen
                        .cell(row, col)
                        .map(|cell| cell.contents())
                        .unwrap_or(" ");
                    line.push(if text.is_empty() {
                        ' '
                    } else {
                        text.chars().next().unwrap_or(' ')
                    });
                }
                for status_window in parse_status_windows(&line) {
                    if status_window.active {
                        active = Some(status_window.index);
                    }
                    if row == y
                        && usize::from(x) >= status_window.start
                        && usize::from(x) < status_window.end
                    {
                        target = Some(status_window.index);
                    }
                    if !windows.contains(&status_window.index) {
                        windows.push(status_window.index);
                    }
                }
            }
            (target, active, windows)
        };
        let (Some(target), Some(active)) = (target, active) else {
            return false;
        };
        if target == active {
            return true;
        }
        windows.sort_unstable();
        let Some(active_position) = windows.iter().position(|index| *index == active) else {
            return false;
        };
        let Some(target_position) = windows.iter().position(|index| *index == target) else {
            return false;
        };
        let forward = (target_position + windows.len() - active_position) % windows.len();
        let backward = (active_position + windows.len() - target_position) % windows.len();
        let (key, repeats) = if forward <= backward {
            (b"\x1bOS".as_slice(), forward)
        } else {
            (b"\x1bOR".as_slice(), backward)
        };
        for _ in 0..repeats {
            self.send(key);
        }
        true
    }

    pub(super) fn close_process(&mut self) {
        if self.exited.is_none() {
            let _ = self.child.terminate_forcefully();
        }
    }
}

impl Drop for TerminalTab {
    fn drop(&mut self) {
        self.close_process();
    }
}

#[cfg(test)]
mod tests {
    use super::redact_proxy_stream_chunk;

    #[test]
    fn proxy_redaction_matches_longest_and_survives_every_chunk_boundary() {
        let input = b"before https://user:secret@proxy.example/path after";
        let long = b"https://user:secret@proxy.example/path".as_slice();
        let short = b"user:secret".as_slice();
        for split in 0..=input.len() {
            let mut pending = Vec::new();
            let mut output =
                redact_proxy_stream_chunk(&mut pending, &[short, long], &input[..split], false);
            output.extend(redact_proxy_stream_chunk(
                &mut pending,
                &[short, long],
                &input[split..],
                false,
            ));
            output.extend(redact_proxy_stream_chunk(
                &mut pending,
                &[short, long],
                &[],
                true,
            ));
            let output = String::from_utf8(output).unwrap();
            assert_eq!(output, "before <redacted-proxy> after");
        }
    }

    #[test]
    fn proxy_redaction_retains_historical_values_in_byte_sized_streams() {
        let old = b"http://old-secret@old.example".as_slice();
        let new = b"https://new-secret@new.example".as_slice();
        let input = b"http://old-secret@old.example https://new-secret@new.example";
        let mut pending = Vec::new();
        let mut output = Vec::new();
        for byte in input {
            output.extend(redact_proxy_stream_chunk(
                &mut pending,
                &[old, new],
                std::slice::from_ref(byte),
                false,
            ));
        }
        output.extend(redact_proxy_stream_chunk(
            &mut pending,
            &[old, new],
            &[],
            true,
        ));
        assert_eq!(
            String::from_utf8(output).unwrap(),
            "<redacted-proxy> <redacted-proxy>"
        );
    }

    #[test]
    fn proxy_redaction_flushes_unrelated_short_output_without_waiting_for_long_secrets() {
        let long = vec![b'x'; 4096];
        let mut pending = Vec::new();
        let output = redact_proxy_stream_chunk(&mut pending, &[long.as_slice()], b"prompt>", false);
        assert_eq!(output, b"prompt>");
        assert!(pending.is_empty());
    }

    #[test]
    fn proxy_redaction_hides_secret_length_behind_a_fixed_marker() {
        let short = b"http://u:a@proxy".as_slice();
        let long = b"http://user:a-much-longer-password@proxy".as_slice();
        for needle in [short, long] {
            let mut pending = Vec::new();
            let mut output = redact_proxy_stream_chunk(&mut pending, &[needle], needle, false);
            output.extend(redact_proxy_stream_chunk(
                &mut pending,
                &[needle],
                &[],
                true,
            ));
            assert_eq!(output, b"<redacted-proxy>");
        }
    }
}
