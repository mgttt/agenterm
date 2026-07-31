const WORKFLOW: &str = include_str!("../.github/workflows/ci.yml");

const CACHE_SHA: &str = "0400d5f644dc74513175e3cd8d07132dd4860809";
const SAVE_CONDITION: &str =
    "if: success() && github.event_name == 'push' && github.ref == 'refs/heads/main'";
const INPUT_HASH: &str = "${{ hashFiles('rust-toolchain.toml', 'Cargo.lock', 'Cargo.toml', \
                          'build.rs', 'scripts/artifacts.json') }}";

fn job(name: &str, next: &str) -> &'static str {
    let start = WORKFLOW
        .find(&format!("  {name}:\n"))
        .unwrap_or_else(|| panic!("missing {name} job"));
    let end = WORKFLOW[start + 1..]
        .find(&format!("\n  {next}:\n"))
        .map(|offset| start + 1 + offset)
        .unwrap_or(WORKFLOW.len());
    &WORKFLOW[start..end]
}

fn cache_paths(job: &str) -> Vec<String> {
    let mut paths = Vec::new();
    let lines = job.lines().collect::<Vec<_>>();
    for (index, line) in lines.iter().enumerate() {
        if !line.trim_start().starts_with("uses: actions/cache/") {
            continue;
        }
        let Some(path_index) = lines[index + 1..]
            .iter()
            .position(|candidate| candidate.trim() == "path: |")
            .map(|offset| index + 1 + offset)
        else {
            panic!("cache step has no path block");
        };
        for candidate in &lines[path_index + 1..] {
            let trimmed = candidate.trim();
            if !candidate.starts_with("            ") || trimmed.contains(':') {
                break;
            }
            if !trimmed.is_empty() {
                paths.push(trimmed.to_owned());
            }
        }
    }
    paths
}

#[test]
fn cache_pilot_is_sha_pinned_and_limited_to_two_jobs() {
    let windows = job("windows", "linux-x86_64");
    let linux = job("linux-x86_64", "linux-aarch64");
    let remaining = &WORKFLOW[WORKFLOW
        .find("  linux-aarch64:\n")
        .expect("linux-aarch64 boundary")..];

    for pilot in [windows, linux] {
        assert_eq!(pilot.matches("uses: actions/cache/restore@").count(), 2);
        assert_eq!(pilot.matches("uses: actions/cache/save@").count(), 2);
        assert_eq!(pilot.matches(CACHE_SHA).count(), 4);
        assert_eq!(pilot.matches("continue-on-error: true").count(), 4);
        assert!(!pilot.contains("enableCrossOsArchive"));
    }
    assert!(!remaining.contains("uses: actions/cache/"));
    assert_eq!(WORKFLOW.matches(CACHE_SHA).count(), 8);
}

#[test]
fn cache_keys_separate_downloads_targets_cells_and_source_revisions() {
    let windows = job("windows", "linux-x86_64");
    let linux = job("linux-x86_64", "linux-aarch64");

    for (pilot, cell) in [(windows, "windows-x86_64-native"), (linux, "linux-x86_64")] {
        let download_prefix = format!(
            "cargo-home-v2-{cell}-${{{{ runner.os }}}}-${{{{ runner.arch }}}}-rust1.97-"
        );
        assert!(
            pilot.contains(&format!("key: {download_prefix}{INPUT_HASH}")),
            "missing dependency-identity cargo-home key for {cell}"
        );
        assert!(
            pilot.contains(&format!(
                "restore-keys: |\n            {download_prefix}"
            )),
            "missing compatible cargo-home restore prefix for {cell}"
        );
        assert!(
            !pilot.contains(&format!(
                "key: {download_prefix}{INPUT_HASH}-${{{{ github.sha }}}}"
            )),
            "cargo-home cache must not be duplicated for every source revision"
        );

        let target_base = format!(
            "cargo-target-v2-{cell}-${{{{ runner.os }}}}-${{{{ runner.arch }}}}-rust1.97-debug-{INPUT_HASH}-"
        );
        assert!(
            pilot.contains(&format!("key: {target_base}${{{{ github.sha }}}}")),
            "missing revision-specific cargo-target key for {cell}"
        );
        assert!(
            pilot.contains(&format!("restore-keys: |\n            {target_base}")),
            "missing revision-independent target restore base for {cell}"
        );
    }
}

#[test]
fn cache_paths_are_exact_and_exclude_product_or_release_evidence() {
    let windows = job("windows", "linux-x86_64");
    let linux = job("linux-x86_64", "linux-aarch64");
    let windows_paths = cache_paths(windows);
    let linux_paths = cache_paths(linux);

    for required in [
        "~/.cargo/registry/index/",
        "~/.cargo/registry/cache/",
        "~/.cargo/git/db/",
        "target/debug/",
        "target/.rustc_info.json",
    ] {
        assert!(windows_paths.iter().any(|path| path == required));
        assert!(linux_paths.iter().any(|path| path == required));
    }
    assert!(
        !windows_paths
            .iter()
            .any(|path| path.contains("x86_64-unknown-linux-gnu"))
    );
    assert!(
        linux_paths
            .iter()
            .any(|path| path == "target/x86_64-unknown-linux-gnu/debug/")
    );

    for path in windows_paths.iter().chain(&linux_paths) {
        assert_ne!(path, "target");
        assert_ne!(path, "target/");
        for forbidden in [
            "target-release",
            "release-fast",
            "qualification",
            "smoke",
            "dist",
            "task-bootstrap",
        ] {
            assert!(!path.contains(forbidden), "forbidden cache path: {path}");
        }
    }
}

#[test]
fn caches_wrap_but_never_control_authoritative_work() {
    let windows = job("windows", "linux-x86_64");
    let linux = job("linux-x86_64", "linux-aarch64");

    assert!(!WORKFLOW.contains("cache-hit"));
    assert!(!WORKFLOW.contains("fail-on-cache-miss"));
    for pilot in [windows, linux] {
        assert_eq!(pilot.matches(SAVE_CONDITION).count(), 2);
        assert!(
            pilot.find("uses: actions/cache/restore@").unwrap()
                < pilot.find("rustup component add").unwrap()
        );
        assert!(pilot.rfind("uses: actions/cache/save@").unwrap() > pilot.rfind("run:").unwrap());
    }
    assert!(
        windows.find("Run quality gate").unwrap()
            < windows.find("Save Windows x86_64 build target").unwrap()
    );
    assert!(
        windows
            .find("Run representative public-interface slice")
            .unwrap()
            < windows.find("Save Windows x86_64 build target").unwrap()
    );
    assert!(
        linux
            .find("Validate isolated agenterm-net research loop")
            .unwrap()
            < linux.find("Save Linux x86_64 build targets").unwrap()
    );
}
