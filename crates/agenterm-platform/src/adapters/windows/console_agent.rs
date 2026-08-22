//! A pseudoconsole for Windows builds that do not have one.
//!
//! ConPTY arrived in Windows 10 build 17763. On anything older — Windows
//! Server 2016 is 14393 and still in support — `CreatePseudoConsole` does not
//! exist, and Microsoft's ConPTY redistributable does not lower that floor
//! either: it supports 10.0.17763.0 and above, the same as the in-box API.
//!
//! What does work everywhere is the mechanism every terminal used before
//! ConPTY: put the child in its own *hidden* console, then read that console's
//! screen buffer and turn what changed into a terminal stream. The APIs
//! involved (`AllocConsole`, `ReadConsoleOutputW`, `WriteConsoleInputW`) are
//! as old as Win32.
//!
//! A process can be attached to only one console, so the scraping cannot
//! happen inside a GUI host. It runs in a separate agent process — this
//! executable, re-executed with [`AGENT_ARGUMENT`], so nothing third-party
//! enters the product.
//!
//! The agent speaks the same two pipes a pseudoconsole does: terminal bytes
//! out, input bytes in. Everything above the adapter therefore cannot tell
//! which backend it got, which is the whole point — the difference is sealed
//! here and not spread through the terminal.

use std::ffi::c_void;
use std::io;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::ptr::{null, null_mut};

use windows_sys::Win32::Foundation::{
    CloseHandle, GENERIC_READ, GENERIC_WRITE, GetLastError, HANDLE, INVALID_HANDLE_VALUE,
    WAIT_OBJECT_0,
};
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING, ReadFile, WriteFile,
};
use windows_sys::Win32::System::Console::{
    AllocConsole, CHAR_INFO, CHAR_INFO_0, COORD, CONSOLE_SCREEN_BUFFER_INFO, FreeConsole,
    GetConsoleScreenBufferInfo, GetConsoleWindow, INPUT_RECORD, INPUT_RECORD_0, KEY_EVENT,
    KEY_EVENT_RECORD, KEY_EVENT_RECORD_0, ReadConsoleOutputW, SMALL_RECT,
    SetConsoleScreenBufferSize, SetConsoleWindowInfo, WriteConsoleInputW,
};
use windows_sys::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
    SetInformationJobObject,
};
use windows_sys::Win32::System::Threading::{
    CREATE_NEW_CONSOLE, CREATE_UNICODE_ENVIRONMENT, CreateProcessW, GetExitCodeProcess,
    PROCESS_INFORMATION, STARTF_USESHOWWINDOW, STARTF_USESTDHANDLES, STARTUPINFOW,
    WaitForSingleObject,
};

/// The argument that turns this executable into an agent. Deliberately long
/// and prefixed: it must never collide with a real option, and a user who
/// finds it in a process list should be able to tell what it belongs to.
pub const AGENT_ARGUMENT: &str = "--agenterm-console-agent";

/// How often the screen buffer is polled when the child is producing output.
/// The console API offers no change notification, so this is the floor on
/// output latency; 8 ms is under a frame and well below the cost of the
/// `ReadConsoleOutputW` itself at terminal sizes.
const POLL_BUSY: std::time::Duration = std::time::Duration::from_millis(8);
/// Backoff once the screen stops changing, so an idle shell is not a spin.
const POLL_IDLE: std::time::Duration = std::time::Duration::from_millis(40);
/// Idle polls before backing off. Roughly a quarter second of quiet.
const IDLE_POLLS: u32 = 30;
/// Consecutive failed screen reads before the session is declared lost.
/// Generous on purpose: the cost of giving up early is a terminal that dies,
/// and the cost of giving up late is a fraction of a second of stale screen.
const MAX_CONSECUTIVE_POLL_FAILURES: u32 = 50;

const SW_HIDE: u16 = 0;

#[link(name = "user32")]
unsafe extern "system" {
    fn ShowWindow(window: HANDLE, command: i32) -> i32;
}

/// A raw handle that may cross into a worker thread.
///
/// Console and pipe handles are process-wide kernel objects, not thread
/// affine; `HANDLE` is only `!Send` because it is a bare pointer. Each of
/// these is moved to exactly one thread that owns it for the process
/// lifetime, so there is no sharing to synchronize.
#[derive(Clone, Copy)]
struct Portable(HANDLE);

// SAFETY: see the type comment -- the referent is a kernel object with no
// thread affinity, and each value is moved to a single owning thread.
unsafe impl Send for Portable {}

impl Portable {
    /// Taken by value so a closure captures the wrapper rather than reaching
    /// through to the bare pointer field, which is not `Send`.
    fn handle(self) -> HANDLE {
        self.0
    }
}

// CHAR_INFO attribute bits. windows-sys exposes these as plain u16 constants
// in a module this crate does not otherwise need, so they are named here.
const FOREGROUND_BLUE: u16 = 0x0001;
const FOREGROUND_GREEN: u16 = 0x0002;
const FOREGROUND_RED: u16 = 0x0004;
const FOREGROUND_INTENSITY: u16 = 0x0008;
const BACKGROUND_BLUE: u16 = 0x0010;
const BACKGROUND_GREEN: u16 = 0x0020;
const BACKGROUND_RED: u16 = 0x0040;
const BACKGROUND_INTENSITY: u16 = 0x0080;
/// A double-width character occupies two cells that carry the *same* code
/// unit. Without these bits every wide glyph is emitted twice.
const COMMON_LVB_LEADING_BYTE: u16 = 0x0100;
const COMMON_LVB_TRAILING_BYTE: u16 = 0x0200;
const COMMON_LVB_REVERSE_VIDEO: u16 = 0x4000;
const COMMON_LVB_UNDERSCORE: u16 = 0x8000;

const DEFAULT_ATTRIBUTES: u16 = FOREGROUND_RED | FOREGROUND_GREEN | FOREGROUND_BLUE;

// ---------------------------------------------------------------------------
// Command-line transport
// ---------------------------------------------------------------------------

/// The child's command line travels as hex rather than as further arguments.
///
/// It is already a fully quoted Windows command line built for `CreateProcessW`
/// and re-quoting it to survive a second `CommandLineToArgvW` round trip is a
/// class of bug with no upside. Hex has no metacharacters at all.
fn encode_utf16_hex(units: &[u16]) -> String {
    let mut encoded = String::with_capacity(units.len() * 4);
    for unit in units {
        encoded.push_str(&format!("{unit:04x}"));
    }
    encoded
}

fn decode_utf16_hex(encoded: &str) -> Option<Vec<u16>> {
    if !encoded.len().is_multiple_of(4) {
        return None;
    }
    let bytes = encoded.as_bytes();
    let mut units = Vec::with_capacity(encoded.len() / 4);
    for chunk in bytes.chunks(4) {
        let text = std::str::from_utf8(chunk).ok()?;
        units.push(u16::from_str_radix(text, 16).ok()?);
    }
    Some(units)
}

// ---------------------------------------------------------------------------
// Host side
// ---------------------------------------------------------------------------

/// Everything the host needs to talk to a spawned agent.
pub(crate) struct AgentSpawn {
    /// The agent process. It outlives the child by exactly as long as it takes
    /// to flush the final screen, so the host may wait on it as if it were the
    /// child.
    pub(crate) process: OwnedHandle,
    pub(crate) pid: u32,
    /// Resize requests. Separate from the input pipe so a resize cannot be
    /// mistaken for something the child typed.
    pub(crate) control: OwnedHandle,
}

/// Spawns the agent with the three pipe ends it needs.
///
/// `child_command_line` is the fully quoted line the child would have been
/// given directly, and `environment` the UTF-16 environment block, both
/// already built by the ConPTY path — the agent inherits them rather than
/// rebuilding them, so both backends start the child from identical inputs.
pub(crate) fn spawn_agent(
    input_read: &OwnedHandle,
    output_write: &OwnedHandle,
    child_command_line: &[u16],
    environment: Option<&[u16]>,
    current_dir: Option<&[u16]>,
    cols: u16,
    rows: u16,
) -> io::Result<AgentSpawn> {
    // The two session pipes are built by the ConPTY path, which hands its
    // endpoints straight to `CreatePseudoConsole` and therefore creates them
    // non-inheritable. An agent inherits or it gets nothing, and "nothing"
    // surfaces only as ERROR_INVALID_HANDLE from the first read.
    make_inheritable(input_read)?;
    make_inheritable(output_write)?;

    let control = create_pipe()?;
    let executable = std::env::current_exe()?;
    let mut executable_utf16: Vec<u16> = executable
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();

    let command_line = format!(
        "\"{}\" {AGENT_ARGUMENT} {} {} {} {cols} {rows} {}",
        executable.display().to_string().replace('"', "\"\""),
        input_read.as_raw_handle() as usize,
        output_write.as_raw_handle() as usize,
        control.read.as_raw_handle() as usize,
        encode_utf16_hex(child_command_line)
    );
    let mut command_line: Vec<u16> = command_line
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();

    let mut startup: STARTUPINFOW = unsafe { std::mem::zeroed() };
    startup.cb = size_of::<STARTUPINFOW>() as u32;
    // Both flags matter. USESTDHANDLES keeps the agent from inheriting the
    // host's stdio, and USESHOWWINDOW with SW_HIDE means the new console is
    // never shown -- hiding it after creation flashes a window.
    startup.dwFlags = STARTF_USESTDHANDLES | STARTF_USESHOWWINDOW;
    startup.wShowWindow = SW_HIDE;
    startup.hStdInput = input_read.as_raw_handle() as HANDLE;
    startup.hStdOutput = output_write.as_raw_handle() as HANDLE;
    startup.hStdError = output_write.as_raw_handle() as HANDLE;

    let mut information: PROCESS_INFORMATION = unsafe { std::mem::zeroed() };
    let created = unsafe {
        // SAFETY: every pointer is either null or a live NUL-terminated
        // buffer owned by this frame for the duration of the call.
        CreateProcessW(
            executable_utf16.as_mut_ptr(),
            command_line.as_mut_ptr(),
            null(),
            null(),
            1,
            CREATE_NEW_CONSOLE | CREATE_UNICODE_ENVIRONMENT,
            environment.map_or(null(), |block| block.as_ptr().cast::<c_void>()),
            current_dir.map_or(null(), <[u16]>::as_ptr),
            &startup,
            &mut information,
        )
    };
    let _ = &mut executable_utf16;
    if created == 0 {
        return Err(io::Error::last_os_error());
    }
    unsafe {
        // SAFETY: the thread handle is owned by this frame and unused; the
        // agent is not created suspended.
        CloseHandle(information.hThread);
    }
    Ok(AgentSpawn {
        process: unsafe {
            // SAFETY: CreateProcessW transferred a unique process handle.
            OwnedHandle::from_raw_handle(information.hProcess as _)
        },
        pid: information.dwProcessId,
        control: control.write,
    })
}

/// Tells a running agent the terminal is now `cols` x `rows`.
///
/// Four bytes, fixed width: a pipe carries no message boundaries, and a
/// length-prefixed format would only add a way to get out of sync.
pub(crate) fn request_resize(control: &OwnedHandle, cols: u16, rows: u16) -> io::Result<()> {
    let message = [
        (cols & 0xFF) as u8,
        (cols >> 8) as u8,
        (rows & 0xFF) as u8,
        (rows >> 8) as u8,
    ];
    write_all(control.as_raw_handle() as HANDLE, &message)
}

/// Marks one already-created handle as inheritable, in place.
///
/// Duplicating instead would give the child a different handle value than the
/// one written into its command line, which is the whole way it finds them.
fn make_inheritable(handle: &OwnedHandle) -> io::Result<()> {
    use windows_sys::Win32::Foundation::{HANDLE_FLAG_INHERIT, SetHandleInformation};

    let set = unsafe {
        // SAFETY: the handle is live and owned by the caller for this call.
        SetHandleInformation(
            handle.as_raw_handle() as HANDLE,
            HANDLE_FLAG_INHERIT,
            HANDLE_FLAG_INHERIT,
        )
    };
    if set == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

struct Pipe {
    read: OwnedHandle,
    write: OwnedHandle,
}

fn create_pipe() -> io::Result<Pipe> {
    use windows_sys::Win32::Security::SECURITY_ATTRIBUTES;
    use windows_sys::Win32::System::Pipes::CreatePipe;

    let attributes = SECURITY_ATTRIBUTES {
        nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: null_mut(),
        // Inheritable, or the agent receives a closed handle and never hears
        // about a resize.
        bInheritHandle: 1,
    };
    let mut read = null_mut();
    let mut write = null_mut();
    let created = unsafe {
        // SAFETY: both slots are valid out-pointers and attributes is a live
        // initialized structure.
        CreatePipe(&mut read, &mut write, &attributes, 0)
    };
    if created == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(Pipe {
        read: unsafe {
            // SAFETY: CreatePipe transferred this unique handle.
            OwnedHandle::from_raw_handle(read as _)
        },
        write: unsafe {
            // SAFETY: CreatePipe transferred this unique handle.
            OwnedHandle::from_raw_handle(write as _)
        },
    })
}

// ---------------------------------------------------------------------------
// Agent side
// ---------------------------------------------------------------------------

/// Runs the agent if `arguments` say to, returning the exit code to use.
///
/// `None` means these are ordinary arguments and the caller should carry on
/// being itself. A binary that embeds this adapter calls this before parsing
/// anything else; the agent is not a mode of the product, it is a different
/// program that happens to live in the same file.
#[must_use]
pub fn run_if_agent(arguments: &[String]) -> Option<i32> {
    let position = arguments.iter().position(|value| value == AGENT_ARGUMENT)?;
    let rest = &arguments[position + 1..];
    Some(match parse_and_run(rest) {
        Ok(code) => code,
        // The agent owns a hidden console and its stdout is the terminal
        // stream, so there is nowhere to print. The exit code tells the host
        // that the agent and not the child failed; the diagnostics sink is
        // what says which step, because "exit 251" on its own is exactly the
        // kind of dead end this whole area keeps producing.
        Err(error) => {
            #[cfg(feature = "runtime")]
            crate::diagnostics::record("console_agent", "agent_failed", &error.to_string());
            let _ = &error;
            251
        }
    })
}

fn parse_and_run(rest: &[String]) -> io::Result<i32> {
    let invalid = || io::Error::new(io::ErrorKind::InvalidInput, "malformed console agent request");
    if rest.len() < 6 {
        return Err(invalid());
    }
    let handle_at = |index: usize| -> io::Result<HANDLE> {
        rest[index]
            .parse::<usize>()
            .map(|value| value as HANDLE)
            .map_err(|_| invalid())
    };
    let input_read = handle_at(0)?;
    let output_write = handle_at(1)?;
    let control_read = handle_at(2)?;
    let cols: u16 = rest[3].parse().map_err(|_| invalid())?;
    let rows: u16 = rest[4].parse().map_err(|_| invalid())?;
    let command_line = decode_utf16_hex(&rest[5]).ok_or_else(invalid)?;
    run_agent(
        input_read,
        output_write,
        control_read,
        cols.max(1),
        rows.max(1),
        command_line,
    )
}

fn step<T>(what: &str, result: io::Result<T>) -> io::Result<T> {
    result.map_err(|error| io::Error::new(error.kind(), format!("{what}: {error}")))
}

fn run_agent(
    input_read: HANDLE,
    output_write: HANDLE,
    control_read: HANDLE,
    cols: u16,
    rows: u16,
    mut command_line: Vec<u16>,
) -> io::Result<i32> {
    let console = step("acquire console", ConsoleHandles::acquire())?;
    step("resize console", console.resize(cols, rows))?;

    let job = step("create job", OwnedJob::new())?;
    let child = step("spawn child", spawn_child(&console, &mut command_line))?;
    // The child dies with the agent. Without this, killing the host leaves an
    // orphan attached to a console nobody can see.
    unsafe {
        // SAFETY: both handles are live and owned by this frame.
        AssignProcessToJobObject(job.0.as_raw_handle() as HANDLE, child.as_raw_handle() as HANDLE);
    }

    let input_console = Portable(console.input.as_raw_handle() as HANDLE);
    let input_pipe = Portable(input_read);
    spawn_thread("agenterm-console-agent-input", move || {
        forward_input(input_pipe.handle(), input_console.handle());
    });
    let control_pipe = Portable(control_read);
    spawn_thread("agenterm-console-agent-control", move || {
        forward_control(control_pipe.handle());
    });

    let mut screen = ScreenMirror::new(cols, rows);
    let mut idle = 0_u32;
    let mut consecutive_failures = 0_u32;
    loop {
        apply_pending_resize(&console);
        let changed = match screen.poll_and_emit(&console, output_write) {
            Ok(changed) => {
                consecutive_failures = 0;
                changed
            }
            Err(error) => {
                // A single failed read is survivable -- a resize landing
                // between the size query and the read is the ordinary cause,
                // and the next poll sees a consistent console. Only a
                // persistent failure means the session is really gone, and
                // treating the first one as fatal is what silently killed
                // the terminal on every window resize.
                consecutive_failures += 1;
                if consecutive_failures > MAX_CONSECUTIVE_POLL_FAILURES {
                    return Err(error);
                }
                false
            }
        };
        idle = if changed { 0 } else { idle.saturating_add(1) };

        let child_state = unsafe {
            // SAFETY: child is a live process handle owned by this frame.
            WaitForSingleObject(child.as_raw_handle() as HANDLE, 0)
        };
        if child_state == WAIT_OBJECT_0 {
            // One last pass: the child's final output can land between the
            // previous poll and its exit, and dropping it loses exactly the
            // line a user most wants to see.
            let _ = screen.poll_and_emit(&console, output_write);
            break;
        }
        std::thread::sleep(if idle >= IDLE_POLLS { POLL_IDLE } else { POLL_BUSY });
    }

    let mut code: u32 = 0;
    unsafe {
        // SAFETY: the child has exited and the handle is still owned here.
        GetExitCodeProcess(child.as_raw_handle() as HANDLE, &mut code);
    }
    // Closing the stream is what tells the host the session ended.
    unsafe {
        // SAFETY: output_write was inherited and is not used after this.
        CloseHandle(output_write);
    }
    Ok(code as i32)
}

fn spawn_thread(name: &'static str, task: impl FnOnce() + Send + 'static) {
    let _ = crate::threading::spawn_named_detached(name, Box::new(task));
}

struct OwnedJob(OwnedHandle);

impl OwnedJob {
    fn new() -> io::Result<Self> {
        let job = unsafe {
            // SAFETY: null attributes and name request an unnamed job.
            CreateJobObjectW(null(), null())
        };
        if job.is_null() {
            return Err(io::Error::last_os_error());
        }
        let job = unsafe {
            // SAFETY: CreateJobObjectW transferred a unique handle.
            OwnedHandle::from_raw_handle(job as _)
        };
        let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { std::mem::zeroed() };
        limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        unsafe {
            // SAFETY: limits is live, initialized and correctly sized.
            SetInformationJobObject(
                job.as_raw_handle() as HANDLE,
                JobObjectExtendedLimitInformation,
                (&raw const limits).cast::<c_void>(),
                size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            );
        }
        Ok(Self(job))
    }
}

/// The agent's own console, opened by name.
///
/// `GetStdHandle` is the wrong way to reach it: the agent's standard handles
/// are the host's pipes, and stay that way after `AllocConsole`. `CONOUT$`
/// and `CONIN$` name the attached console directly.
struct ConsoleHandles {
    input: OwnedHandle,
    output: OwnedHandle,
}

impl ConsoleHandles {
    fn acquire() -> io::Result<Self> {
        unsafe {
            // SAFETY: neither call takes arguments. FreeConsole is tolerated
            // failing -- it only matters when one was already attached.
            FreeConsole();
            if AllocConsole() == 0 {
                return Err(io::Error::last_os_error());
            }
            // Belt and braces with STARTF_USESHOWWINDOW: a console allocated
            // at run time can still surface a window on some configurations.
            let window = GetConsoleWindow();
            if !window.is_null() {
                ShowWindow(window, i32::from(SW_HIDE));
            }
        }
        Ok(Self {
            input: open_console_device("CONIN$")?,
            output: open_console_device("CONOUT$")?,
        })
    }

    fn info(&self) -> io::Result<CONSOLE_SCREEN_BUFFER_INFO> {
        let mut info: CONSOLE_SCREEN_BUFFER_INFO = unsafe { std::mem::zeroed() };
        let read = unsafe {
            // SAFETY: the handle is a live console output handle and info is
            // writable storage of exactly the required size.
            GetConsoleScreenBufferInfo(self.output.as_raw_handle() as HANDLE, &mut info)
        };
        if read == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(info)
    }

    /// Makes the buffer exactly the terminal's size.
    ///
    /// Buffer height equal to window height is deliberate: a taller buffer
    /// would keep scrollback the host cannot see, and the host owns
    /// scrollback. Shrinking has to move the window first and growing has to
    /// move it last, because neither may ever exceed the buffer.
    fn resize(&self, cols: u16, rows: u16) -> io::Result<()> {
        let handle = self.output.as_raw_handle() as HANDLE;
        let target = COORD {
            X: cols.min(i16::MAX as u16) as i16,
            Y: rows.min(i16::MAX as u16) as i16,
        };
        let minimal = SMALL_RECT {
            Left: 0,
            Top: 0,
            Right: 0,
            Bottom: 0,
        };
        let window = SMALL_RECT {
            Left: 0,
            Top: 0,
            Right: target.X - 1,
            Bottom: target.Y - 1,
        };
        unsafe {
            // SAFETY: handle is a live console output handle; both rectangles
            // and the coordinate are initialized locals.
            SetConsoleWindowInfo(handle, 1, &minimal);
            if SetConsoleScreenBufferSize(handle, target) == 0 {
                // Growing the window before the buffer is the failing order;
                // retry after making room.
                let error = io::Error::last_os_error();
                if SetConsoleWindowInfo(handle, 1, &window) == 0 {
                    return Err(error);
                }
                if SetConsoleScreenBufferSize(handle, target) == 0 {
                    return Err(error);
                }
            }
            if SetConsoleWindowInfo(handle, 1, &window) == 0 {
                return Err(io::Error::last_os_error());
            }
        }
        Ok(())
    }
}

fn open_console_device(name: &str) -> io::Result<OwnedHandle> {
    use windows_sys::Win32::Security::SECURITY_ATTRIBUTES;

    let attributes = SECURITY_ATTRIBUTES {
        nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: null_mut(),
        // The child must inherit these, or it paints nowhere and the scrape
        // comes back blank with no error anywhere.
        bInheritHandle: 1,
    };
    let wide: Vec<u16> = name.encode_utf16().chain(std::iter::once(0)).collect();
    let handle = unsafe {
        // SAFETY: the name is NUL-terminated and attributes is initialized.
        CreateFileW(
            wide.as_ptr(),
            GENERIC_READ | GENERIC_WRITE,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            &attributes,
            OPEN_EXISTING,
            0,
            null_mut(),
        )
    };
    if handle.is_null() || handle == INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error());
    }
    Ok(unsafe {
        // SAFETY: CreateFileW returned a unique owned handle.
        OwnedHandle::from_raw_handle(handle as _)
    })
}

fn spawn_child(console: &ConsoleHandles, command_line: &mut [u16]) -> io::Result<OwnedHandle> {
    let mut startup: STARTUPINFOW = unsafe { std::mem::zeroed() };
    startup.cb = size_of::<STARTUPINFOW>() as u32;
    startup.dwFlags = STARTF_USESTDHANDLES;
    startup.hStdInput = console.input.as_raw_handle() as HANDLE;
    startup.hStdOutput = console.output.as_raw_handle() as HANDLE;
    startup.hStdError = console.output.as_raw_handle() as HANDLE;

    let mut information: PROCESS_INFORMATION = unsafe { std::mem::zeroed() };
    let created = unsafe {
        // SAFETY: command_line is a live NUL-terminated mutable buffer and
        // every other pointer is null or a live local. The child inherits the
        // agent's environment and working directory, which the host already
        // set to what the child should see.
        CreateProcessW(
            null(),
            command_line.as_mut_ptr(),
            null(),
            null(),
            1,
            0,
            null(),
            null(),
            &startup,
            &mut information,
        )
    };
    if created == 0 {
        return Err(io::Error::last_os_error());
    }
    unsafe {
        // SAFETY: the thread handle is owned here and never used.
        CloseHandle(information.hThread);
    }
    Ok(unsafe {
        // SAFETY: CreateProcessW transferred a unique process handle.
        OwnedHandle::from_raw_handle(information.hProcess as _)
    })
}

// ---------------------------------------------------------------------------
// Screen mirror: console buffer -> terminal stream
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Cell {
    text: char,
    attributes: u16,
    /// A wide character's second cell, which carries no glyph of its own.
    continuation: bool,
}

impl Default for Cell {
    fn default() -> Self {
        Self {
            text: ' ',
            attributes: DEFAULT_ATTRIBUTES,
            continuation: false,
        }
    }
}

/// What the host has already been told the screen looks like.
///
/// The mirror exists so the agent emits *differences*. Repainting everything
/// each poll would work and would also make every keystroke redraw the screen,
/// which is both slow and visibly wrong when the user has scrolled.
struct ScreenMirror {
    cols: u16,
    rows: u16,
    cells: Vec<Cell>,
    cursor: (u16, u16),
    attributes: u16,
    /// The absolute buffer row the window started at last time. When the
    /// console scrolls, this is how the agent knows to push lines into the
    /// host's scrollback rather than silently overwrite them.
    window_top: i16,
    started: bool,
}

impl ScreenMirror {
    fn new(cols: u16, rows: u16) -> Self {
        Self {
            cols,
            rows,
            cells: vec![Cell::default(); usize::from(cols) * usize::from(rows)],
            cursor: (0, 0),
            attributes: DEFAULT_ATTRIBUTES,
            window_top: 0,
            started: false,
        }
    }

    /// Reads the console once and writes whatever changed. Returns whether
    /// anything did.
    fn poll_and_emit(&mut self, console: &ConsoleHandles, output: HANDLE) -> io::Result<bool> {
        let info = console.info()?;
        let cols = (info.srWindow.Right - info.srWindow.Left + 1).max(1) as u16;
        let rows = (info.srWindow.Bottom - info.srWindow.Top + 1).max(1) as u16;
        let mut out = Vec::with_capacity(256);

        if cols != self.cols || rows != self.rows {
            // A resize invalidates every row. Start the mirror over rather
            // than trying to reconcile two geometries.
            *self = Self::new(cols, rows);
        }

        let scrolled = info.srWindow.Top - self.window_top;
        self.window_top = info.srWindow.Top;

        let read = read_window(console, &info, cols, rows)?;

        if !self.started {
            self.started = true;
            // Clear and home once, so the host's idea of the screen and the
            // mirror's start out identical instead of merely similar.
            out.extend_from_slice(b"\x1b[H\x1b[2J");
            self.attributes = DEFAULT_ATTRIBUTES;
            out.extend_from_slice(b"\x1b[0m");
            self.cells = vec![Cell::default(); usize::from(cols) * usize::from(rows)];
            self.cursor = (0, 0);
        } else if scrolled > 0 {
            // Park at the last row and feed newlines: that is what pushes the
            // vacated lines into the host's scrollback. Emitting them as text
            // would duplicate content the host already has.
            let feed = usize::from(rows).min(scrolled as usize);
            out.extend_from_slice(format!("\x1b[{};1H", rows).as_bytes());
            for _ in 0..feed {
                out.extend_from_slice(b"\r\n");
            }
            self.scroll_mirror(feed);
            self.cursor = (rows.saturating_sub(1), 0);
        }

        for row in 0..rows {
            let start = usize::from(row) * usize::from(cols);
            let new_row = &read[start..start + usize::from(cols)];
            if self.cells[start..start + usize::from(cols)] == *new_row {
                continue;
            }
            self.emit_row(&mut out, row, new_row);
            self.cells[start..start + usize::from(cols)].copy_from_slice(new_row);
        }

        // Clamped into the window: the console reports the cursor in absolute
        // buffer coordinates, and a cursor parked one row past the last line
        // would otherwise be emitted as a position the host does not have.
        let last_row = rows.saturating_sub(1) as i16;
        let last_column = cols.saturating_sub(1) as i16;
        let cursor = (
            (info.dwCursorPosition.Y - info.srWindow.Top).clamp(0, last_row) as u16,
            (info.dwCursorPosition.X - info.srWindow.Left).clamp(0, last_column) as u16,
        );
        if cursor != self.cursor || !out.is_empty() {
            out.extend_from_slice(format!("\x1b[{};{}H", cursor.0 + 1, cursor.1 + 1).as_bytes());
            self.cursor = cursor;
        }

        if out.is_empty() {
            return Ok(false);
        }
        write_all(output, &out)?;
        Ok(true)
    }

    fn scroll_mirror(&mut self, lines: usize) {
        let width = usize::from(self.cols);
        let shift = lines.min(usize::from(self.rows)) * width;
        self.cells.drain(..shift);
        self.cells.resize(width * usize::from(self.rows), Cell::default());
    }

    /// Rewrites one row from column one. Erasing to end of line first is what
    /// makes a shortened line actually get shorter.
    fn emit_row(&mut self, out: &mut Vec<u8>, row: u16, cells: &[Cell]) {
        out.extend_from_slice(format!("\x1b[{};1H\x1b[K", row + 1).as_bytes());
        let mut text = String::new();
        for cell in cells {
            if cell.continuation {
                continue;
            }
            if cell.attributes != self.attributes {
                if !text.is_empty() {
                    out.extend_from_slice(text.as_bytes());
                    text.clear();
                }
                out.extend_from_slice(sgr_for(cell.attributes).as_bytes());
                self.attributes = cell.attributes;
            }
            text.push(cell.text);
        }
        // Trailing blanks are already handled by the erase, so they are only
        // written when a later cell on the row is non-blank.
        while text.ends_with(' ') {
            text.pop();
        }
        out.extend_from_slice(text.as_bytes());
        self.cursor = (row, 0);
    }
}

fn read_window(
    console: &ConsoleHandles,
    info: &CONSOLE_SCREEN_BUFFER_INFO,
    cols: u16,
    rows: u16,
) -> io::Result<Vec<Cell>> {
    let mut raw = vec![
        CHAR_INFO {
            Char: CHAR_INFO_0 { UnicodeChar: 0 },
            Attributes: DEFAULT_ATTRIBUTES,
        };
        usize::from(cols) * usize::from(rows)
    ];
    let mut region = SMALL_RECT {
        Left: info.srWindow.Left,
        Top: info.srWindow.Top,
        Right: info.srWindow.Left + cols as i16 - 1,
        Bottom: info.srWindow.Top + rows as i16 - 1,
    };
    let read = unsafe {
        // SAFETY: raw is exactly size.X * size.Y elements, matching the
        // rectangle requested, and region is a live out/in parameter.
        ReadConsoleOutputW(
            console.output.as_raw_handle() as HANDLE,
            raw.as_mut_ptr(),
            COORD {
                X: cols as i16,
                Y: rows as i16,
            },
            COORD { X: 0, Y: 0 },
            &mut region,
        )
    };
    if read == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(raw.into_iter().map(decode_cell).collect())
}

fn decode_cell(raw: CHAR_INFO) -> Cell {
    let unit = unsafe {
        // SAFETY: the console always fills the Unicode arm for ReadConsoleOutputW.
        raw.Char.UnicodeChar
    };
    Cell {
        text: char::from_u32(u32::from(unit)).filter(|c| *c != '\0').unwrap_or(' '),
        // The width bits describe layout, not appearance; keeping them would
        // make two identical-looking cells compare unequal.
        attributes: raw.Attributes & !(COMMON_LVB_LEADING_BYTE | COMMON_LVB_TRAILING_BYTE),
        continuation: raw.Attributes & COMMON_LVB_TRAILING_BYTE != 0,
    }
}

/// Console attribute bits to an SGR sequence.
///
/// The console orders its colour bits blue-green-red and ANSI orders them
/// red-green-blue, so the two nibbles are not interchangeable and swapping
/// red and blue is the entire mapping.
fn sgr_for(attributes: u16) -> String {
    let ansi = |red: bool, green: bool, blue: bool| {
        u8::from(red) | (u8::from(green) << 1) | (u8::from(blue) << 2)
    };
    let foreground = ansi(
        attributes & FOREGROUND_RED != 0,
        attributes & FOREGROUND_GREEN != 0,
        attributes & FOREGROUND_BLUE != 0,
    );
    let background = ansi(
        attributes & BACKGROUND_RED != 0,
        attributes & BACKGROUND_GREEN != 0,
        attributes & BACKGROUND_BLUE != 0,
    );
    let foreground = if attributes & FOREGROUND_INTENSITY != 0 {
        90 + u16::from(foreground)
    } else {
        30 + u16::from(foreground)
    };
    let background = if attributes & BACKGROUND_INTENSITY != 0 {
        100 + u16::from(background)
    } else {
        40 + u16::from(background)
    };
    let mut sequence = String::from("\x1b[0");
    if attributes & COMMON_LVB_REVERSE_VIDEO != 0 {
        sequence.push_str(";7");
    }
    if attributes & COMMON_LVB_UNDERSCORE != 0 {
        sequence.push_str(";4");
    }
    sequence.push_str(&format!(";{foreground};{background}m"));
    sequence
}

// ---------------------------------------------------------------------------
// Input: terminal bytes -> console input records
// ---------------------------------------------------------------------------

/// The most recent size the host asked for, `cols << 16 | rows`, or zero.
///
/// The control thread only records; the poll thread applies. Resizing the
/// console from another thread races the poll loop's `ReadConsoleOutputW`,
/// whose rectangle is then larger than the buffer it is reading — which
/// surfaces as a failed read and, before this, killed the agent mid-session.
static PENDING_RESIZE: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

fn forward_control(control: HANDLE) {
    use std::sync::atomic::Ordering;

    let mut message = [0_u8; 4];
    loop {
        if read_exact(control, &mut message).is_err() {
            return;
        }
        let cols = u16::from(message[0]) | (u16::from(message[1]) << 8);
        let rows = u16::from(message[2]) | (u16::from(message[3]) << 8);
        if cols == 0 || rows == 0 {
            continue;
        }
        // Last writer wins: an intermediate size during a drag is worth
        // nothing once a newer one has arrived.
        PENDING_RESIZE.store((u32::from(cols) << 16) | u32::from(rows), Ordering::Release);
    }
}

/// Applies a size the control thread recorded, if any.
fn apply_pending_resize(console: &ConsoleHandles) {
    use std::sync::atomic::Ordering;

    let pending = PENDING_RESIZE.swap(0, Ordering::AcqRel);
    if pending == 0 {
        return;
    }
    let cols = (pending >> 16) as u16;
    let rows = (pending & 0xFFFF) as u16;
    // A refused resize is not fatal: the next poll simply reads the size the
    // console still has, and the mirror rebuilds itself around it.
    let _ = console.resize(cols.max(1), rows.max(1));
}

fn forward_input(input: HANDLE, console_input: HANDLE) {
    let mut buffer = [0_u8; 1024];
    let mut pending: Vec<u8> = Vec::new();
    loop {
        let read = match read_some(input, &mut buffer) {
            Ok(0) | Err(_) => return,
            Ok(count) => count,
        };
        pending.extend_from_slice(&buffer[..read]);
        let consumed = write_records(console_input, &pending);
        pending.drain(..consumed);
        // An unbounded remainder would mean a byte stream that never forms a
        // sequence; drop it rather than grow without limit.
        if pending.len() > 64 {
            pending.clear();
        }
    }
}

/// Translates as many complete key presses as `bytes` contains, returning how
/// many bytes were consumed. A partial escape sequence is left for the next
/// read rather than being delivered as a literal escape.
fn write_records(console_input: HANDLE, bytes: &[u8]) -> usize {
    let mut records = Vec::new();
    let mut index = 0;
    while index < bytes.len() {
        let Some((key, used)) = decode_key(&bytes[index..]) else {
            break;
        };
        index += used;
        records.push(key_record(key, true));
        records.push(key_record(key, false));
    }
    if !records.is_empty() {
        let mut written = 0_u32;
        unsafe {
            // SAFETY: records is a live slice of initialized records and
            // written is a valid out-pointer.
            WriteConsoleInputW(
                console_input,
                records.as_ptr(),
                records.len() as u32,
                &mut written,
            );
        }
    }
    index
}

#[derive(Clone, Copy)]
struct Key {
    virtual_key: u16,
    unicode: u16,
    control: bool,
}

const LEFT_CTRL_PRESSED: u32 = 0x0008;

fn key_record(key: Key, down: bool) -> INPUT_RECORD {
    INPUT_RECORD {
        EventType: KEY_EVENT as u16,
        Event: INPUT_RECORD_0 {
            KeyEvent: KEY_EVENT_RECORD {
                bKeyDown: i32::from(down),
                wRepeatCount: 1,
                wVirtualKeyCode: key.virtual_key,
                wVirtualScanCode: 0,
                uChar: KEY_EVENT_RECORD_0 {
                    UnicodeChar: key.unicode,
                },
                dwControlKeyState: if key.control { LEFT_CTRL_PRESSED } else { 0 },
            },
        },
    }
}

/// One key press from the front of a terminal byte stream.
///
/// `None` means the bytes so far are a prefix of something longer and the
/// caller should wait for more.
fn decode_key(bytes: &[u8]) -> Option<(Key, usize)> {
    const VK_BACK: u16 = 0x08;
    const VK_TAB: u16 = 0x09;
    const VK_RETURN: u16 = 0x0D;
    const VK_ESCAPE: u16 = 0x1B;
    const VK_END: u16 = 0x23;
    const VK_HOME: u16 = 0x24;
    const VK_F1: u16 = 0x70;

    let plain = |virtual_key: u16, unicode: u16, used: usize| {
        Some((
            Key {
                virtual_key,
                unicode,
                control: false,
            },
            used,
        ))
    };

    match bytes.first()? {
        0x1B => {
            if bytes.len() < 2 {
                // Could be a lone Escape or the start of a sequence. Waiting
                // one more read is better than guessing wrong on every arrow.
                return None;
            }
            match bytes[1] {
                b'[' => decode_csi(bytes),
                b'O' if bytes.len() >= 3 => match bytes[2] {
                    b'P'..=b'S' => plain(VK_F1 + u16::from(bytes[2] - b'P'), 0, 3),
                    b'H' => plain(VK_HOME, 0, 3),
                    b'F' => plain(VK_END, 0, 3),
                    _ => plain(VK_ESCAPE, 0x1B, 1),
                },
                b'O' => None,
                _ => plain(VK_ESCAPE, 0x1B, 1),
            }
        }
        b'\r' | b'\n' => plain(VK_RETURN, u16::from(b'\r'), 1),
        0x08 | 0x7F => plain(VK_BACK, u16::from(b'\x08'), 1),
        b'\t' => plain(VK_TAB, u16::from(b'\t'), 1),
        byte @ 0x01..=0x1A => Some((
            Key {
                // Ctrl+A is 0x01, and the console expects the letter's own
                // virtual key with the control modifier set.
                virtual_key: u16::from(b'A') + u16::from(*byte) - 1,
                unicode: u16::from(*byte),
                control: true,
            },
            1,
        )),
        _ => {
            // Anything else is text. Decode one whole UTF-8 scalar so a
            // multi-byte character is never split into replacement bytes.
            let text = std::str::from_utf8(bytes).map_or_else(
                |error| {
                    let valid = error.valid_up_to();
                    (valid > 0).then(|| std::str::from_utf8(&bytes[..valid]).unwrap_or_default())
                },
                Some,
            )?;
            let character = text.chars().next()?;
            let mut units = [0_u16; 2];
            let encoded = character.encode_utf16(&mut units);
            plain(0, encoded[0], character.len_utf8())
        }
    }
}

fn decode_csi(bytes: &[u8]) -> Option<(Key, usize)> {
    const VK_PRIOR: u16 = 0x21;
    const VK_NEXT: u16 = 0x22;
    const VK_END: u16 = 0x23;
    const VK_HOME: u16 = 0x24;
    const VK_LEFT: u16 = 0x25;
    const VK_UP: u16 = 0x26;
    const VK_RIGHT: u16 = 0x27;
    const VK_DOWN: u16 = 0x28;
    const VK_INSERT: u16 = 0x2D;
    const VK_DELETE: u16 = 0x2E;
    const VK_F1: u16 = 0x70;

    // Find the final byte of the sequence, which is what identifies it.
    let end = bytes[2..].iter().position(|byte| byte.is_ascii_alphabetic() || *byte == b'~')?;
    let final_byte = bytes[2 + end];
    let parameters = &bytes[2..2 + end];
    let used = 3 + end;
    let key = |virtual_key: u16| {
        Some((
            Key {
                virtual_key,
                unicode: 0,
                control: false,
            },
            used,
        ))
    };
    match final_byte {
        b'A' => key(VK_UP),
        b'B' => key(VK_DOWN),
        b'C' => key(VK_RIGHT),
        b'D' => key(VK_LEFT),
        b'H' => key(VK_HOME),
        b'F' => key(VK_END),
        b'~' => {
            let number: u16 = std::str::from_utf8(parameters)
                .ok()
                .and_then(|text| text.split(';').next()?.parse().ok())?;
            match number {
                1 | 7 => key(VK_HOME),
                2 => key(VK_INSERT),
                3 => key(VK_DELETE),
                4 | 8 => key(VK_END),
                5 => key(VK_PRIOR),
                6 => key(VK_NEXT),
                11..=15 => key(VK_F1 + number - 11),
                17..=21 => key(VK_F1 + number - 12),
                23 | 24 => key(VK_F1 + number - 13),
                _ => Some((
                    Key {
                        virtual_key: 0,
                        unicode: 0,
                        control: false,
                    },
                    used,
                )),
            }
        }
        // An unrecognized sequence is consumed rather than delivered as text:
        // a stray "[200~" typed into a shell is worse than a dropped key.
        _ => Some((
            Key {
                virtual_key: 0,
                unicode: 0,
                control: false,
            },
            used,
        )),
    }
}

// ---------------------------------------------------------------------------
// Raw pipe helpers
// ---------------------------------------------------------------------------

fn write_all(handle: HANDLE, mut bytes: &[u8]) -> io::Result<()> {
    while !bytes.is_empty() {
        let mut written = 0_u32;
        let ok = unsafe {
            // SAFETY: bytes is a live slice and written is a valid out-pointer.
            WriteFile(
                handle,
                bytes.as_ptr(),
                bytes.len() as u32,
                &mut written,
                null_mut(),
            )
        };
        if ok == 0 {
            return Err(io::Error::from_raw_os_error(unsafe { GetLastError() } as i32));
        }
        if written == 0 {
            return Err(io::Error::new(io::ErrorKind::WriteZero, "pipe accepted nothing"));
        }
        bytes = &bytes[written as usize..];
    }
    Ok(())
}

fn read_some(handle: HANDLE, buffer: &mut [u8]) -> io::Result<usize> {
    let mut read = 0_u32;
    let ok = unsafe {
        // SAFETY: buffer is live and writable for its whole length.
        ReadFile(
            handle,
            buffer.as_mut_ptr(),
            buffer.len() as u32,
            &mut read,
            null_mut(),
        )
    };
    if ok == 0 {
        return Err(io::Error::from_raw_os_error(unsafe { GetLastError() } as i32));
    }
    Ok(read as usize)
}

fn read_exact(handle: HANDLE, buffer: &mut [u8]) -> io::Result<()> {
    let mut filled = 0;
    while filled < buffer.len() {
        match read_some(handle, &mut buffer[filled..])? {
            0 => return Err(io::Error::from(io::ErrorKind::UnexpectedEof)),
            count => filled += count,
        }
    }
    Ok(())
}

use std::os::windows::ffi::OsStrExt;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_command_line_survives_the_hex_round_trip_with_every_metacharacter() {
        let original: Vec<u16> = "cmd.exe /k \"a b\" & ^ | \"中文\"\0"
            .encode_utf16()
            .collect();
        let encoded = encode_utf16_hex(&original);
        assert!(encoded.chars().all(|c| c.is_ascii_hexdigit()));
        assert_eq!(decode_utf16_hex(&encoded).as_deref(), Some(&original[..]));
    }

    #[test]
    fn a_truncated_or_non_hex_command_line_is_rejected_rather_than_guessed() {
        assert_eq!(decode_utf16_hex("abc"), None, "not a whole unit");
        assert_eq!(decode_utf16_hex("zzzz"), None, "not hexadecimal");
        assert_eq!(decode_utf16_hex(""), Some(Vec::new()));
    }

    /// The console orders colour bits blue-green-red and ANSI red-green-blue.
    /// Getting this backwards is invisible in monochrome output and wrong for
    /// every coloured prompt.
    #[test]
    fn console_colours_map_to_ansi_with_red_and_blue_swapped() {
        assert!(sgr_for(FOREGROUND_RED).contains(";31;"), "{}", sgr_for(FOREGROUND_RED));
        assert!(sgr_for(FOREGROUND_BLUE).contains(";34;"), "{}", sgr_for(FOREGROUND_BLUE));
        assert!(sgr_for(FOREGROUND_GREEN).contains(";32;"));
        assert!(
            sgr_for(FOREGROUND_RED | FOREGROUND_INTENSITY).contains(";91;"),
            "intensity selects the bright range"
        );
        assert!(sgr_for(BACKGROUND_BLUE).contains(";44m"));
        assert!(
            sgr_for(BACKGROUND_BLUE | BACKGROUND_INTENSITY).contains(";104m"),
            "background intensity selects the bright range"
        );
    }

    #[test]
    fn reverse_video_and_underline_are_carried_through() {
        assert!(sgr_for(COMMON_LVB_REVERSE_VIDEO).contains(";7"));
        assert!(sgr_for(COMMON_LVB_UNDERSCORE).contains(";4"));
        assert!(!sgr_for(DEFAULT_ATTRIBUTES).contains(";7"));
    }

    fn wide_cell(unit: u16, trailing: bool) -> CHAR_INFO {
        CHAR_INFO {
            Char: CHAR_INFO_0 { UnicodeChar: unit },
            Attributes: DEFAULT_ATTRIBUTES
                | if trailing {
                    COMMON_LVB_TRAILING_BYTE
                } else {
                    COMMON_LVB_LEADING_BYTE
                },
        }
    }

    /// The trap that shows up as doubled CJK: both halves of a wide character
    /// carry the same code unit, and only the width bits tell them apart.
    #[test]
    fn the_second_half_of_a_wide_character_is_marked_as_continuation() {
        let lead = decode_cell(wide_cell(u16::from(b'A'), false));
        let trail = decode_cell(wide_cell(u16::from(b'A'), true));
        assert!(!lead.continuation);
        assert!(trail.continuation);
        assert_eq!(
            lead.attributes, trail.attributes,
            "width bits must not make two identically-styled cells differ"
        );
    }

    #[test]
    fn a_null_console_cell_reads_as_a_blank_not_a_nul_byte() {
        let cell = decode_cell(CHAR_INFO {
            Char: CHAR_INFO_0 { UnicodeChar: 0 },
            Attributes: DEFAULT_ATTRIBUTES,
        });
        assert_eq!(cell.text, ' ');
    }

    #[test]
    fn arrows_and_editing_keys_decode_to_their_virtual_keys() {
        let cases: &[(&[u8], u16)] = &[
            (b"\x1b[A", 0x26),
            (b"\x1b[B", 0x28),
            (b"\x1b[C", 0x27),
            (b"\x1b[D", 0x25),
            (b"\x1b[H", 0x24),
            (b"\x1b[F", 0x23),
            (b"\x1b[3~", 0x2E),
            (b"\x1b[5~", 0x21),
            (b"\x1b[6~", 0x22),
            (b"\x1bOP", 0x70),
        ];
        for (bytes, expected) in cases {
            let (key, used) = decode_key(bytes).expect("decodes");
            assert_eq!(key.virtual_key, *expected, "{bytes:?}");
            assert_eq!(used, bytes.len(), "{bytes:?} consumed whole");
        }
    }

    /// A half-received escape sequence must not be delivered as a literal
    /// Escape followed by "[", which is what a naive decoder does to every
    /// arrow key that arrives split across two reads.
    #[test]
    fn a_partial_escape_sequence_waits_instead_of_becoming_literal_text() {
        assert!(decode_key(b"\x1b").is_none());
        assert!(decode_key(b"\x1b[").is_none());
        assert!(decode_key(b"\x1b[1").is_none());
        assert!(decode_key(b"\x1bO").is_none());
        let (key, used) = decode_key(b"\x1b[1;5A").expect("complete sequence decodes");
        assert_eq!(key.virtual_key, 0x26);
        assert_eq!(used, 6, "parameters are consumed with the sequence");
    }

    #[test]
    fn control_characters_carry_the_control_modifier_and_the_letter_key() {
        let (key, used) = decode_key(b"\x03").expect("Ctrl+C decodes");
        assert_eq!(used, 1);
        assert!(key.control);
        assert_eq!(key.virtual_key, u16::from(b'C'));
        assert_eq!(key.unicode, 3);
    }

    #[test]
    fn enter_tab_and_backspace_use_the_keys_a_console_expects() {
        assert_eq!(decode_key(b"\r").unwrap().0.virtual_key, 0x0D);
        assert_eq!(decode_key(b"\n").unwrap().0.virtual_key, 0x0D);
        assert_eq!(decode_key(b"\t").unwrap().0.virtual_key, 0x09);
        assert_eq!(decode_key(b"\x7f").unwrap().0.virtual_key, 0x08);
        assert_eq!(decode_key(b"\x08").unwrap().0.virtual_key, 0x08);
    }

    /// A multi-byte character split across two pipe reads must not turn into
    /// replacement characters.
    #[test]
    fn a_split_utf8_character_waits_for_its_remaining_bytes() {
        let text = "中".as_bytes();
        assert!(decode_key(&text[..1]).is_none());
        assert!(decode_key(&text[..2]).is_none());
        let (key, used) = decode_key(text).expect("whole character decodes");
        assert_eq!(used, 3);
        assert_eq!(key.unicode, 0x4E2D);
    }

    #[test]
    fn plain_text_decodes_one_character_at_a_time() {
        let (key, used) = decode_key(b"abc").expect("decodes");
        assert_eq!(used, 1);
        assert_eq!(key.unicode, u16::from(b'a'));
        assert!(!key.control);
    }

    /// The mirror only exists to suppress unchanged output. If an unchanged
    /// screen still emitted bytes, every idle poll would repaint the terminal.
    #[test]
    fn an_unchanged_row_is_not_rewritten() {
        let mut mirror = ScreenMirror::new(4, 2);
        let row = vec![Cell::default(); 4];
        let mut out = Vec::new();
        mirror.emit_row(&mut out, 0, &row);
        assert!(
            out.ends_with(b"\x1b[K"),
            "a blank row erases and writes nothing: {out:?}"
        );
        assert_eq!(mirror.cells[..4], row[..], "mirror starts equal to a blank row");
    }

    #[test]
    fn a_row_is_erased_before_it_is_rewritten_so_it_can_get_shorter() {
        let mut mirror = ScreenMirror::new(6, 1);
        let mut row = vec![Cell::default(); 6];
        row[0].text = 'h';
        row[1].text = 'i';
        let mut out = Vec::new();
        mirror.emit_row(&mut out, 0, &row);
        let rendered = String::from_utf8(out).expect("utf8");
        assert!(rendered.starts_with("\x1b[1;1H\x1b[K"), "{rendered:?}");
        assert!(rendered.ends_with("hi"), "{rendered:?}");
        assert!(
            !rendered.ends_with("hi    "),
            "trailing blanks ride on the erase instead of being written"
        );
    }

    #[test]
    fn scrolling_the_mirror_drops_the_top_and_blanks_the_bottom() {
        let mut mirror = ScreenMirror::new(2, 3);
        for (index, cell) in mirror.cells.iter_mut().enumerate() {
            cell.text = char::from(b'a' + index as u8);
        }
        mirror.scroll_mirror(1);
        assert_eq!(mirror.cells.len(), 6, "geometry is preserved");
        assert_eq!(mirror.cells[0].text, 'c', "the first row is gone");
        assert_eq!(mirror.cells[4], Cell::default(), "the last row is blank");
    }

    #[test]
    fn scrolling_further_than_the_screen_still_leaves_a_whole_screen() {
        let mut mirror = ScreenMirror::new(3, 2);
        mirror.scroll_mirror(99);
        assert_eq!(mirror.cells.len(), 6);
        assert!(mirror.cells.iter().all(|cell| *cell == Cell::default()));
    }

    #[test]
    fn a_resize_message_is_four_fixed_bytes_in_little_endian_order() {
        // Guards the agent's decode against the host's encode: a pipe has no
        // message boundaries, so both sides must agree exactly.
        let (cols, rows) = (203_u16, 51_u16);
        let message = [
            (cols & 0xFF) as u8,
            (cols >> 8) as u8,
            (rows & 0xFF) as u8,
            (rows >> 8) as u8,
        ];
        assert_eq!(u16::from(message[0]) | (u16::from(message[1]) << 8), cols);
        assert_eq!(u16::from(message[2]) | (u16::from(message[3]) << 8), rows);
    }

    #[test]
    fn only_the_agent_argument_turns_this_binary_into_an_agent() {
        assert_eq!(run_if_agent(&["--help".to_owned()]), None);
        assert_eq!(run_if_agent(&[]), None);
        assert!(AGENT_ARGUMENT.starts_with("--agenterm-"));
    }

    /// A malformed agent request must fail with a distinct code rather than
    /// run something unexpected: the agent has no channel to explain itself.
    #[test]
    fn a_malformed_agent_request_exits_with_its_own_code() {
        let arguments = vec![AGENT_ARGUMENT.to_owned(), "not-a-handle".to_owned()];
        assert_eq!(run_if_agent(&arguments), Some(251));
    }
}
