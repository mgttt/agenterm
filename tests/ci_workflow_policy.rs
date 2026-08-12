use std::sync::LazyLock;

static AGENTERM: LazyLock<String> =
    LazyLock::new(|| include_str!("../.github/workflows/ci-agenterm.yml").replace("\r\n", "\n"));
static CON: LazyLock<String> = LazyLock::new(|| {
    include_str!("../.github/workflows/ci-agenterm-con.yml").replace("\r\n", "\n")
});

const CHECKOUT_SHA: &str = "08eba0b27e820071cde6df949e0beb9ba4906955";

#[test]
fn product_ci_workflows_are_independent_and_sha_pinned() {
    assert!(AGENTERM.contains("name: CI / agenterm"));
    assert!(CON.contains("name: CI / agenterm-con"));
    assert!(!AGENTERM.contains("-p agenterm-con"));
    assert!(!CON.contains("-p agenterm --"));
    assert!(!CON.contains("agenterm.tasks.json"));
    assert!(!CON.contains("task run"));
    assert!(AGENTERM.contains("-p agenterm --all-targets"));
    assert!(AGENTERM.contains("./rh-check.sh"));
    for source in [AGENTERM.as_str(), CON.as_str()] {
        assert!(source.contains(CHECKOUT_SHA));
        assert!(source.contains("persist-credentials: false"));
        assert!(source.contains("permissions:\n  contents: read"));
        assert!(source.contains("workflow_dispatch:"));
        assert!(source.contains("push:"));
        assert!(source.contains("pull_request:"));
    }
}

#[test]
fn both_products_cover_all_six_target_cells() {
    for target in [
        "x86_64-pc-windows-msvc",
        "aarch64-pc-windows-msvc",
        "x86_64-unknown-linux-gnu",
        "aarch64-unknown-linux-gnu",
        "aarch64-apple-darwin",
        "x86_64-apple-darwin",
    ] {
        assert!(AGENTERM.contains(target), "main CI misses {target}");
        assert!(CON.contains(target), "con CI misses {target}");
    }
    assert!(AGENTERM.contains("cargo xwin check --locked -p agenterm"));
    assert!(CON.contains("cargo xwin check --locked -p agenterm-con"));
    assert!(AGENTERM.contains("gcc-aarch64-linux-gnu"));
    assert!(CON.contains("gcc-aarch64-linux-gnu"));
    assert!(AGENTERM.contains("runner: macos-15-intel"));
    assert!(CON.contains("runner: macos-15-intel"));
}

#[test]
fn con_windows_job_uses_release_equivalent_unwind_graph_and_blackbox_package() {
    assert!(CON.contains("--profile con-release-fast"));
    assert!(CON.contains("build-std=std,panic_unwind,compiler_builtins"));
    assert!(CON.contains("build-std-features=panic-unwind,backtrace-trace-only"));
    assert!(CON.contains("RUSTC_BOOTSTRAP: \"1\""));
    assert!(CON.contains("AGENTERM_NO_ACTIVATE: \"1\""));
    assert!(CON.contains("cargo test -Z"));
    assert!(CON.contains("-p agenterm-con"));
    assert!(CON.contains("cargo build -Z"));
    assert!(CON.contains("--bin agenterm-con"));
    assert!(CON.contains("Validate con capability and evidence contract"));
    assert!(CON.contains("--test agenterm_con_alignment"));
}
