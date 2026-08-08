use std::sync::LazyLock;

static CANDIDATE: LazyLock<String> =
    LazyLock::new(|| include_str!("../.github/workflows/candidate.yml").replace("\r\n", "\n"));
static PROMOTION: LazyLock<String> =
    LazyLock::new(|| include_str!("../.github/workflows/release.yml").replace("\r\n", "\n"));
static INTEGRITY: LazyLock<String> = LazyLock::new(|| {
    include_str!("../.github/workflows/release-integrity.yml").replace("\r\n", "\n")
});
static GIT_ATTRIBUTES: LazyLock<String> =
    LazyLock::new(|| include_str!("../.gitattributes").replace("\r\n", "\n"));

const CHECKOUT_SHA: &str = "08eba0b27e820071cde6df949e0beb9ba4906955";
const UPLOAD_SHA: &str = "ea165f8d65b6e75b540449e92b4886f43607fa02";
const DOWNLOAD_SHA: &str = "fa0a91b85d4f404e444e00e005971372dc801d16";
const CACHE_SHA: &str = "0400d5f644dc74513175e3cd8d07132dd4860809";

#[test]
fn candidate_is_manual_exact_sha_and_has_no_publish_authority() {
    assert!(CANDIDATE.contains("name: Release Candidate"));
    assert!(CANDIDATE.contains("workflow_dispatch:"));
    assert!(CANDIDATE.contains("source_sha:"));
    assert!(!CANDIDATE.contains("\n  push:"));
    assert!(CANDIDATE.contains("actions: read\n  contents: read"));
    assert!(!CANDIDATE.contains("contents: write"));
    assert!(CANDIDATE.contains("[[ \"$SOURCE_SHA\" =~ ^[0-9a-f]{40}$ ]]"));
    assert!(CANDIDATE.contains("[[ \"$GITHUB_SHA\" == \"$SOURCE_SHA\" ]]"));
    assert!(CANDIDATE.contains("git merge-base --is-ancestor"));
    assert!(CANDIDATE.contains("workflows/ci.yml/runs?head_sha=$SOURCE_SHA&status=success"));
    assert!(CANDIDATE.contains("ref: ${{ inputs.source_sha }}"));
    assert!(CANDIDATE.contains("AGENTERM_CANDIDATE_SOURCE_SHA: ${{ inputs.source_sha }}"));
    assert!(CANDIDATE.contains("git switch -C main \"%SOURCE_SHA%\""));
}

#[test]
fn candidate_runs_one_full_gate_and_seals_six_platform_parts() {
    assert_eq!(
        CANDIDATE
            .matches("check.cmd --release --include-stress")
            .count(),
        1
    );
    for platform in [
        "windows-x86_64",
        "windows-aarch64",
        "linux-x86_64",
        "linux-aarch64",
        "macos-aarch64",
        "macos-x86_64",
    ] {
        assert!(
            CANDIDATE.contains(&format!("platform_id: {platform}")),
            "missing candidate cell {platform}"
        );
    }
    assert!(CANDIDATE.contains("pattern: candidate-part-*"));
    assert!(CANDIDATE.contains("merge-multiple: true"));
    assert!(CANDIDATE.contains("target/qualification/receipt.json"));
    assert!(CANDIDATE.contains("name: Stage flat candidate part"));
    assert!(CANDIDATE.contains("path: candidate-part/"));
    assert!(CANDIDATE.contains("candidate-aggregate.rh"));
    assert!(CANDIDATE.contains("--project-root . --"));
    assert!(CANDIDATE.contains("path: candidate-output/"));
    assert!(!CANDIDATE.contains(".agenterm-rhai.bin"));
    assert!(!CANDIDATE.contains("scripts/rhai/check.rhai"));
    assert!(!CANDIDATE.contains("scripts/rhai/fresh-clone-rehearsal.rhai"));
    assert!(CANDIDATE.contains("name: release-candidate-${{ github.run_id }}"));
    assert!(CANDIDATE.contains("retention-days: 14"));
}

#[test]
fn candidate_cargo_home_caches_are_platform_isolated_and_revision_reusable() {
    let input_hash = "${{ hashFiles('rust-toolchain.toml', 'Cargo.lock', 'Cargo.toml', 'build.rs', 'scripts/artifacts.json') }}";
    let generic_key = format!(
        "cargo-home-candidate-v2-${{{{ matrix.platform_id }}}}-${{{{ runner.os }}}}-${{{{ runner.arch }}}}-rust1.97-{input_hash}"
    );
    let windows_arm64_key = format!(
        "cargo-home-candidate-v2-windows-aarch64-${{{{ runner.os }}}}-${{{{ runner.arch }}}}-rust1.97-{input_hash}"
    );

    assert_eq!(CANDIDATE.matches(&format!("key: {generic_key}")).count(), 2);
    assert_eq!(
        CANDIDATE
            .matches(&format!("key: {windows_arm64_key}"))
            .count(),
        2
    );
    assert!(!CANDIDATE.contains("cargo-home-candidate-${{ runner.os }}"));
    assert!(!CANDIDATE.contains("cargo-home-v3-windows-aarch64"));

    for step_name in [
        "Restore candidate Cargo cache",
        "Save candidate Cargo cache",
        "Restore Windows ARM64 cargo cache",
        "Save Windows ARM64 cargo cache",
    ] {
        let step = CANDIDATE
            .split("      - name:")
            .find(|step| step.contains(step_name))
            .expect("candidate Cargo-home cache step");
        assert!(step.contains(CACHE_SHA));
        assert!(!step.contains("inputs.source_sha"));
        assert!(!step.contains("restore-keys:"));
    }

    for step_name in [
        "Restore candidate Cargo cache",
        "Save candidate Cargo cache",
    ] {
        let step = CANDIDATE
            .split("      - name:")
            .find(|step| step.contains(step_name))
            .expect("generic candidate Cargo-home cache step");
        assert!(step.contains("matrix.platform_id != 'windows-aarch64'"));
    }
}

#[test]
fn promotion_is_manual_candidate_bound_and_performs_no_build_or_overwrite() {
    assert!(PROMOTION.contains("workflow_dispatch:"));
    assert!(PROMOTION.contains("candidate_run_id:"));
    assert!(PROMOTION.contains("confirmation:"));
    assert!(!PROMOTION.contains("\n  push:"));
    assert!(PROMOTION.contains(".github/workflows/candidate.yml"));
    assert!(PROMOTION.contains("workflow_dispatch"));
    assert!(PROMOTION.contains("conclusion"));
    assert!(PROMOTION.contains("head_sha"));
    assert!(PROMOTION.contains("publish-$tag"));
    assert!(PROMOTION.contains("candidate-verify.rh"));
    // H1: pure-derive releases.json during verify + publish (not a second truth).
    assert!(PROMOTION.contains("task run build-releases-index"));
    assert!(!PROMOTION.contains("build-releases-index.rhai"));
    assert!(PROMOTION.contains("candidate/releases.json"));
    assert!(PROMOTION.contains("Derive releases.json index"));
    assert!(PROMOTION.contains("environment: release"));
    assert!(PROMOTION.contains("contents: write"));
    assert!(PROMOTION.contains("repos/$GITHUB_REPOSITORY/git/refs"));
    assert!(PROMOTION.contains("--verify-tag"));
    assert!(PROMOTION.contains("Recovering exact unpublished draft"));
    assert!(PROMOTION.contains("agenterm-promotion-identity"));
    assert!(PROMOTION.contains("scripts/rh/promotion-identity.rh"));
    assert!(PROMOTION.contains("agenterm-promotion:v1 candidate_run_id="));
    assert!(PROMOTION.contains("body_sha256"));
    assert!(PROMOTION.contains("[[ \"$(jq -r .body <<<\"$release\")\" == \"$release_body\" ]]"));
    assert!(PROMOTION.contains("[[ \"$(jq -r .name <<<\"$release\")\" == \"AgenTerm $TAG\" ]]"));
    assert!(!PROMOTION.contains("--generate-notes"));
    assert!(PROMOTION.contains("gh api --paginate --slurp"));
    assert!(PROMOTION.contains("select(.tag_name == $wanted)"));
    assert!(PROMOTION.contains("verify_remote_assets"));
    assert!(PROMOTION.contains("gh release upload \"$TAG\" \"$file\""));
    assert!(PROMOTION.contains("sha256sum \"$remote_file\""));
    assert!(PROMOTION.contains("path: candidate/"));
    assert!(!PROMOTION.contains(".agenterm-rhai.bin"));
    for forbidden in [
        "--clobber",
        "cargo ",
        "build.bat",
        "build.sh",
        "check.cmd",
        "check.sh",
        "release.cmd",
        "release.sh",
        "task run check",
        "task run package",
        "notarytool",
        "codesign",
    ] {
        assert!(
            !PROMOTION.contains(forbidden),
            "promotion contains forbidden operation: {forbidden}"
        );
    }
    // Forbid the build/check/package orchestrators, but allow leaf tasks whose
    // ids share a prefix (e.g. build-releases-index).
    for line in PROMOTION.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("task run ") {
            let task = rest.split_whitespace().next().unwrap_or("");
            assert!(
                task != "build" && task != "check" && task != "package",
                "promotion contains forbidden operation: task run {task}"
            );
        }
    }
}

#[test]
fn workflow_actions_are_immutable_and_post_release_integrity_is_read_only() {
    for (source, sha) in [
        (CANDIDATE.as_str(), CHECKOUT_SHA),
        (CANDIDATE.as_str(), UPLOAD_SHA),
        (CANDIDATE.as_str(), DOWNLOAD_SHA),
        (PROMOTION.as_str(), CHECKOUT_SHA),
        (PROMOTION.as_str(), UPLOAD_SHA),
        (PROMOTION.as_str(), DOWNLOAD_SHA),
    ] {
        assert!(source.contains(sha), "missing pinned action SHA {sha}");
    }
    assert!(INTEGRITY.contains("permissions:\n  actions: read\n  contents: read"));
    assert!(!INTEGRITY.contains("contents: write"));
    assert!(!INTEGRITY.contains("gh release upload"));
    assert!(!INTEGRITY.contains("--clobber"));
    assert!(INTEGRITY.contains("sha256sum -c"));
    assert!(INTEGRITY.contains("verified-promotion-$PROMOTION_RUN_ID"));
    assert!(INTEGRITY.contains("candidate-manifest.json"));
    assert!(!INTEGRITY.contains("\n  push:"));
}

#[test]
fn candidate_collects_delivery_timing_evidence() {
    assert!(CANDIDATE.contains("Publish candidate delivery timing evidence"));
    assert!(CANDIDATE.contains("Upload candidate delivery timing"));
    assert!(CANDIDATE.contains("delivery-timing/candidate-delivery-timing.json"));
    assert!(CANDIDATE.contains("candidate-delivery-timing-${{ github.run_id }}"));
}

#[test]
fn promotion_collects_delivery_timing_evidence() {
    assert!(PROMOTION.contains("Publish promotion timing evidence"));
    assert!(PROMOTION.contains("Upload promotion timing evidence"));
    assert!(PROMOTION.contains("delivery-timing/release-delivery-timing.json"));
    assert!(PROMOTION.contains("release-delivery-timing-${{ github.run_id }}"));
    assert!(PROMOTION.contains("agenterm-release-timing"));
    assert!(PROMOTION.contains("\"checkout_ms\""));
    assert!(PROMOTION.contains("\"toolchain_ms\""));
    assert!(PROMOTION.contains("\"promotion_ms\""));
    assert!(PROMOTION.contains("\"tag_to_public_ms\""));
    assert!(PROMOTION.contains("\"release_published_ms\""));
    assert!(PROMOTION.contains("\"candidate_to_promotion_ms\""));
}

#[test]
fn candidate_timing_collects_checkout_and_toolchain_stages() {
    assert!(CANDIDATE.contains("\"checkout_ms\""));
    assert!(CANDIDATE.contains("\"toolchain_ms\""));
    assert!(CANDIDATE.contains("\"cache_ms\""));
    assert!(CANDIDATE.contains("\"compile_ms\""));
    assert!(CANDIDATE.contains("\"test_ms\""));
    assert!(CANDIDATE.contains("\"package_ms\""));
    assert!(CANDIDATE.contains("\"artifact_transfer_ms\""));
    assert!(CANDIDATE.contains("\"aggregate_ms\""));
}

#[test]
fn release_identity_inputs_have_platform_stable_line_endings() {
    for path in [
        "Cargo.lock",
        "scripts/artifacts.json",
        "scripts/qualification-gates.json",
    ] {
        assert!(
            GIT_ATTRIBUTES
                .lines()
                .any(|line| line == format!("{path} text eol=lf")),
            "release identity input lacks an LF policy: {path}"
        );
    }
}
