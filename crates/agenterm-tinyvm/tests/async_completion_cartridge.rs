use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::rc::Rc;

use agenterm_tinyvm::{
    CartridgeManifest, GameInput, GameLimits, GameRuntime, Indexed2dFrame, Limits,
    NativeModuleRegistry, ResourceDomainAllocator, WasmError,
};

fn must<T>(result: Result<T, WasmError>, context: &str) -> T {
    match result {
        Ok(value) => value,
        Err(error) => panic!("{context}: {}", error.message()),
    }
}

#[test]
fn standard_async_completion_cartridge_runs_host_neutrally() {
    let module = "fan:async/v1";
    let fixture =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/async-completion-v1.wat");
    let bare = wat::parse_file(fixture).expect("compile async completion fixture");
    let wasm = must(
        CartridgeManifest {
            game_id: "org.example.async-completion".to_owned(),
            game_version: "0.1.0".to_owned(),
            abi_version: 1,
            state_version: 1,
            capabilities: vec![module.to_owned()],
        }
        .append_to_wasm(&bare),
        "attach async completion manifest",
    );

    let mut allocator = ResourceDomainAllocator::new();
    let mut registry = NativeModuleRegistry::new();
    let queue = Rc::new(RefCell::new(must(
        registry.completion_queue(module, 1, 4, &mut allocator),
        "create completion queue",
    )));
    let issued = Rc::new(Cell::new(None));
    let start_queue = queue.clone();
    let observed = issued.clone();
    must(
        registry.register(module, "start", 0, 1, move |_, _| {
            let ticket = start_queue
                .try_borrow_mut()
                .map_err(|_| WasmError::Trap("fixture completion reentrancy"))?
                .begin(4)
                .map_err(|_| WasmError::Trap("fixture completion begin"))?;
            observed.set(Some(ticket));
            Ok(vec![ticket.as_i32()])
        }),
        "register async start",
    );
    must(
        registry.register_completion_imports(module, queue.clone(), 8),
        "register common completion imports",
    );

    let mut runtime = must(
        GameRuntime::from_bytes_with_registry(
            &wasm,
            Limits::default(),
            GameLimits::default(),
            1,
            registry,
        ),
        "open async completion cartridge",
    );
    let pending = must(runtime.tick(GameInput::default()), "render pending frame");
    let pending = must(
        Indexed2dFrame::decode(&pending.render),
        "decode pending indexed frame",
    );
    assert_eq!(pending.palette_rgba().collect::<Vec<_>>(), [0]);

    let ticket = issued.get().expect("start returned completion ticket");
    queue
        .borrow_mut()
        .try_complete(ticket, 7, vec![0x11, 0x22, 0x33, 0xff])
        .expect("publish completion payload");
    let ready = must(
        runtime.tick(GameInput::default()),
        "guest takes ready completion",
    );
    let ready = must(
        Indexed2dFrame::decode(&ready.render),
        "decode completed indexed frame",
    );
    assert_eq!(ready.palette_rgba().collect::<Vec<_>>(), [0xff33_2211]);
    assert_eq!(ready.pixels(), [0]);
    assert!(queue.borrow().is_empty());
}
