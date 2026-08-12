use std::sync::LazyLock;

static AGENTERM: LazyLock<String> =
    LazyLock::new(|| include_str!("../.github/workflows/ci-agenterm.yml").replace("\r\n", "\n"));
static CON: LazyLock<String> = LazyLock::new(|| {
    include_str!("../.github/workflows/ci-agenterm-con.yml").replace("\r\n", "\n")
});

#[test]
fn split_feedback_ci_has_no_cross_product_cache_or_artifact_authority() {
    for source in [AGENTERM.as_str(), CON.as_str()] {
        assert!(!source.contains("actions/upload-artifact"));
        assert!(!source.contains("actions/download-artifact"));
        assert!(!source.contains("actions/cache"));
        assert!(!source.contains("target/qualification/receipt.json"));
        assert!(!source.contains("contents: write"));
        assert!(source.contains("cancel-in-progress: true"));
    }
    assert!(!AGENTERM.contains("con-release-fast"));
    assert!(!CON.contains("check.cmd"));
    assert!(!CON.contains("release.cmd"));
}
