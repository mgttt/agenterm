//! Windows loader entry that retains Rust's generated `lang_start` wrapper.
//!
//! The Windows standard library ignores the C `argc`/`argv` pair and parses
//! `GetCommandLineW` itself. Calling rustc's generated `main` with an empty C
//! pair therefore preserves `std::env::args`, runtime initialization, panic
//! containment and cleanup without routing through the MSVC startup object.

#[cfg(target_arch = "x86_64")]
core::arch::global_asm!(
    ".globl agenterm_con_invoke_main",
    "agenterm_con_invoke_main:",
    "jmp main",
);

#[cfg(target_arch = "aarch64")]
core::arch::global_asm!(
    ".globl agenterm_con_invoke_main",
    "agenterm_con_invoke_main:",
    "b main",
);

#[cfg(windows)]
unsafe extern "C" {
    fn agenterm_con_invoke_main(argc: i32, argv: *const *const u8) -> i32;
}

#[cfg(windows)]
#[link(name = "kernel32")]
unsafe extern "system" {
    fn ExitProcess(exit_code: u32) -> !;
}

#[cfg(windows)]
unsafe extern "C" {
    /// Brings up the VC runtime's per-process state, including everything
    /// C++ exception handling depends on.
    ///
    /// This is normally called by `__scrt_common_main_seh`, which lives
    /// behind the MSVC startup object — the very thing `/ENTRY` replaces.
    /// It is *not* reachable through the `.CRT$XI*` table this file walks,
    /// so walking that table is not a substitute for it.
    ///
    /// It went unnoticed for as long as the VC runtime was a DLL: the
    /// import library resolves the same symbols, and `VCRUNTIME140.dll`
    /// initializes itself from `DllMain` at load time. Link the VC runtime
    /// statically and there is no `DllMain`, nobody initializes it, and the
    /// first panic dies at `STATUS_STACK_BUFFER_OVERRUN` instead of being
    /// caught — a failure that looks like a corrupt stack and is really a
    /// missing constructor.
    ///
    /// Returns zero on failure.
    fn __vcrt_initialize() -> i32;
}

type CInitializer = unsafe extern "C" fn() -> i32;
type Initializer = unsafe extern "C" fn();

#[used]
#[unsafe(link_section = ".CRT$XIA")]
static C_INITIALIZERS_START: Option<CInitializer> = None;
#[used]
#[unsafe(link_section = ".CRT$XIZ")]
static C_INITIALIZERS_END: Option<CInitializer> = None;
#[used]
#[unsafe(link_section = ".CRT$XCA")]
static INITIALIZERS_START: Option<Initializer> = None;
#[used]
#[unsafe(link_section = ".CRT$XCZ")]
static INITIALIZERS_END: Option<Initializer> = None;
#[used]
#[unsafe(link_section = ".CRT$XPA")]
static PRE_TERMINATORS_START: Option<Initializer> = None;
#[used]
#[unsafe(link_section = ".CRT$XPZ")]
static PRE_TERMINATORS_END: Option<Initializer> = None;
#[used]
#[unsafe(link_section = ".CRT$XTA")]
static TERMINATORS_START: Option<Initializer> = None;
#[used]
#[unsafe(link_section = ".CRT$XTZ")]
static TERMINATORS_END: Option<Initializer> = None;

#[cfg(test)]
static TEST_INITIALIZER_RAN: core::sync::atomic::AtomicBool =
    core::sync::atomic::AtomicBool::new(false);

#[cfg(test)]
unsafe extern "C" fn test_initializer() {
    TEST_INITIALIZER_RAN.store(true, core::sync::atomic::Ordering::Release);
}

#[cfg(test)]
#[used]
#[unsafe(link_section = ".CRT$XCU")]
static TEST_INITIALIZER: Option<Initializer> = Some(test_initializer);

fn run_c_initializers() -> i32 {
    let mut cursor = core::ptr::addr_of!(C_INITIALIZERS_START) as usize
        + core::mem::size_of::<Option<CInitializer>>();
    let end = core::ptr::addr_of!(C_INITIALIZERS_END) as usize;
    while cursor < end {
        // SAFETY: the PE linker merges and pointer-aligns `.CRT$XI*` entries
        // between the two referenced sentinels in lexical subsection order.
        if let Some(initialize) = unsafe { (cursor as *const Option<CInitializer>).read() } {
            // SAFETY: each non-null table entry has the MSVC C initializer ABI.
            let result = unsafe { initialize() };
            if result != 0 {
                return result;
            }
        }
        cursor += core::mem::size_of::<Option<CInitializer>>();
    }
    0
}

fn run_initializers(start: &'static Option<Initializer>, end: &'static Option<Initializer>) {
    let mut cursor =
        core::ptr::from_ref(start) as usize + core::mem::size_of::<Option<Initializer>>();
    let end = core::ptr::from_ref(end) as usize;
    while cursor < end {
        // SAFETY: the PE linker merges and pointer-aligns entries between the
        // matching lexical sentinels. XL TLS callbacks are outside these ranges.
        if let Some(initialize) = unsafe { (cursor as *const Option<Initializer>).read() } {
            // SAFETY: each non-null table entry has the MSVC initializer ABI.
            unsafe { initialize() };
        }
        cursor += core::mem::size_of::<Option<Initializer>>();
    }
}

#[cfg(windows)]
#[unsafe(no_mangle)]
pub extern "system" fn agenterm_con_entry() -> ! {
    // Before the `.CRT$XI*` walk: those initializers may already unwind, and
    // unwinding is one of the things this call makes work.
    // SAFETY: no arguments, called exactly once, first in the process.
    if unsafe { __vcrt_initialize() } == 0 {
        // SAFETY: without the VC runtime there is no exception handling, so
        // there is no way to report this except by not continuing.
        unsafe { ExitProcess(254) }
    }
    let initialization = run_c_initializers();
    if initialization != 0 {
        // SAFETY: failed static initialization cannot enter Rust main.
        unsafe { ExitProcess(255) }
    }
    run_initializers(&INITIALIZERS_START, &INITIALIZERS_END);
    // SAFETY: rustc emits `main` with this C ABI. Windows std ignores the C
    // argument pair and obtains the Unicode process command line natively.
    let exit_code = unsafe { agenterm_con_invoke_main(0, core::ptr::null()) };
    run_initializers(&PRE_TERMINATORS_START, &PRE_TERMINATORS_END);
    run_initializers(&TERMINATORS_START, &TERMINATORS_END);
    // SAFETY: `lang_start` has completed Rust runtime cleanup; returning from
    // a PE loader entry has no defined process-exit contract.
    unsafe { ExitProcess(exit_code as u32) }
}

#[cfg(test)]
mod tests {
    #[test]
    fn custom_entry_runs_crt_constructors_before_rust_main() {
        assert!(super::TEST_INITIALIZER_RAN.load(core::sync::atomic::Ordering::Acquire));
    }
}
