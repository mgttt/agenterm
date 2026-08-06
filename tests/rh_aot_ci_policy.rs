use std::sync::LazyLock;

static WORKFLOW: LazyLock<String> =
    LazyLock::new(|| include_str!("../.github/workflows/ci.yml").replace("\r\n", "\n"));

fn job_span(name: &str, next_job: Option<&str>) -> &'static str {
    let marker = format!("  {name}:\n");
    let start = WORKFLOW
        .find(marker.as_str())
        .unwrap_or_else(|| panic!("missing CI job: {name}"));
    let end = next_job
        .and_then(|next| WORKFLOW.find(&format!("\n  {next}:\n")))
        .unwrap_or(WORKFLOW.len());
    assert!(end > start, "CI job span for {name} is empty");
    &WORKFLOW[start..end]
}

#[test]
fn linux_x86_64_ci_proves_rh_aot_pipeline() {
    let job = job_span("linux-x86_64", Some("linux-aarch64"));
    assert!(job.contains("cargo test -p agenterm-rh --locked"));
    assert!(job.contains("cargo test --locked --test rh_aot_smoke"));
}

#[test]
fn windows_ci_proves_rh_aot_pipeline() {
    let job = job_span("windows", Some("linux-x86_64"));
    assert!(job.contains("cargo test -p agenterm-rh --locked"));
    assert!(job.contains("rh-aot-smoke"));
}

#[test]
fn cross_cells_compile_rh_for_target() {
    let linux = job_span("linux-aarch64", Some("macos"));
    assert!(linux.contains("cargo check -p agenterm-rh --locked"));
    assert!(linux.contains("aarch64-unknown-linux-gnu"));
    assert!(linux.contains("AGENTERM_RH_QUALIFY_TARGET"));

    let windows = job_span("windows-aarch64", None);
    assert!(windows.contains("cargo check -p agenterm-rh --locked"));
    assert!(windows.contains("aarch64-pc-windows-msvc"));
    assert!(windows.contains("AGENTERM_RH_QUALIFY_TARGET"));
}

#[test]
fn macos_ci_cross_compiles_rh_reference_pack() {
    let job = job_span("macos", Some("windows-aarch64"));
    assert!(job.contains("AGENTERM_RH_QUALIFY_TARGET"));
    assert!(job.contains("cargo check -p agenterm-rh --locked"));
    assert!(job.contains("cross_compiles_reference_pack_when_target_env_set"));
}
