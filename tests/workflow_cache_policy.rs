use std::sync::LazyLock;

static WORKFLOW: LazyLock<String> =
    LazyLock::new(|| include_str!("../.github/workflows/ci.yml").replace("\r\n", "\n"));

const CACHE_SHA: &str = "0400d5f644dc74513175e3cd8d07132dd4860809";
const SAVE_CONDITION: &str =
    "if: success() && github.event_name == 'push' && github.ref == 'refs/heads/main'";
const INPUT_HASH: &str = "${{ hashFiles('rust-toolchain.toml', 'Cargo.lock', 'Cargo.toml', \
                          'build.rs', 'scripts/artifacts.json') }}";

fn job(name: &str) -> &'static str {
    let ordered_jobs = [
        "windows",
        "linux-x86_64",
        "linux-aarch64",
        "macos",
        "windows-aarch64",
    ];
    let start = WORKFLOW
        .find(&format!("  {name}:\n"))
        .unwrap_or_else(|| panic!("missing {name} job"));
    let job_index = ordered_jobs
        .iter()
        .position(|item| item == &name)
        .unwrap_or_else(|| panic!("unsupported job: {name}"));
    let end = if job_index + 1 < ordered_jobs.len() {
        WORKFLOW
            .find(&format!("\n  {}:\n", ordered_jobs[job_index + 1]))
            .unwrap_or(WORKFLOW.len())
    } else {
        WORKFLOW.len()
    };
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

fn all_jobs() -> Vec<&'static str> {
    vec![
        "windows",
        "linux-x86_64",
        "linux-aarch64",
        "macos",
        "windows-aarch64",
    ]
}

#[test]
fn cache_pilot_is_sha_pinned_and_covers_all_ci_cache_cells() {
    for job_name in all_jobs() {
        let pilot = job(job_name);
        assert_eq!(pilot.matches("uses: actions/cache/restore@").count(), 2);
        assert_eq!(pilot.matches("uses: actions/cache/save@").count(), 2);
        for action in pilot
            .lines()
            .filter(|line| line.contains("uses: actions/cache/"))
        {
            assert!(
                action.contains(CACHE_SHA),
                "cache action is not pinned to the approved SHA: {action}"
            );
        }
        let cache_steps = pilot
            .split("      - name:")
            .filter(|step| step.contains("uses: actions/cache/"))
            .collect::<Vec<_>>();
        assert_eq!(cache_steps.len(), 4);
        assert!(
            cache_steps
                .iter()
                .all(|step| step.contains("continue-on-error: true")),
            "every cache step must remain fail-safe"
        );
        assert!(!pilot.contains("enableCrossOsArchive"));
    }
    assert_eq!(WORKFLOW.matches(CACHE_SHA).count(), 22);
}

#[test]
fn platform_contract_download_cache_is_pinned_fail_safe_and_bounded() {
    let start = WORKFLOW
        .find("  platform-contract:\n")
        .expect("missing platform-contract job");
    let end = WORKFLOW
        .find("\n  windows:\n")
        .expect("missing windows job boundary");
    let pilot = &WORKFLOW[start..end];

    // Download cache only: the four matrix cells compile little enough that a
    // per-cell target cache is not worth its eviction pressure.
    assert_eq!(pilot.matches("uses: actions/cache/restore@").count(), 1);
    assert_eq!(pilot.matches("uses: actions/cache/save@").count(), 1);
    for action in pilot
        .lines()
        .filter(|line| line.contains("uses: actions/cache/"))
    {
        assert!(
            action.contains(CACHE_SHA),
            "cache action is not pinned to the approved SHA: {action}"
        );
    }
    let cache_steps = pilot
        .split("      - name:")
        .filter(|step| step.contains("uses: actions/cache/"))
        .collect::<Vec<_>>();
    assert_eq!(cache_steps.len(), 2);
    assert!(
        cache_steps
            .iter()
            .all(|step| step.contains("continue-on-error: true")),
        "every cache step must remain fail-safe"
    );
    assert_eq!(pilot.matches(SAVE_CONDITION).count(), 1);

    let download_prefix = "cargo-home-v2-platform-contract-${{ matrix.target }}-${{ runner.os }}-${{ runner.arch }}-rust1.97-";
    assert!(
        pilot.contains(&format!("key: {download_prefix}{INPUT_HASH}")),
        "missing dependency-identity cargo-home key for platform-contract"
    );
    assert!(
        pilot.contains(&format!("restore-keys: |\n            {download_prefix}")),
        "missing compatible cargo-home restore prefix for platform-contract"
    );
    assert!(
        !pilot.contains("target/debug/"),
        "platform-contract must not cache build targets"
    );
}

#[test]
fn cache_keys_separate_downloads_targets_cells_and_source_revisions() {
    let windows = job("windows");
    let linux = job("linux-x86_64");
    let linux_aarch64 = job("linux-aarch64");
    let macos = job("macos");
    let windows_aarch64 = job("windows-aarch64");

    for (pilot, cell) in [(windows, "windows-x86_64-native"), (linux, "linux-x86_64")] {
        let download_prefix =
            format!("cargo-home-v2-{cell}-${{{{ runner.os }}}}-${{{{ runner.arch }}}}-rust1.97-");
        assert!(
            pilot.contains(&format!("key: {download_prefix}{INPUT_HASH}")),
            "missing dependency-identity cargo-home key for {cell}"
        );
        assert!(
            pilot.contains(&format!("restore-keys: |\n            {download_prefix}")),
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

    let linux_aarch64_download =
        "cargo-home-v2-linux-aarch64-${{ runner.os }}-${{ runner.arch }}-rust1.97-";
    let linux_aarch64_target =
        "cargo-target-v2-linux-aarch64-${{ runner.os }}-${{ runner.arch }}-rust1.97-debug-";
    assert!(
        linux_aarch64.contains(&format!("key: {linux_aarch64_download}{INPUT_HASH}")),
        "missing dependency-identity cargo-home key for linux-aarch64"
    );
    assert!(
        linux_aarch64.contains(&format!(
            "restore-keys: |\n            {linux_aarch64_download}"
        )),
        "missing compatible cargo-home restore prefix for linux-aarch64"
    );
    assert!(
        linux_aarch64.contains(&format!(
            "key: {linux_aarch64_target}{INPUT_HASH}-${{{{ github.sha }}}}"
        )),
        "missing revision-specific cargo-target key for linux-aarch64"
    );

    let macos_download =
        "cargo-home-v2-macos-${{ runner.os }}-${{ runner.arch }}-${{ matrix.target }}-rust1.97-";
    let macos_target = "cargo-target-v2-macos-${{ matrix.target }}-${{ runner.os }}-${{ runner.arch }}-rust1.97-debug-";
    assert!(
        macos.contains(&format!("key: {macos_download}{INPUT_HASH}")),
        "missing dependency-identity cargo-home key for macos"
    );
    assert!(
        macos.contains(&format!("restore-keys: |\n            {macos_download}")),
        "missing compatible cargo-home restore prefix for macos"
    );
    assert!(
        macos.contains(&format!(
            "key: {macos_target}{INPUT_HASH}-${{{{ github.sha }}}}"
        )),
        "missing revision-specific cargo-target key for macos"
    );

    let windows_aarch64_download = "cargo-home-v2-ci-windows-aarch64-${{ runner.os }}-${{ runner.arch }}-rust1.97-cargo-xwin0.23.0-";
    let windows_aarch64_target = "cargo-target-v3-windows-aarch64-ci-${{ runner.os }}-${{ runner.arch }}-rust1.97-cargo-xwin0.23.0-debug-";
    assert!(
        windows_aarch64.contains(&format!("key: {windows_aarch64_download}{INPUT_HASH}")),
        "missing dependency-identity cargo-home key for windows-aarch64"
    );
    assert!(
        windows_aarch64.contains(&format!(
            "restore-keys: |\n            {windows_aarch64_download}"
        )),
        "missing compatible cargo-home restore prefix for windows-aarch64"
    );
    assert!(
        windows_aarch64.contains(&format!(
            "key: {windows_aarch64_target}{INPUT_HASH}-${{{{ github.sha }}}}"
        )),
        "missing revision-specific cargo-target key for windows-aarch64"
    );
}

#[test]
fn cache_paths_are_exact_and_exclude_product_or_release_evidence() {
    let windows = job("windows");
    let linux = job("linux-x86_64");
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
    let windows = job("windows");
    let linux = job("linux-x86_64");

    assert!(WORKFLOW.contains("cache-hit"));
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
