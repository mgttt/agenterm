//! Black-box chassis CLI: compose/check/inspect without invoking cargo.

use std::fs;
use std::path::PathBuf;
use std::process::Command;

use agenterm_chassis::CELLS;

fn bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_agenterm-chassis"))
}

fn write_layout(root: &std::path::Path) {
    for (i, cell) in CELLS.iter().enumerate() {
        let dir = root.join("l1").join(cell);
        fs::create_dir_all(&dir).expect("cell");
        fs::write(dir.join("loader"), format!("L1-{cell}-{i}")).expect("loader");
    }
    fs::create_dir_all(root.join("l2")).expect("l2");
    fs::write(
        root.join("l2/host-abi.json"),
        include_str!("../l2/host-abi.json"),
    )
    .expect("abi");
    fs::create_dir_all(root.join("l3")).expect("l3");
    fs::write(
        root.join("l3/app.json"),
        include_str!("../l3/example-app.json"),
    )
    .expect("app");
}

#[test]
fn cli_compose_check_inspect_round_trip() {
    let tmp = std::env::temp_dir().join(format!("chassis-cli-{}", std::process::id()));
    let _ = fs::remove_dir_all(&tmp);
    let from = tmp.join("from");
    let out = tmp.join("out");
    write_layout(&from);

    let compose = Command::new(bin())
        .args([
            "compose",
            "--from",
            from.to_str().expect("utf8"),
            "--out",
            out.to_str().expect("utf8"),
        ])
        .output()
        .expect("compose");
    assert!(
        compose.status.success(),
        "{}",
        String::from_utf8_lossy(&compose.stderr)
    );
    let stdout = String::from_utf8_lossy(&compose.stdout);
    assert!(stdout.contains("\"invokes_cargo\": false"));
    assert!(stdout.contains("\"compile\": false"));

    let check = Command::new(bin())
        .args(["check", out.to_str().expect("utf8")])
        .output()
        .expect("check");
    assert!(
        check.status.success(),
        "{}",
        String::from_utf8_lossy(&check.stderr)
    );

    let inspect = Command::new(bin())
        .args(["inspect", out.to_str().expect("utf8")])
        .output()
        .expect("inspect");
    assert!(
        inspect.status.success(),
        "{}",
        String::from_utf8_lossy(&inspect.stderr)
    );
    let report = String::from_utf8_lossy(&inspect.stdout);
    assert!(report.contains("tabs.list"));
    assert!(report.contains("example.app"));
    assert!(!report.contains("libSystem"));

    for cell in CELLS {
        let src = fs::read(from.join("l1").join(cell).join("loader")).expect("src");
        let dst = fs::read(out.join("l1").join(cell).join("loader")).expect("dst");
        assert_eq!(src, dst, "{cell}");
    }
    let _ = fs::remove_dir_all(&tmp);
}

#[test]
fn cli_eval_l2_adds_without_cargo() {
    let tmp = std::env::temp_dir().join(format!("chassis-eval-{}", std::process::id()));
    let _ = fs::remove_dir_all(&tmp);
    fs::create_dir_all(&tmp).expect("tmp");
    let prog = tmp.join("add.json");
    fs::write(prog.as_path(), include_str!("../l2/programs/add.json")).expect("prog");
    let out = Command::new(bin())
        .args(["eval-l2", prog.to_str().expect("utf8")])
        .output()
        .expect("eval-l2");
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("\"value\": 3") || stdout.contains("\"value\":3"),
        "{stdout}"
    );
    let _ = fs::remove_dir_all(&tmp);
}

#[test]
fn crate_is_not_the_workbench() {
    let manifest = include_str!("../Cargo.toml");
    assert!(
        !manifest.contains("agenterm =") && !manifest.contains("path = \"../..\""),
        "chassis must not depend on the workbench package"
    );
}
