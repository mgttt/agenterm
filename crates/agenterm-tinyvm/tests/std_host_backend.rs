#![cfg(feature = "std-host")]

use agenterm_tinyvm::{
    DescriptorRights, FileType, HostBackend, HostClock, HostContext, HostError, HostLimits,
    OpenOptions, SeekWhence, StdHostBackend, StdHostLimits,
};
#[cfg(feature = "wasi-p1")]
use agenterm_tinyvm::{Val, WasiPreview1, WasmModule};

fn file_rights() -> DescriptorRights {
    DescriptorRights::READ
        .union(DescriptorRights::WRITE)
        .union(DescriptorRights::SEEK)
        .union(DescriptorRights::STAT)
}

#[test]
fn std_host_preopen_drives_a_real_bounded_file_lifecycle() {
    let directory = tempfile::tempdir().expect("temporary preopen");
    let outside_directory = tempfile::tempdir().expect("separate outside directory");
    let outside = outside_directory.path().join("outside.txt");
    std::fs::write(&outside, b"outside").expect("write outside sentinel");
    let outside_name = outside_directory
        .path()
        .file_name()
        .and_then(|name| name.to_str())
        .expect("outside directory name");
    let escape = format!("../{outside_name}/outside.txt");

    let mut backend = StdHostBackend::default();
    let native_root = backend
        .open_ambient_preopen(directory.path())
        .expect("open ambient directory once");
    assert!(
        backend
            .path_open(native_root, &escape, OpenOptions::read_write())
            .is_err()
    );
    assert_eq!(
        std::fs::read(&outside).expect("read outside sentinel"),
        b"outside"
    );

    let rights = file_rights()
        .union(DescriptorRights::PATH_OPEN)
        .union(DescriptorRights::PATH_UNLINK);
    let mut host = HostContext::new(backend, HostLimits::default());
    let root = host
        .register_preopen(native_root, "/save".to_owned(), rights)
        .expect("register virtual preopen");
    let file = host
        .path_open(
            root,
            "slot.bin",
            OpenOptions {
                create: true,
                directory: false,
                read: true,
                truncate: true,
                write: true,
            },
            file_rights(),
        )
        .expect("create through preopen");

    assert_eq!(host.fd_write(file, b"hello"), Ok(5));
    assert_eq!(host.fd_seek(file, 0, SeekWhence::Start), Ok(0));
    let mut bytes = [0u8; 5];
    assert_eq!(host.fd_read(file, &mut bytes), Ok(5));
    assert_eq!(&bytes, b"hello");
    let stat = host.fd_stat(file).expect("stat opened file");
    assert!(stat.file_type == FileType::RegularFile);
    assert_eq!(stat.size, 5);
    assert_eq!(host.fd_close(file), Ok(()));
    assert_eq!(
        std::fs::read(directory.path().join("slot.bin")).expect("read created file"),
        b"hello"
    );
    assert_eq!(host.path_unlink(root, "slot.bin"), Ok(()));
    assert!(!directory.path().join("slot.bin").exists());
}

#[test]
fn std_host_clocks_random_exit_and_handle_limit_are_explicit() {
    let directory = tempfile::tempdir().expect("temporary preopen");
    let mut backend = StdHostBackend::new(StdHostLimits { max_handles: 1 });
    let root = backend
        .open_ambient_preopen(directory.path())
        .expect("first handle");
    assert_eq!(
        backend.open_ambient_preopen(directory.path()),
        Err(HostError::TooManyDescriptors)
    );
    assert!(backend.clock_now(HostClock::Realtime).expect("realtime") > 0);
    let before = backend
        .clock_now(HostClock::Monotonic)
        .expect("monotonic before");
    backend.sleep(1).expect("bounded sleep");
    let after = backend
        .clock_now(HostClock::Monotonic)
        .expect("monotonic after");
    assert!(after >= before);
    assert_eq!(
        backend.clock_now(HostClock::ProcessCpu),
        Err(HostError::NotSupported)
    );
    let mut random = [0u8; 32];
    backend.random_fill(&mut random).expect("system random");
    assert_eq!(backend.exit(23), Ok(()));
    assert_eq!(backend.exit_code(), Some(23));
    assert_eq!(backend.take_exit_code(), Some(23));
    assert_eq!(backend.exit_code(), None);
    assert_eq!(backend.fd_close(root), Ok(()));
}

#[test]
#[cfg(feature = "wasi-p1")]
fn standard_wasi_module_reaches_the_real_preopen_backend() {
    let directory = tempfile::tempdir().expect("temporary WASI preopen");
    let mut backend = StdHostBackend::default();
    let native_root = backend
        .open_ambient_preopen(directory.path())
        .expect("open ambient directory once");
    let rights = file_rights()
        .union(DescriptorRights::PATH_OPEN)
        .union(DescriptorRights::PATH_UNLINK);
    let mut context = HostContext::new(backend, HostLimits::default());
    context
        .register_preopen(native_root, "/save".to_owned(), rights)
        .expect("register virtual preopen");
    let wasi = WasiPreview1::new(context);
    let wasm = wat::parse_str(
        r#"(module
          (import "wasi_snapshot_preview1" "path_open"
            (func $path_open (param i32 i32 i32 i32 i32 i64 i64 i32 i32) (result i32)))
          (import "wasi_snapshot_preview1" "fd_write"
            (func $fd_write (param i32 i32 i32 i32) (result i32)))
          (import "wasi_snapshot_preview1" "fd_seek"
            (func $fd_seek (param i32 i64 i32 i32) (result i32)))
          (import "wasi_snapshot_preview1" "fd_read"
            (func $fd_read (param i32 i32 i32 i32) (result i32)))
          (import "wasi_snapshot_preview1" "fd_filestat_get"
            (func $fd_filestat_get (param i32 i32) (result i32)))
          (import "wasi_snapshot_preview1" "fd_close"
            (func $fd_close (param i32) (result i32)))
          (import "wasi_snapshot_preview1" "path_unlink_file"
            (func $path_unlink_file (param i32 i32 i32) (result i32)))
          (memory 1)
          (data (i32.const 0) "\60\00\00\00\05\00\00\00\68\00\00\00\05\00\00\00")
          (data (i32.const 64) "slot.bin")
          (data (i32.const 96) "hello")
          (func (export "main") (result i32)
            (local $fd i32)
            (drop (call $path_open
              (i32.const 0) (i32.const 0) (i32.const 64) (i32.const 8)
              (i32.const 1) (i64.const 2097222) (i64.const 0)
              (i32.const 0) (i32.const 32)))
            (local.set $fd (i32.load (i32.const 32)))
            (drop (call $fd_write
              (local.get $fd) (i32.const 0) (i32.const 1) (i32.const 36)))
            (drop (call $fd_seek
              (local.get $fd) (i64.const 0) (i32.const 0) (i32.const 40)))
            (drop (call $fd_read
              (local.get $fd) (i32.const 8) (i32.const 1) (i32.const 48)))
            (drop (call $fd_filestat_get (local.get $fd) (i32.const 128)))
            (drop (call $fd_close (local.get $fd)))
            (call $path_unlink_file (i32.const 0) (i32.const 64) (i32.const 8))))"#,
    )
    .expect("compile independent WAT fixture");
    let mut module = must(WasmModule::from_bytes(&wasm), "decode WASI fixture");
    must(wasi.bind(&mut module), "bind WASI subset");
    let mut instance = must(module.instantiate(), "instantiate WASI fixture");
    let result = must(instance.invoke_by_name("main", &[]), "run WASI fixture");
    assert!(matches!(result.as_slice(), [Val::I32(0)]));
    let memory = must(instance.memory(), "guest memory");
    assert_eq!(u32_at(&memory, 32), 1);
    assert_eq!(u32_at(&memory, 36), 5);
    assert_eq!(u64_at(&memory, 40), 0);
    assert_eq!(u32_at(&memory, 48), 5);
    assert_eq!(&memory[104..109], b"hello");
    assert_eq!(memory[128 + 16], 4);
    assert_eq!(u64_at(&memory, 128 + 32), 5);
    assert!(!directory.path().join("slot.bin").exists());
}

#[cfg(feature = "wasi-p1")]
fn u32_at(memory: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(memory[offset..offset + 4].try_into().expect("u32 bytes"))
}

#[cfg(feature = "wasi-p1")]
fn u64_at(memory: &[u8], offset: usize) -> u64 {
    u64::from_le_bytes(memory[offset..offset + 8].try_into().expect("u64 bytes"))
}

#[cfg(feature = "wasi-p1")]
fn must<T, E>(result: Result<T, E>, context: &str) -> T {
    match result {
        Ok(value) => value,
        Err(_) => panic!("{context}"),
    }
}
