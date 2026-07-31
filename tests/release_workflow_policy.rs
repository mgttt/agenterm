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
    assert!(CANDIDATE.contains("candidate-aggregate.rhai"));
    assert!(CANDIDATE.contains("--project-root . --"));
    assert!(CANDIDATE.contains("path: candidate-output/"));
    assert!(!CANDIDATE.contains(".agenterm-script.bin"));
    assert!(CANDIDATE.contains("name: release-candidate-${{ github.run_id }}"));
    assert!(CANDIDATE.contains("retention-days: 14"));
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
    assert!(PROMOTION.contains("candidate-verify.rhai"));
    assert!(PROMOTION.contains("environment: release"));
    assert!(PROMOTION.contains("contents: write"));
    assert!(PROMOTION.contains("repos/$GITHUB_REPOSITORY/git/refs"));
    assert!(PROMOTION.contains("--verify-tag"));
    assert!(PROMOTION.contains("Recovering exact unpublished draft"));
    assert!(PROMOTION.contains("agenterm-promotion-identity"));
    assert!(PROMOTION.contains("scripts/rhai/promotion-identity.rhai"));
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
    assert!(!PROMOTION.contains(".agenterm-script.bin"));
    for forbidden in [
        "--clobber",
        "cargo build",
        "cargo test",
        "cargo check",
        "check.cmd --release",
        "package-client-release",
        "package-release-qualified",
        "notarytool",
        "codesign",
    ] {
        assert!(
            !PROMOTION.contains(forbidden),
            "promotion contains forbidden operation: {forbidden}"
        );
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
