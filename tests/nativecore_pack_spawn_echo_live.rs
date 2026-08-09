//! Black-box proof for `plan/design-dynacore-native-core.md` §7: a
//! nativecore pack, loaded and run through the REAL product entry points
//! (`agenterm::script_nativecore_pack::cached_nativecore_pack` /
//! `agenterm::script_backend::try_execute_nativecore_pack_invocation` — the
//! exact two calls `script_worker.rs::execute_inner` makes), spawns and
//! waits on a REAL child process through `agenterm-nativecore`'s `seam.rs`
//! (real Win32 `CreateProcessA`/`WaitForSingleObject`/`GetExitCodeProcess`),
//! and the real exit code round-trips back through the product path.
//!
//! Not a subprocess of the *test itself* (no `agenterm --framed-worker`
//! child spawned here): nativecore has no fleet bridge to reach through a
//! server (§7.1 — "没有 bridge 要穿过去"), so calling the product functions
//! directly IS the product path — the exact reasoning
//! `script_worker.rs::dynacore_in_process_broker_tests`'s own header gives
//! for calling `script_dynacore_host::verify_pack`/`run_pack` directly
//! instead of going through a spawned `agenterm` binary.
//!
//! A single `#[test]` in this file, deliberately:
//! `cached_nativecore_pack()` is a process-wide `OnceLock`, pinned by the
//! first call in this process (`script_nativecore_pack.rs`'s own doc) —
//! `cargo test` runs every `#[test]` in one integration-test file inside ONE
//! process, so a second test setting different env vars in this same file
//! could not reliably observe them. `nativecore_pack_bad_contract_rejected.rs`
//! is the sibling file for the second acceptance scenario, for the same
//! reason.

use agenterm::script_backend::try_execute_nativecore_pack_invocation;
use agenterm::script_nativecore_pack::cached_nativecore_pack;
use agenterm::script_protocol::ScriptOperation;
use agenterm_nativecore::{pack, payloads, store::Store};

#[test]
fn spawn_echo_pack_spawns_and_waits_on_a_real_child_process_through_the_product_path() {
    let dir = std::env::temp_dir().join(format!(
        "agenterm-nativecore-pack-live-spawn-echo-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system time")
            .as_nanos()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    let store = Store::open(&dir).expect("open a real content-addressed store on disk");
    let module = payloads::spawn_echo();
    let manifest = pack::pack(&store, &module).expect("pack spawn_echo into the real store");

    unsafe {
        std::env::set_var("AGENTERM_NATIVECORE_PACK_STORE", &dir);
        std::env::set_var("AGENTERM_NATIVECORE_PACK_HASH", &manifest.hash);
    }

    assert!(
        cached_nativecore_pack().is_some(),
        "the product loader must accept a real, well-formed spawn_echo pack from a real store"
    );

    let (outcome, stdout) = capture_real_stdout(|| {
        try_execute_nativecore_pack_invocation(ScriptOperation::Run)
            .expect("spawn_echo must verify and run through the product path")
            .expect("a configured pack must return Some(_), not None")
    });

    assert_eq!(
        outcome.value,
        Some(serde_json::Value::from(7u64)),
        "a real cmd.exe child spawned via CreateProcessA must exit 7, and a real \
         GetExitCodeProcess must read that exit code back through the product path"
    );
    assert!(
        stdout.contains("exit=07"),
        "expected the real child's exit code to be visible from a real WriteFile to the \
         real process stdout, got {stdout:?}"
    );

    unsafe {
        std::env::remove_var("AGENTERM_NATIVECORE_PACK_STORE");
        std::env::remove_var("AGENTERM_NATIVECORE_PACK_HASH");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// Same technique as `agenterm-nativecore/tests/native_intents.rs`'s own
/// `capture_real_stdout`: swap the REAL process `STD_OUTPUT_HANDLE` to a
/// pipe (Win32 `SetStdHandle`) so this test can see what `seam.rs`'s raw
/// `WriteFile` calls actually wrote, without relying on Rust's
/// `print!`/`io::stdout()` capturing — which never observes these writes at
/// all, since they bypass Rust's stdio layer entirely and go straight to the
/// OS handle.
#[cfg(windows)]
fn capture_real_stdout<T>(f: impl FnOnce() -> T) -> (T, String) {
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetStdHandle(which: u32) -> *mut u8;
        fn SetStdHandle(which: u32, h: *mut u8) -> i32;
        fn CreatePipe(read_h: *mut *mut u8, write_h: *mut *mut u8, sa: *mut u8, size: u32) -> i32;
        fn ReadFile(h: *mut u8, buf: *mut u8, n: u32, got: *mut u32, ov: *mut u8) -> i32;
        fn CloseHandle(h: *mut u8) -> i32;
    }
    const STD_OUTPUT_HANDLE: u32 = 0xFFFF_FFF5; // -11 as u32
    unsafe {
        let mut read_h: *mut u8 = std::ptr::null_mut();
        let mut write_h: *mut u8 = std::ptr::null_mut();
        assert_ne!(
            CreatePipe(&mut read_h, &mut write_h, std::ptr::null_mut(), 0),
            0,
            "CreatePipe failed"
        );
        let original = GetStdHandle(STD_OUTPUT_HANDLE);
        assert_ne!(SetStdHandle(STD_OUTPUT_HANDLE, write_h), 0, "SetStdHandle failed");

        let result = f();

        SetStdHandle(STD_OUTPUT_HANDLE, original);
        CloseHandle(write_h); // our copy must close so ReadFile below sees EOF

        let mut collected = Vec::new();
        let mut buf = [0u8; 4096];
        loop {
            let mut got: u32 = 0;
            let r = ReadFile(
                read_h,
                buf.as_mut_ptr(),
                buf.len() as u32,
                &mut got,
                std::ptr::null_mut(),
            );
            if r == 0 || got == 0 {
                break;
            }
            collected.extend_from_slice(&buf[..got as usize]);
        }
        CloseHandle(read_h);
        (result, String::from_utf8_lossy(&collected).to_string())
    }
}
