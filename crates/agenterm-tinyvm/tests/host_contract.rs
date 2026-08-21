use agenterm_tinyvm::{
    DescriptorRights, FileStat, FileType, GuestFd, HostBackend, HostClock, HostContext, HostError,
    HostHandle, HostLimits, HostResult, OpenOptions, SeekWhence,
};

#[derive(Default)]
struct FixtureBackend {
    opened: Option<(u32, String)>,
    closed: Vec<u32>,
    unlinked: Option<(u32, String)>,
    exit_code: Option<u32>,
}

impl HostBackend for FixtureBackend {
    fn clock_now(&mut self, clock: HostClock) -> HostResult<u64> {
        match clock {
            HostClock::Monotonic => Ok(42),
            _ => Err(HostError::NotSupported),
        }
    }

    fn sleep(&mut self, _duration_nanoseconds: u64) -> HostResult<()> {
        Err(HostError::NotSupported)
    }

    fn random_fill(&mut self, output: &mut [u8]) -> HostResult<()> {
        output.fill(7);
        Ok(())
    }

    fn fd_read(&mut self, handle: HostHandle, output: &mut [u8]) -> HostResult<usize> {
        let bytes = handle.raw().to_le_bytes();
        let count = output.len().min(bytes.len());
        output[..count].copy_from_slice(&bytes[..count]);
        Ok(count)
    }

    fn fd_write(&mut self, handle: HostHandle, input: &[u8]) -> HostResult<usize> {
        if handle.raw() == 900 {
            Ok(input.len())
        } else {
            Err(HostError::Io)
        }
    }

    fn fd_seek(&mut self, handle: HostHandle, offset: i64, _whence: SeekWhence) -> HostResult<u64> {
        Ok(u64::from(handle.raw()) + u64::try_from(offset).map_err(|_| HostError::Invalid)?)
    }

    fn fd_close(&mut self, handle: HostHandle) -> HostResult<()> {
        self.closed.push(handle.raw());
        Ok(())
    }

    fn fd_stat(&mut self, handle: HostHandle) -> HostResult<FileStat> {
        Ok(FileStat {
            file_type: FileType::RegularFile,
            size: u64::from(handle.raw()),
        })
    }

    fn path_open(
        &mut self,
        directory: HostHandle,
        path: &str,
        _options: OpenOptions,
    ) -> HostResult<HostHandle> {
        self.opened = Some((directory.raw(), path.to_owned()));
        Ok(HostHandle::new(900))
    }

    fn path_unlink(&mut self, directory: HostHandle, path: &str) -> HostResult<()> {
        self.unlinked = Some((directory.raw(), path.to_owned()));
        Ok(())
    }

    fn exit(&mut self, code: u32) -> HostResult<()> {
        self.exit_code = Some(code);
        Ok(())
    }
}

fn all_file_rights() -> DescriptorRights {
    DescriptorRights::READ
        .union(DescriptorRights::WRITE)
        .union(DescriptorRights::SEEK)
        .union(DescriptorRights::STAT)
}

#[test]
fn guest_descriptors_map_to_opaque_backend_handles_and_rights() {
    let mut host = HostContext::new(FixtureBackend::default(), HostLimits::default());
    let read = host
        .register_descriptor(HostHandle::new(0x7856_3412), DescriptorRights::READ)
        .expect("register read handle");
    let write = host
        .register_descriptor(HostHandle::new(900), DescriptorRights::WRITE)
        .expect("register write handle");
    assert_eq!(read.raw(), 0);
    assert_eq!(write.raw(), 1);

    let mut bytes = [0u8; 4];
    assert_eq!(host.fd_read(read, &mut bytes), Ok(4));
    assert_eq!(bytes, [0x12, 0x34, 0x56, 0x78]);
    assert_eq!(host.fd_write(read, b"x"), Err(HostError::NotCapable));
    assert_eq!(host.fd_write(write, b"abc"), Ok(3));
    assert_eq!(host.fd_close(read), Ok(()));
    assert_eq!(host.fd_read(read, &mut bytes), Err(HostError::BadHandle));
}

#[test]
fn preopens_keep_physical_paths_out_of_guest_space() {
    let mut host = HostContext::new(FixtureBackend::default(), HostLimits::default());
    let rights = all_file_rights()
        .union(DescriptorRights::PATH_OPEN)
        .union(DescriptorRights::PATH_UNLINK);
    let root = host
        .register_preopen(HostHandle::new(77), "/save".to_owned(), rights)
        .expect("register preopen");
    assert_eq!(host.preopen_name(root), Ok(Some("/save")));

    for path in ["", "/private/file", "../file", "dir/../file", "dir\\file"] {
        assert_eq!(
            host.path_open(root, path, OpenOptions::read_only(), DescriptorRights::READ),
            Err(HostError::InvalidPath)
        );
    }
    assert!(host.backend().opened.is_none());
    assert_eq!(
        host.path_open(
            root,
            "slot/state.bin",
            OpenOptions::read_only(),
            all_file_rights()
        ),
        Err(HostError::Invalid)
    );
    assert!(host.backend().opened.is_none());

    let file = host
        .path_open(
            root,
            "slot/state.bin",
            OpenOptions::read_write(),
            all_file_rights(),
        )
        .expect("open through virtual root");
    assert_eq!(
        host.backend().opened,
        Some((77, "slot/state.bin".to_owned()))
    );
    assert_eq!(host.fd_write(file, b"save"), Ok(4));
    assert_eq!(host.path_unlink(root, "slot/state.bin"), Ok(()));
    assert_eq!(
        host.backend().unlinked,
        Some((77, "slot/state.bin".to_owned()))
    );
}

#[test]
fn process_values_are_bounded_and_replaced_transactionally() {
    let limits = HostLimits {
        max_process_entries: 2,
        max_process_bytes: 12,
        ..HostLimits::default()
    };
    let mut host = HostContext::new(FixtureBackend::default(), limits);
    assert_eq!(
        host.set_process_values(vec!["game".to_owned()], vec!["A=B".to_owned()]),
        Ok(())
    );
    assert_eq!(host.args(), ["game"]);
    assert_eq!(host.environ(), ["A=B"]);

    assert_eq!(
        host.set_process_values(
            vec!["one".to_owned(), "two".to_owned()],
            vec!["three".to_owned()]
        ),
        Err(HostError::ProcessTooLarge)
    );
    assert_eq!(host.args(), ["game"]);
    assert_eq!(host.environ(), ["A=B"]);
}

#[test]
fn unsupported_platform_operations_fail_explicitly() {
    let mut host = HostContext::new(FixtureBackend::default(), HostLimits::default());
    assert_eq!(host.clock_now(HostClock::Monotonic), Ok(42));
    assert_eq!(
        host.clock_now(HostClock::Realtime),
        Err(HostError::NotSupported)
    );
    assert_eq!(host.sleep(1), Err(HostError::NotSupported));
    let mut random = [0u8; 3];
    assert_eq!(host.random_fill(&mut random), Ok(()));
    assert_eq!(random, [7, 7, 7]);
    assert_eq!(host.exit(19), Ok(()));
    assert_eq!(host.backend().exit_code, Some(19));
    assert_eq!(
        host.preopen_name(GuestFd::new(999)),
        Err(HostError::BadHandle)
    );
}

#[test]
fn descriptor_capacity_fails_before_opening_a_backend_handle() {
    let limits = HostLimits {
        max_descriptors: 1,
        ..HostLimits::default()
    };
    let mut host = HostContext::new(FixtureBackend::default(), limits);
    let root = host
        .register_preopen(
            HostHandle::new(77),
            "/save".to_owned(),
            DescriptorRights::PATH_OPEN.union(DescriptorRights::READ),
        )
        .expect("register only descriptor");
    assert_eq!(
        host.path_open(
            root,
            "slot.bin",
            OpenOptions::read_only(),
            DescriptorRights::READ
        ),
        Err(HostError::TooManyDescriptors)
    );
    assert!(host.backend().opened.is_none());
    assert!(host.backend().closed.is_empty());
}
