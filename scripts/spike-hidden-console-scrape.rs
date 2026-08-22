//! Spike: the pre-ConPTY terminal mechanism, proven in one process.
//!
//! Windows Server 2016 (build 14393) has no ConPTY, and Microsoft's ConPTY
//! redistributable does not lower that floor -- it supports 10.0.17763.0 and
//! above, the same as the in-box API. The only route left is winpty's: own a
//! hidden console, spawn the child onto it, and poll the screen buffer.
//!
//! This file is the feasibility evidence for that plan, kept because it also
//! records the three traps that cost a cycle each. Build and run it directly:
//!
//!     rustc -O scripts/spike-hidden-console-scrape.rs -o spike.exe && ./spike.exe
//!
//! It writes its report to dist/conprobe.txt. Not part of the build; not a
//! test. See plan-v0.1.18 CON-oldpty and skills/windows-binary-portability.

#![allow(non_snake_case)]
use std::ffi::c_void;

type Handle = *mut c_void;
#[repr(C)] #[derive(Clone, Copy, Default)] struct Coord { x: i16, y: i16 }
#[repr(C)] #[derive(Clone, Copy, Default)] struct SmallRect { left: i16, top: i16, right: i16, bottom: i16 }
#[repr(C)] #[derive(Clone, Copy, Default)] struct CharInfo { unicode: u16, attributes: u16 }
#[repr(C)] #[derive(Clone, Copy, Default)]
struct ScreenBufferInfo { size: Coord, cursor: Coord, attributes: u16, window: SmallRect, max_window: Coord }
#[repr(C)] struct StartupInfoW {
    cb: u32, reserved: *mut u16, desktop: *mut u16, title: *mut u16,
    x: u32, y: u32, x_size: u32, y_size: u32, x_chars: u32, y_chars: u32,
    fill_attribute: u32, flags: u32, show_window: u16, cb_reserved2: u16,
    lp_reserved2: *mut u8, std_input: Handle, std_output: Handle, std_error: Handle,
}
#[repr(C)] struct SecurityAttributes { length: u32, descriptor: *mut c_void, inherit: i32 }
#[repr(C)] struct ProcessInformation { process: Handle, thread: Handle, pid: u32, tid: u32 }

#[link(name = "kernel32")]
unsafe extern "system" {
    fn FreeConsole() -> i32;
    fn AllocConsole() -> i32;
    fn GetConsoleWindow() -> Handle;
    fn CreateFileW(name: *const u16, access: u32, share: u32, sa: *const c_void, disp: u32, flags: u32, template: Handle) -> Handle;
    fn SetConsoleScreenBufferSize(h: Handle, size: Coord) -> i32;
    fn GetConsoleScreenBufferInfo(h: Handle, info: *mut ScreenBufferInfo) -> i32;
    fn ReadConsoleOutputW(h: Handle, buf: *mut CharInfo, size: Coord, origin: Coord, region: *mut SmallRect) -> i32;
    fn CreateProcessW(app: *const u16, cmd: *mut u16, pa: *const c_void, ta: *const c_void,
        inherit: i32, flags: u32, env: *const c_void, dir: *const u16,
        si: *const StartupInfoW, pi: *mut ProcessInformation) -> i32;
    fn WaitForSingleObject(h: Handle, ms: u32) -> u32;
    fn GetLastError() -> u32;
}
#[link(name = "user32")]
unsafe extern "system" { fn ShowWindow(h: Handle, cmd: i32) -> i32; }

fn wide(s: &str) -> Vec<u16> { s.encode_utf16().chain(Some(0)).collect() }

fn main() {
    let mut report = String::new();
    unsafe {
        // Own a fresh console and hide it. This is what the agent process does.
        FreeConsole();
        if AllocConsole() == 0 {
            println!("AllocConsole failed: {}", GetLastError());
            return;
        }
        let hwnd = GetConsoleWindow();
        report.push_str(&format!("console window: {:?} hidden={}\n", hwnd, ShowWindow(hwnd, 0) != 0 || true));

        // GetStdHandle is wrong here: this process was started with redirected
        // stdio, so it still returns the pipe rather than the new console.
        // CONOUT$ / CONIN$ name the attached console directly.
        // Inheritable, or the child receives closed handles and paints nowhere.
        let sa = SecurityAttributes { length: size_of::<SecurityAttributes>() as u32, descriptor: std::ptr::null_mut(), inherit: 1 };
        let sa_ptr = (&raw const sa).cast::<c_void>();
        let conout = CreateFileW(wide("CONOUT$").as_ptr(), 0xC000_0000, 3, sa_ptr, 3, 0, std::ptr::null_mut());
        let conin = CreateFileW(wide("CONIN$").as_ptr(), 0xC000_0000, 3, sa_ptr, 3, 0, std::ptr::null_mut());
        report.push_str(&format!("CONOUT$={:?} CONIN$={:?} err={}
", conout, conin, GetLastError()));
        let out = conout;
        let _ = SetConsoleScreenBufferSize(out, Coord { x: 80, y: 40 });

        // Run a child attached to this same console.
        let mut cmd = wide("cmd.exe /c echo AGENT_PROBE_LINE && ver");
        let si = StartupInfoW {
            cb: size_of::<StartupInfoW>() as u32, reserved: std::ptr::null_mut(),
            desktop: std::ptr::null_mut(), title: std::ptr::null_mut(),
            x: 0, y: 0, x_size: 0, y_size: 0, x_chars: 0, y_chars: 0,
            fill_attribute: 0, flags: 0x0000_0100 /* STARTF_USESTDHANDLES */, show_window: 0, cb_reserved2: 0,
            lp_reserved2: std::ptr::null_mut(), std_input: conin,
            std_output: conout, std_error: conout,
        };
        let mut pi = ProcessInformation { process: std::ptr::null_mut(), thread: std::ptr::null_mut(), pid: 0, tid: 0 };
        let spawned = CreateProcessW(std::ptr::null(), cmd.as_mut_ptr(), std::ptr::null(),
            std::ptr::null(), 1, 0, std::ptr::null(), std::ptr::null(), &si, &mut pi);
        report.push_str(&format!("spawned={spawned} pid={} err={}\n", pi.pid, GetLastError()));
        if spawned != 0 { WaitForSingleObject(pi.process, 5000); }

        // Scrape what the child painted.
        let mut info = ScreenBufferInfo::default();
        let got_info = GetConsoleScreenBufferInfo(out, &mut info);
        report.push_str(&format!("info={got_info} buffer={}x{} cursor=({},{})\n",
            info.size.x, info.size.y, info.cursor.x, info.cursor.y));

        let w = info.size.x.max(1) as usize;
        let rows = (info.cursor.y + 2).clamp(1, 12) as usize;
        let mut cells = vec![CharInfo::default(); w * rows];
        let mut region = SmallRect { left: 0, top: 0, right: (w - 1) as i16, bottom: (rows - 1) as i16 };
        let read = ReadConsoleOutputW(out, cells.as_mut_ptr(),
            Coord { x: w as i16, y: rows as i16 }, Coord { x: 0, y: 0 }, &mut region);
        report.push_str(&format!("ReadConsoleOutputW={read} err={}\n--- scraped ---\n", GetLastError()));
        for row in 0..rows {
            let line: String = cells[row * w..(row + 1) * w].iter()
                .map(|c| char::from_u32(c.unicode as u32).unwrap_or(' ')).collect();
            let line = line.trim_end();
            if !line.is_empty() { report.push_str(&format!("[{row:2}] {line}\n")); }
        }
        let attrs: Vec<u16> = cells.iter().take(8).map(|c| c.attributes).collect();
        report.push_str(&format!("--- first 8 attributes: {attrs:?}\n"));
    }
    let _ = std::fs::write(r"D:\dev\agenterm\dist\conprobe.txt", &report);
}
