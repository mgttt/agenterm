#![cfg(feature = "shared-memory")]

use std::process::Command;

use agenterm_platform::shared_memory::{SharedMemory, SharedMemoryErrorKind};

const CHILD_MODE: &str = "AGENTERM_PLATFORM_SHARED_MEMORY_CHILD";
const MAPPING_NAME: &str = "AGENTERM_PLATFORM_SHARED_MEMORY_NAME";
const MAPPING_LEN: usize = 4096;

#[test]
fn named_mapping_is_cross_process_and_released() {
    if std::env::var_os(CHILD_MODE).is_some() {
        let name = std::env::var(MAPPING_NAME).expect("child mapping name");
        let mut mapping = SharedMemory::open(&name, MAPPING_LEN).expect("child opens mapping");
        // SAFETY: the parent reserves the first u64 for this child and waits
        // before reading it, so no concurrent Rust reference exists.
        unsafe { mapping.as_mut_ptr().cast::<u64>().write(0xfeed_beef) };
        return;
    }

    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system clock after Unix epoch")
        .as_nanos();
    let name = format!(
        "apm-{:08x}-{:012x}",
        std::process::id(),
        nonce % 0x1_0000_0000_0000
    );
    let mapping = SharedMemory::create(&name, MAPPING_LEN).expect("parent creates mapping");
    let status = Command::new(std::env::current_exe().expect("integration test executable"))
        .args(["--exact", "named_mapping_is_cross_process_and_released"])
        .env(CHILD_MODE, "write")
        .env(MAPPING_NAME, &name)
        .status()
        .expect("spawn shared-memory child");
    assert!(status.success(), "shared-memory child failed: {status}");
    // SAFETY: the child exited before this read and the view covers one u64.
    assert_eq!(
        unsafe { mapping.as_ptr().cast::<u64>().read() },
        0xfeed_beef
    );
    drop(mapping);
    let error = SharedMemory::open(&name, MAPPING_LEN).expect_err("creator removes native name");
    assert_eq!(error.kind(), SharedMemoryErrorKind::NotFound);
}
