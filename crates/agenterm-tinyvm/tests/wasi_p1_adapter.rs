#![cfg(feature = "wasi-p1")]

use agenterm_tinyvm::{
    DescriptorRights, FileStat, FileType, HostBackend, HostClock, HostContext, HostError,
    HostHandle, HostLimits, HostResult, OpenOptions, SeekWhence, Val, WasiPreview1, WasmModule,
};

#[derive(Default)]
struct FixtureBackend {
    closed: Vec<u32>,
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

    fn fd_read(&mut self, _handle: HostHandle, _output: &mut [u8]) -> HostResult<usize> {
        Err(HostError::NotSupported)
    }

    fn fd_write(&mut self, _handle: HostHandle, _input: &[u8]) -> HostResult<usize> {
        Err(HostError::NotSupported)
    }

    fn fd_seek(
        &mut self,
        _handle: HostHandle,
        _offset: i64,
        _whence: SeekWhence,
    ) -> HostResult<u64> {
        Err(HostError::NotSupported)
    }

    fn fd_close(&mut self, handle: HostHandle) -> HostResult<()> {
        self.closed.push(handle.raw());
        Ok(())
    }

    fn fd_stat(&mut self, _handle: HostHandle) -> HostResult<FileStat> {
        Ok(FileStat {
            file_type: FileType::Directory,
            size: 0,
        })
    }

    fn path_open(
        &mut self,
        _directory: HostHandle,
        _path: &str,
        _options: OpenOptions,
    ) -> HostResult<HostHandle> {
        Err(HostError::NotSupported)
    }

    fn path_unlink(&mut self, _directory: HostHandle, _path: &str) -> HostResult<()> {
        Err(HostError::NotSupported)
    }

    fn exit(&mut self, _code: u32) -> HostResult<()> {
        Err(HostError::NotSupported)
    }
}

#[test]
fn wasi_p1_process_clock_random_preopen_and_close_execute_through_standard_imports() {
    let mut context = HostContext::new(FixtureBackend::default(), HostLimits::default());
    context
        .set_process_values(
            vec!["demo".to_owned(), "--x".to_owned()],
            vec!["A=B".to_owned()],
        )
        .expect("set process values");
    context
        .register_preopen(
            HostHandle::new(77),
            "/save".to_owned(),
            DescriptorRights::PATH_OPEN,
        )
        .expect("register preopen");
    let wasi = WasiPreview1::new(context);
    let mut module = must(WasmModule::from_bytes(&fixture_module()), "decode fixture");
    must(wasi.bind(&mut module), "bind exact WASI imports");
    let mut instance = must(module.instantiate(), "instantiate fixture");
    let results = must(instance.invoke_by_name("main", &[]), "invoke main");
    assert!(matches!(results.as_slice(), [Val::I32(0)]));

    let memory = must(instance.memory(), "memory");
    assert_eq!(u32_at(&memory, 0), 2);
    assert_eq!(u32_at(&memory, 4), 9);
    assert_eq!(u32_at(&memory, 8), 32);
    assert_eq!(u32_at(&memory, 12), 37);
    assert_eq!(&memory[32..41], b"demo\0--x\0");
    assert_eq!(u32_at(&memory, 80), 1);
    assert_eq!(u32_at(&memory, 84), 4);
    assert_eq!(u32_at(&memory, 88), 96);
    assert_eq!(&memory[96..100], b"A=B\0");
    assert_eq!(u64_at(&memory, 128), 42);
    assert_eq!(&memory[136..140], &[7, 7, 7, 7]);
    assert_eq!(u32_at(&memory, 148), 5);
    assert_eq!(&memory[152..157], b"/save");
    drop(memory);

    let context = wasi.try_context().expect("borrow context after call");
    assert_eq!(context.backend().closed, [77]);
}

#[test]
fn wasi_p1_rejects_unknown_or_wrongly_typed_imports_before_instantiation() {
    let wasi = WasiPreview1::new(HostContext::new(
        FixtureBackend::default(),
        HostLimits::default(),
    ));
    let mut unknown = must(
        WasmModule::from_bytes(&single_import_module("sock_open", 0)),
        "decode unknown fixture",
    );
    assert!(wasi.bind(&mut unknown).is_err());

    let mut wrong = must(
        WasmModule::from_bytes(&single_import_module("random_get", 1)),
        "decode wrong-signature fixture",
    );
    assert!(wasi.bind(&mut wrong).is_err());
}

fn fixture_module() -> Vec<u8> {
    let types = vec![
        function_type(&[0x7f, 0x7f], &[0x7f]),
        function_type(&[0x7f, 0x7e, 0x7f], &[0x7f]),
        function_type(&[0x7f, 0x7f, 0x7f], &[0x7f]),
        function_type(&[0x7f], &[0x7f]),
        function_type(&[], &[0x7f]),
    ];
    let imports = [
        ("args_sizes_get", 0),
        ("args_get", 0),
        ("environ_sizes_get", 0),
        ("environ_get", 0),
        ("clock_time_get", 1),
        ("random_get", 0),
        ("fd_prestat_get", 0),
        ("fd_prestat_dir_name", 2),
        ("fd_close", 3),
    ];

    let mut body = vec![0];
    call2(&mut body, 0, 0, 4);
    call2(&mut body, 1, 8, 32);
    call2(&mut body, 2, 80, 84);
    call2(&mut body, 3, 88, 96);
    i32_const(&mut body, 1);
    body.extend_from_slice(&[0x42, 0x00]);
    i32_const(&mut body, 128);
    call_drop(&mut body, 4);
    call2(&mut body, 5, 136, 4);
    call2(&mut body, 6, 0, 144);
    i32_const(&mut body, 0);
    i32_const(&mut body, 152);
    i32_const(&mut body, 5);
    call_drop(&mut body, 7);
    i32_const(&mut body, 0);
    call(&mut body, 8);
    body.push(0x0b);

    module(types, &imports, 4, Some(body))
}

fn single_import_module(field: &str, type_index: u32) -> Vec<u8> {
    let types = vec![
        function_type(&[0x7f, 0x7f], &[0x7f]),
        function_type(&[0x7f], &[0x7f]),
        function_type(&[], &[0x7f]),
    ];
    module(
        types,
        &[(field, type_index)],
        2,
        Some(vec![0, 0x41, 0, 0x0b]),
    )
}

fn module(
    types: Vec<Vec<u8>>,
    imports: &[(&str, u32)],
    main_type: u32,
    body: Option<Vec<u8>>,
) -> Vec<u8> {
    let mut wasm = b"\0asm\x01\0\0\0".to_vec();
    let mut type_payload = Vec::new();
    u32_leb(types.len() as u32, &mut type_payload);
    for ty in types {
        type_payload.extend_from_slice(&ty);
    }
    section(1, &type_payload, &mut wasm);

    let mut import_payload = Vec::new();
    u32_leb(imports.len() as u32, &mut import_payload);
    for (field, ty) in imports {
        name("wasi_snapshot_preview1", &mut import_payload);
        name(field, &mut import_payload);
        import_payload.push(0);
        u32_leb(*ty, &mut import_payload);
    }
    section(2, &import_payload, &mut wasm);

    if let Some(body) = body {
        let mut functions = vec![1];
        u32_leb(main_type, &mut functions);
        section(3, &functions, &mut wasm);
        section(5, &[1, 0, 1], &mut wasm);

        let mut exports = vec![1];
        name("main", &mut exports);
        exports.push(0);
        u32_leb(imports.len() as u32, &mut exports);
        section(7, &exports, &mut wasm);

        let mut code = vec![1];
        u32_leb(body.len() as u32, &mut code);
        code.extend_from_slice(&body);
        section(10, &code, &mut wasm);
    }
    wasm
}

fn function_type(params: &[u8], results: &[u8]) -> Vec<u8> {
    let mut out = vec![0x60];
    u32_leb(params.len() as u32, &mut out);
    out.extend_from_slice(params);
    u32_leb(results.len() as u32, &mut out);
    out.extend_from_slice(results);
    out
}

fn call2(body: &mut Vec<u8>, function: u32, first: i32, second: i32) {
    i32_const(body, first);
    i32_const(body, second);
    call_drop(body, function);
}

fn call_drop(body: &mut Vec<u8>, function: u32) {
    call(body, function);
    body.push(0x1a);
}

fn call(body: &mut Vec<u8>, function: u32) {
    body.push(0x10);
    u32_leb(function, body);
}

fn i32_const(body: &mut Vec<u8>, value: i32) {
    body.push(0x41);
    let mut value = value;
    loop {
        let byte = value as u8 & 0x7f;
        value >>= 7;
        let done = (value == 0 && byte & 0x40 == 0) || (value == -1 && byte & 0x40 != 0);
        body.push(if done { byte } else { byte | 0x80 });
        if done {
            break;
        }
    }
}

fn section(id: u8, payload: &[u8], wasm: &mut Vec<u8>) {
    wasm.push(id);
    u32_leb(payload.len() as u32, wasm);
    wasm.extend_from_slice(payload);
}

fn name(value: &str, output: &mut Vec<u8>) {
    u32_leb(value.len() as u32, output);
    output.extend_from_slice(value.as_bytes());
}

fn u32_leb(mut value: u32, output: &mut Vec<u8>) {
    loop {
        let mut byte = (value & 0x7f) as u8;
        value >>= 7;
        if value != 0 {
            byte |= 0x80;
        }
        output.push(byte);
        if value == 0 {
            break;
        }
    }
}

fn u32_at(memory: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(memory[offset..offset + 4].try_into().expect("u32 bytes"))
}

fn u64_at(memory: &[u8], offset: usize) -> u64 {
    u64::from_le_bytes(memory[offset..offset + 8].try_into().expect("u64 bytes"))
}

fn must<T, E>(result: Result<T, E>, context: &str) -> T {
    match result {
        Ok(value) => value,
        Err(_) => panic!("{context}"),
    }
}
