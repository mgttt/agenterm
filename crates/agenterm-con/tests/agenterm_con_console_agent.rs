//! The pre-ConPTY terminal backend, driven end to end.
//!
//! Windows Server 2016 has no pseudoconsole, so `agenterm-con` falls back to
//! hosting the child in a hidden console and scraping its screen buffer. That
//! path can only be reached on a system old enough to lack ConPTY — which is
//! neither CI nor any developer's machine — and a backend nobody can run is a
//! backend nobody notices breaking. `AGENTERM_FORCE_CONSOLE_AGENT=1` selects
//! it on a modern system so these journeys exercise the real thing.
//!
//! Everything asserted here is asserted through the ordinary control surface,
//! because the point of the backend is that nothing above the adapter can
//! tell which one it got.

#![cfg(windows)]

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

/// Real GUI sessions get flakier the more of them race, so they take turns.
fn gui_guard() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn binary() -> PathBuf {
    let mut path = std::env::current_exe().expect("test executable path");
    path.pop();
    path.pop();
    path.push("agenterm-con.exe");
    assert!(
        path.is_file(),
        "agenterm-con is missing at {}; build it with \
         `cargo build -p agenterm-con --bin agenterm-con`",
        path.display()
    );
    path
}

fn scratch(label: &str) -> PathBuf {
    let mut path = std::env::temp_dir();
    path.push(format!(
        "agenterm-con-console-agent-{label}-{}-{}",
        std::process::id(),
        Instant::now().elapsed().as_nanos()
    ));
    let _ = std::fs::remove_dir_all(&path);
    std::fs::create_dir_all(&path).expect("scratch directory");
    path
}

struct Session {
    child: Child,
    endpoint: String,
}

impl Session {
    /// Starts a host pinned to the console-agent backend.
    fn start(label: &str) -> Self {
        let directory = scratch(label);
        let endpoint = format!(
            r"pipe:\\.\pipe\agenterm-con-agent-{}-{label}",
            std::process::id()
        );
        let child = Command::new(binary())
            .arg("--no-activate")
            .arg("--control")
            .arg(&endpoint)
            .env("AGENTERM_FORCE_CONSOLE_AGENT", "1")
            .current_dir(&directory)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn agenterm-con");
        let session = Self { child, endpoint };
        session.wait_ready();
        session
    }

    /// The control endpoint is not listening the instant the process exists.
    /// Waiting for it here keeps every later failure meaningful: a refused
    /// connection then means the host died, not that it had not started.
    fn wait_ready(&self) {
        let deadline = Instant::now() + Duration::from_secs(30);
        let mut last = String::new();
        while Instant::now() < deadline {
            match self.try_control(&["list-tabs"]) {
                Ok(_) => return,
                Err(error) => last = error,
            }
            std::thread::sleep(Duration::from_millis(200));
        }
        panic!("control endpoint never came up: {last}\ndiagnostics:\n{}", diagnostics());
    }

    fn try_control(&self, arguments: &[&str]) -> Result<String, String> {
        let output = Command::new(binary())
            .arg("cli")
            .arg("--control")
            .arg(&self.endpoint)
            .args(arguments)
            .output()
            .map_err(|error| error.to_string())?;
        if !output.status.success() {
            return Err(String::from_utf8_lossy(&output.stderr).into_owned());
        }
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    }

    fn control(&self, arguments: &[&str]) -> String {
        self.try_control(arguments).unwrap_or_else(|error| {
            panic!(
                "control {arguments:?} failed: {error}\ndiagnostics:\n{}",
                diagnostics()
            )
        })
    }

    /// Polls the pane until `needle` shows up, so a slow child is waited for
    /// rather than raced. The console backend polls its buffer, so output is
    /// never instantaneous the way a pipe read is.
    fn wait_for_pane(&self, needle: &str, timeout: Duration) -> String {
        let deadline = Instant::now() + timeout;
        let mut last = String::new();
        while Instant::now() < deadline {
            last = self.control(&["capture-pane", "--max-bytes", "8000"]);
            if last.contains(needle) {
                return last;
            }
            std::thread::sleep(Duration::from_millis(150));
        }
        panic!("{needle:?} never appeared. Last pane:\n{last}");
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        let _ = Command::new(binary())
            .arg("cli")
            .arg("--control")
            .arg(&self.endpoint)
            .arg("close-window")
            .output();
        std::thread::sleep(Duration::from_millis(400));
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Reads whatever the host recorded about its own failures, so a broken
/// backend reports why instead of only that.
fn diagnostics() -> String {
    let Some(path) = agenterm_platform::diagnostics::log_path() else {
        return String::new();
    };
    std::fs::read_to_string(path).unwrap_or_default()
}

/// The whole point: a shell runs, and its banner and prompt reach the pane.
/// On a system without ConPTY this is the difference between a terminal and
/// an error message.
#[test]
fn a_shell_starts_and_paints_through_the_console_agent() {
    let _guard = gui_guard();
    let session = Session::start("startup");
    let tabs = {
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            let tabs = session.control(&["list-tabs"]);
            if tabs.contains("\"child_alive\": true") || Instant::now() >= deadline {
                break tabs;
            }
            std::thread::sleep(Duration::from_millis(200));
        }
    };
    assert!(
        tabs.contains("\"child_alive\": true"),
        "the agent did not bring up a live child.\ntabs: {tabs}\ndiagnostics:\n{}",
        diagnostics()
    );
    session.wait_for_pane("Microsoft Windows", Duration::from_secs(20));
}

/// Keystrokes have to survive the trip back: the host writes terminal bytes,
/// and the agent turns them into console input records. A backend that only
/// paints is a log viewer.
#[test]
fn typed_input_reaches_the_child_and_its_output_comes_back() {
    let _guard = gui_guard();
    let session = Session::start("input");
    session.wait_for_pane("Microsoft Windows", Duration::from_secs(20));
    // The child has to *compute* the marker. Asserting on echoed text would
    // also pass if the agent merely reflected keystrokes without a shell
    // behind them, and counting occurrences of one string does not work
    // either: a long prompt wraps the typed line and splits it in half.
    session.control(&["send-text", "set /a 123456+654321\r"]);
    let pane = session.wait_for_pane("777777", Duration::from_secs(20));
    assert!(
        !pane.contains("777777\r\n777777"),
        "the marker appeared twice, which means it was echoed rather than computed:\n{pane}"
    );
}

/// A resize used to kill the session outright: the console was resized from
/// the control thread while the poll thread was mid-read, the read failed,
/// and the agent treated the first failure as fatal. The session must survive
/// and still be usable afterwards.
#[test]
fn a_resize_does_not_end_the_session() {
    let _guard = gui_guard();
    let session = Session::start("resize");
    session.wait_for_pane("Microsoft Windows", Duration::from_secs(20));

    session.control(&["resize-window", "--width", "1000", "--height", "700"]);
    std::thread::sleep(Duration::from_millis(600));
    session.control(&["resize-window", "--width", "760", "--height", "520"]);
    std::thread::sleep(Duration::from_millis(600));

    session.control(&["send-text", "echo SURVIVED_RESIZE\r"]);
    session.wait_for_pane("SURVIVED_RESIZE", Duration::from_secs(20));

    let tabs = session.control(&["list-tabs"]);
    assert!(
        tabs.contains("\"child_alive\": true"),
        "the child died across a resize.\ntabs: {tabs}\ndiagnostics:\n{}",
        diagnostics()
    );
}

/// Ctrl+C has to *interrupt*, not arrive as a keystroke.
///
/// `WriteConsoleInput` does not raise a console control event -- the console
/// only synthesizes one for real keyboard input -- so delivering Ctrl+C as a
/// key record gave the shell the character and not the signal. It echoed
/// `^C` and went on running whatever it was running. The agent raises
/// `GenerateConsoleCtrlEvent` instead.
#[test]
fn ctrl_c_interrupts_a_running_child_instead_of_being_typed() {
    let _guard = gui_guard();
    let session = Session::start("interrupt");
    session.wait_for_pane("Microsoft Windows", Duration::from_secs(20));

    // A loop that never ends on its own, so anything that stops it can only
    // be the interrupt. `ping -t` is present on every Windows and prints
    // continuously, which also proves the child was actually running.
    session.control(&["send-text", "ping -t 127.0.0.1\r"]);
    session.wait_for_pane("127.0.0.1", Duration::from_secs(20));

    session.control(&["send-text", "\u{3}"]);

    // The prompt coming back is the proof: the loop ended and the shell is
    // accepting input again.
    session.control(&["send-text", "set /a 4242+0\r"]);
    let pane = session.wait_for_pane("4242", Duration::from_secs(25));
    assert!(
        pane.contains("4242"),
        "the shell never returned to a prompt after Ctrl+C:\n{pane}"
    );

    let tabs = session.control(&["list-tabs"]);
    assert!(
        tabs.contains("\"child_alive\": true"),
        "Ctrl+C killed the shell rather than the command it was running.\n\
         tabs: {tabs}\ndiagnostics:\n{}",
        diagnostics()
    );
}

/// A double-width character occupies two console cells carrying the same code
/// unit. Emitting both is the doubled-CJK bug, and it is invisible in any
/// ASCII-only test.
#[test]
fn a_wide_character_is_not_emitted_twice() {
    let _guard = gui_guard();
    let session = Session::start("wide");
    session.wait_for_pane("Microsoft Windows", Duration::from_secs(20));
    session.control(&["send-text", "echo [中文]\r"]);
    let pane = session.wait_for_pane("[中文]", Duration::from_secs(20));
    assert!(
        !pane.contains("中中") && !pane.contains("文文"),
        "a wide character was emitted for both of its cells:\n{pane}"
    );
}

/// The agent must not leave the child behind when the host goes away. An
/// orphan attached to an invisible console is unkillable from any UI.
#[test]
fn closing_the_host_takes_the_agent_and_its_child_with_it() {
    let _guard = gui_guard();
    let before = console_agent_processes();
    {
        let session = Session::start("teardown");
        session.wait_for_pane("Microsoft Windows", Duration::from_secs(20));
        assert!(
            console_agent_processes() > before,
            "no agent process was started"
        );
    }
    let deadline = Instant::now() + Duration::from_secs(15);
    while Instant::now() < deadline && console_agent_processes() > before {
        std::thread::sleep(Duration::from_millis(250));
    }
    assert_eq!(
        console_agent_processes(),
        before,
        "an agent survived its host"
    );
}

/// Counts agent processes by their command line, which is the only thing that
/// distinguishes them from the product: they are the same executable.
fn console_agent_processes() -> usize {
    // Invoked directly rather than through `cmd /c`: the query contains
    // quotes, and routing it through a shell means Rust's argument escaping
    // and cmd's parsing have to agree about them, which they do not.
    let output = Command::new("wmic")
        .args([
            "process",
            "where",
            "name='agenterm-con.exe'",
            "get",
            "commandline",
        ])
        .output();
    let Ok(output) = output else {
        return 0;
    };
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter(|line| line.contains("--agenterm-console-agent"))
        .count()
}

/// Guards the argument itself: the host builds it and the agent matches it as
/// literal text, so a rename on one side alone is a silent product that spawns
/// a second copy of itself as a terminal.
#[test]
fn the_agent_argument_is_the_one_both_sides_agree_on() {
    let path: &Path = &binary();
    assert!(path.is_file());
    let output = Command::new(path)
        .arg("--agenterm-console-agent")
        .arg("not-a-handle")
        .output()
        .expect("run agent mode");
    assert_eq!(
        output.status.code(),
        Some(251),
        "a malformed agent request must fail as an agent, not open a window"
    );
}
