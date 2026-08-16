use std::cell::Cell;
use std::fs;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use agenterm_chassis::CELLS;
use agenterm_chassis::l2_dispatch::HostCallback;
use serde_json::{Value, json};

#[allow(dead_code)]
#[path = "../src/loader/mod.rs"]
mod loader;

#[derive(Clone, Debug)]
struct RecordingHost {
    calls: Rc<Cell<usize>>,
    result: Value,
}

impl HostCallback for RecordingHost {
    fn call(&mut self, capability: &str, parameters: &Value) -> Result<Value, String> {
        assert_eq!(capability, "tabs.active");
        assert_eq!(parameters, &json!({}));
        self.calls.set(self.calls.get() + 1);
        Ok(self.result.clone())
    }
}

#[test]
fn checked_composed_image_dispatches_real_l2_program_exactly_once() {
    let fixture = Fixture::new("valid", &["tabs.active"]);
    fixture.write_program(
        "active-tab.json",
        json!({"caps":["tabs.active"],"ops":[["call","tabs.active"],["halt"]]}),
    );
    let image = loader::load_image(fixture.path()).expect("checked image");
    let calls = Rc::new(Cell::new(0));
    let (value, _) = image
        .eval_l2(
            "active-tab.json",
            RecordingHost {
                calls: Rc::clone(&calls),
                result: json!(73),
            },
        )
        .expect("L2 eval");
    assert_eq!(value, 73);
    assert_eq!(calls.get(), 1);
}

#[test]
fn unknown_and_undeclared_program_capabilities_fail_before_host_callback() {
    for (label, declared, called) in [
        ("unknown", vec!["tabs.active"], "not.a.capability"),
        ("undeclared", vec!["tabs.active"], "tabs.list"),
    ] {
        let fixture = Fixture::new(label, &declared);
        fixture.write_program(
            "bad.json",
            json!({"caps":[called],"ops":[["call",called],["halt"]]}),
        );
        let image = loader::load_image(fixture.path()).expect("syntactically valid image");
        let calls = Rc::new(Cell::new(0));
        let error = image
            .eval_l2(
                "bad.json",
                RecordingHost {
                    calls: Rc::clone(&calls),
                    result: Value::Null,
                },
            )
            .expect_err("reject program capability closure");
        assert!(error.to_string().contains("unknown cap name"), "{error}");
        assert_eq!(calls.get(), 0);
    }
}

#[test]
fn signature_bound_fails_before_host_callback() {
    let fixture = Fixture::new("signature-bound", &["ui.tabs.set-width"]);
    fixture.write_program(
        "width.json",
        json!({
            "caps":["ui.tabs.set-width"],
            "ops":[["call","ui.tabs.set-width"],["halt"]]
        }),
    );
    let image = loader::load_image(fixture.path()).expect("checked image");
    let calls = Rc::new(Cell::new(0));
    let error = image
        .eval_l2(
            "width.json",
            RecordingHost {
                calls: Rc::clone(&calls),
                result: Value::Null,
            },
        )
        .expect_err("missing bounded width");
    assert!(
        error
            .to_string()
            .contains("missing required property `width`")
    );
    assert_eq!(calls.get(), 0);
}

#[test]
fn vm_step_bound_fails_before_host_callback() {
    let fixture = Fixture::new("step-bound", &["tabs.active"]);
    fixture.write_program(
        "loop.json",
        json!({"caps":["tabs.active"],"ops":[["push",1],["jump",0]]}),
    );
    let image = loader::load_image(fixture.path()).expect("checked image");
    let calls = Rc::new(Cell::new(0));
    let error = image
        .eval_l2_bounded(
            "loop.json",
            RecordingHost {
                calls: Rc::clone(&calls),
                result: json!(1),
            },
            4,
        )
        .expect_err("step budget");
    assert!(error.to_string().contains("step budget exceeded"));
    assert_eq!(calls.get(), 0);
}

#[test]
fn malformed_program_and_tampered_abi_fail_during_image_load() {
    let malformed = Fixture::new("malformed-program", &["tabs.active"]);
    fs::create_dir_all(malformed.path().join("l2/programs")).expect("program dir");
    fs::write(malformed.path().join("l2/programs/bad.json"), b"{").expect("bad program");
    assert!(loader::load_image(malformed.path()).is_err());

    let abi = Fixture::new("tampered-abi", &["tabs.active"]);
    abi.write_program(
        "active.json",
        json!({"caps":["tabs.active"],"ops":[["halt"]]}),
    );
    let path = abi.path().join("l2/host-abi.json");
    let tampered = fs::read_to_string(&path).expect("host ABI").replacen(
        "\"version\": 3",
        "\"version\": 2",
        1,
    );
    fs::write(path, tampered).expect("tamper ABI");
    let error = loader::load_image(abi.path()).expect_err("ABI mismatch");
    assert!(error.to_string().contains("expected 2 / 3"), "{error}");
}

#[test]
fn changed_program_fails_before_host_callback() {
    let fixture = Fixture::new("snapshot", &["tabs.active"]);
    fixture.write_program(
        "active.json",
        json!({"caps":["tabs.active"],"ops":[["call","tabs.active"],["halt"]]}),
    );
    let image = loader::load_image(fixture.path()).expect("checked image");
    fixture.write_program(
        "active.json",
        json!({"caps":["not.a.capability"],"ops":[["call","not.a.capability"]]}),
    );
    let calls = Rc::new(Cell::new(0));
    let error = image
        .eval_l2(
            "active.json",
            RecordingHost {
                calls: Rc::clone(&calls),
                result: json!(19),
            },
        )
        .expect_err("tampered source");
    assert!(error.to_string().contains("changed after image load"));
    assert_eq!(calls.get(), 0);
}

struct Fixture {
    root: PathBuf,
}

impl Fixture {
    fn new(label: &str, capabilities: &[&str]) -> Self {
        let root = std::env::temp_dir().join(format!(
            "agenterm-chassis-composed-dispatch-{label}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = fs::remove_dir_all(&root);
        for cell in CELLS {
            let cell_root = root.join("l1").join(cell);
            fs::create_dir_all(&cell_root).expect("L1 cell");
            fs::write(cell_root.join("loader"), format!("frozen-{cell}")).expect("L1 loader");
        }
        fs::create_dir_all(root.join("l2")).expect("L2");
        fs::write(
            root.join("l2/host-abi.json"),
            include_str!("../l2/host-abi.json"),
        )
        .expect("host ABI");
        fs::create_dir_all(root.join("l3")).expect("L3");
        fs::write(
            root.join("l3/app.json"),
            json!({
                "schema":1,
                "name":"composed.dispatch.test",
                "capabilities":capabilities,
            })
            .to_string(),
        )
        .expect("L3 app");
        fs::write(
            root.join("manifest.json"),
            json!({
                "schema":1,
                "compile":false,
                "invokes_cargo":false,
                "cells":CELLS,
            })
            .to_string(),
        )
        .expect("manifest");
        Self { root }
    }

    fn path(&self) -> &Path {
        &self.root
    }

    fn write_program(&self, name: &str, document: Value) {
        let root = self.root.join("l2/programs");
        fs::create_dir_all(&root).expect("program dir");
        fs::write(root.join(name), document.to_string()).expect("L2 program");
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}
