//! Linux spawn adapter — OUT of the kernel. Composes the kernel's ③ raw_syscall
//! primitive into "start a child, wait for it, return its exit code". No new
//! kernel primitive is introduced: fork/execve/wait4 are ordinary syscalls, the
//! exact same ③ path the file adapter already uses. This is the SECOND capability
//! (criterion ④); note it needs nothing from the kernel that ① file I/O didn't.
//!
//! NOTE (execution): this adapter is byte-measured but NOT executed on this host
//! (no WSL). The syscall numbers and fork/exec/wait shape are standard x86_64
//! Linux; correctness of the *mechanism* (load+call through the primitive table)
//! is proven on the Windows side.

use crate::abi::Kernel;

const SYS_WRITE: usize = 1;
const SYS_FORK: usize = 57;
const SYS_EXECVE: usize = 59;
const SYS_EXIT: usize = 60;
const SYS_WAIT4: usize = 61;
const STDOUT: usize = 1;

/// Spawn `/bin/sh -c "exit 7"`, wait, and return the child's exit status.
/// Everything routes through ③ raw_syscall — the same primitive read_file uses.
pub fn spawn_wait(k: &Kernel) -> i32 {
    // argv / the command are fixed byte literals (the experiment measures bytes,
    // not argv parsing). All strings are NUL-terminated.
    let sh = b"/bin/sh\0";
    let dash_c = b"-c\0";
    let script = b"exit 7\0";
    let argv: [usize; 4] = [
        sh.as_ptr() as usize,
        dash_c.as_ptr() as usize,
        script.as_ptr() as usize,
        0,
    ];
    let envp: [usize; 1] = [0];

    let pid = (k.raw_syscall)(SYS_FORK, 0, 0, 0, 0, 0, 0);
    if pid == 0 {
        // Child: replace image. On success execve never returns.
        (k.raw_syscall)(
            SYS_EXECVE,
            sh.as_ptr() as usize,
            argv.as_ptr() as usize,
            envp.as_ptr() as usize,
            0,
            0,
            0,
        );
        // execve failed — exit non-zero so the parent observes an error code.
        (k.raw_syscall)(SYS_EXIT, 127, 0, 0, 0, 0, 0);
        loop {}
    }
    // Parent: wait4(pid, &status, 0, 0), then WEXITSTATUS(status) = (status>>8)&0xff.
    let mut status: i32 = 0;
    (k.raw_syscall)(
        SYS_WAIT4,
        pid as usize,
        core::ptr::addr_of_mut!(status) as usize,
        0,
        0,
        0,
        0,
    );
    (status >> 8) & 0xff
}

/// Write `len` bytes to stdout (so the payload can print the child's exit code).
pub fn write_stdout(k: &Kernel, buf: *const u8, len: usize) {
    (k.raw_syscall)(SYS_WRITE, STDOUT, buf as usize, len, 0, 0, 0);
}
