//! Minimal Console-subsystem trampoline for the GUI-subsystem `agenterm.exe`.

#![cfg_attr(all(windows, not(test)), no_std)]
#![cfg_attr(all(windows, not(test)), no_main)]

#[cfg(all(windows, not(test)))]
mod windows_launcher {
    use core::{ffi::c_void, mem, panic::PanicInfo, ptr};

    type Bool = i32;
    type Dword = u32;
    type Handle = *mut c_void;

    const COMMAND_CAPACITY: usize = 32_768;
    const STD_ERROR_HANDLE: Dword = -12_i32 as Dword;
    const WAIT_FAILED: Dword = Dword::MAX;
    const AGENTERM_EXE: [u16; 13] = [
        b'a' as u16,
        b'g' as u16,
        b'e' as u16,
        b'n' as u16,
        b't' as u16,
        b'e' as u16,
        b'r' as u16,
        b'm' as u16,
        b'.' as u16,
        b'e' as u16,
        b'x' as u16,
        b'e' as u16,
        0,
    ];

    static mut PATH_BUFFER: [u16; COMMAND_CAPACITY] = [0; COMMAND_CAPACITY];
    static mut COMMAND_BUFFER: [u16; COMMAND_CAPACITY] = [0; COMMAND_CAPACITY];

    #[repr(C)]
    struct StartupInfoW {
        cb: Dword,
        lp_reserved: *mut u16,
        lp_desktop: *mut u16,
        lp_title: *mut u16,
        dw_x: Dword,
        dw_y: Dword,
        dw_x_size: Dword,
        dw_y_size: Dword,
        dw_x_count_chars: Dword,
        dw_y_count_chars: Dword,
        dw_fill_attribute: Dword,
        dw_flags: Dword,
        w_show_window: u16,
        cb_reserved_2: u16,
        lp_reserved_2: *mut u8,
        h_std_input: Handle,
        h_std_output: Handle,
        h_std_error: Handle,
    }

    #[repr(C)]
    struct ProcessInformation {
        process: Handle,
        thread: Handle,
        process_id: Dword,
        thread_id: Dword,
    }

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn CloseHandle(object: Handle) -> Bool;
        fn CreateProcessW(
            application_name: *const u16,
            command_line: *mut u16,
            process_attributes: *const c_void,
            thread_attributes: *const c_void,
            inherit_handles: Bool,
            creation_flags: Dword,
            environment: *const c_void,
            current_directory: *const u16,
            startup_info: *const StartupInfoW,
            process_information: *mut ProcessInformation,
        ) -> Bool;
        fn ExitProcess(exit_code: Dword) -> !;
        fn GetCommandLineW() -> *const u16;
        fn GetExitCodeProcess(process: Handle, exit_code: *mut Dword) -> Bool;
        fn GetModuleFileNameW(module: Handle, filename: *mut u16, size: Dword) -> Dword;
        fn GetStdHandle(std_handle: Dword) -> Handle;
        fn WaitForSingleObject(object: Handle, milliseconds: Dword) -> Dword;
        fn WriteFile(
            file: Handle,
            buffer: *const c_void,
            bytes_to_write: Dword,
            bytes_written: *mut Dword,
            overlapped: *mut c_void,
        ) -> Bool;
    }

    #[unsafe(no_mangle)]
    pub extern "system" fn mainCRTStartup() -> ! {
        let exit_code = unsafe { forward() };
        unsafe { ExitProcess(exit_code) }
    }

    unsafe fn forward() -> Dword {
        unsafe {
            let path = ptr::addr_of_mut!(PATH_BUFFER).cast::<u16>();
            let path_len = GetModuleFileNameW(ptr::null_mut(), path, COMMAND_CAPACITY as Dword);
            if path_len == 0 || path_len as usize >= COMMAND_CAPACITY {
                return fail();
            }

            let mut filename_start = 0;
            let mut index = 0;
            while index < path_len as usize {
                let unit = *path.add(index);
                if unit == b'\\' as u16 || unit == b'/' as u16 {
                    filename_start = index + 1;
                }
                index += 1;
            }
            if filename_start + AGENTERM_EXE.len() > COMMAND_CAPACITY {
                return fail();
            }
            index = 0;
            while index < AGENTERM_EXE.len() {
                *path.add(filename_start + index) = AGENTERM_EXE[index];
                index += 1;
            }
            let executable_len = filename_start + AGENTERM_EXE.len() - 1;

            let original = GetCommandLineW();
            if original.is_null() {
                return fail();
            }
            let mut tail = original;
            while *tail == b' ' as u16 || *tail == b'\t' as u16 {
                tail = tail.add(1);
            }
            if *tail == b'"' as u16 {
                tail = tail.add(1);
                while *tail != 0 && *tail != b'"' as u16 {
                    tail = tail.add(1);
                }
                if *tail == b'"' as u16 {
                    tail = tail.add(1);
                }
            } else {
                while *tail != 0 && *tail != b' ' as u16 && *tail != b'\t' as u16 {
                    tail = tail.add(1);
                }
            }

            let command = ptr::addr_of_mut!(COMMAND_BUFFER).cast::<u16>();
            let mut command_len = 0;
            *command.add(command_len) = b'"' as u16;
            command_len += 1;
            index = 0;
            while index < executable_len {
                *command.add(command_len) = *path.add(index);
                command_len += 1;
                index += 1;
            }
            *command.add(command_len) = b'"' as u16;
            command_len += 1;
            while *tail != 0 {
                if command_len + 1 >= COMMAND_CAPACITY {
                    return fail();
                }
                *command.add(command_len) = *tail;
                command_len += 1;
                tail = tail.add(1);
            }
            *command.add(command_len) = 0;

            let mut startup: StartupInfoW = mem::zeroed();
            startup.cb = mem::size_of::<StartupInfoW>() as Dword;
            let mut process: ProcessInformation = mem::zeroed();
            if CreateProcessW(
                path,
                command,
                ptr::null(),
                ptr::null(),
                1,
                0,
                ptr::null(),
                ptr::null(),
                &startup,
                &mut process,
            ) == 0
            {
                return fail();
            }

            let wait_result = WaitForSingleObject(process.process, Dword::MAX);
            let mut exit_code = 1;
            if wait_result != WAIT_FAILED {
                let _ = GetExitCodeProcess(process.process, &mut exit_code);
            }
            let _ = CloseHandle(process.thread);
            let _ = CloseHandle(process.process);
            exit_code
        }
    }

    unsafe fn fail() -> Dword {
        const MESSAGE: &[u8] = b"agenterm: could not start sibling agenterm.exe\r\n";
        unsafe {
            let stderr = GetStdHandle(STD_ERROR_HANDLE);
            let mut written = 0;
            let _ = WriteFile(
                stderr,
                MESSAGE.as_ptr().cast(),
                MESSAGE.len() as Dword,
                &mut written,
                ptr::null_mut(),
            );
        }
        1
    }

    #[panic_handler]
    fn panic(_info: &PanicInfo<'_>) -> ! {
        unsafe { ExitProcess(1) }
    }
}

#[cfg(any(not(windows), test))]
fn main() {
    use std::process::{Command, Stdio};

    #[cfg(windows)]
    const AGENTERM_EXECUTABLE: &str = "agenterm.exe";
    #[cfg(not(windows))]
    const AGENTERM_EXECUTABLE: &str = "agenterm";

    let mut executable = std::env::current_exe().unwrap_or_else(|error| {
        eprintln!("agenterm: could not resolve launcher path: {error}");
        std::process::exit(1);
    });
    executable.set_file_name(AGENTERM_EXECUTABLE);
    let status = Command::new(&executable)
        .args(std::env::args_os().skip(1))
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .unwrap_or_else(|error| {
            eprintln!(
                "agenterm: could not start {}: {error}",
                executable.display()
            );
            std::process::exit(1);
        });
    std::process::exit(status.code().unwrap_or(1));
}
