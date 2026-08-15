use std::sync::LazyLock;

static WORKFLOW: LazyLock<String> =
    LazyLock::new(|| include_str!("../.github/workflows/ci-chassis.yml").replace("\r\n", "\n"));

#[test]
fn classifier_uses_event_specific_diff_bases_and_explicit_outputs() {
    let classify = WORKFLOW
        .split("\n  l1:\n")
        .next()
        .expect("classify job section");
    assert!(classify.contains("classify:"));
    assert!(classify.contains("fetch-depth: 0"));
    assert!(
        classify.contains("requires_l1_candidate: ${{ steps.gate.outputs.requires_l1_candidate }}")
    );
    assert!(classify.contains("l1_reasons: ${{ steps.gate.outputs.l1_reasons }}"));
    assert!(classify.contains("PR_BASE_SHA: ${{ github.event.pull_request.base.sha }}"));
    assert!(classify.contains("PUSH_BASE_SHA: ${{ github.event.before }}"));
    assert!(classify.contains("HEAD_SHA: ${{ github.sha }}"));
    assert!(classify.contains("if [[ \"$EVENT_NAME\" == \"pull_request\" ]]"));
    assert!(classify.contains("base=\"$PR_BASE_SHA\""));
    assert!(classify.contains("base=\"$PUSH_BASE_SHA\""));
    assert!(classify.contains("--base \"$base\""));
    assert!(classify.contains("--head \"$HEAD_SHA\""));
    assert!(classify.contains("--github-output \"$GITHUB_OUTPUT\""));
}

#[test]
fn dispatch_and_missing_push_base_force_the_l1_classifier_true() {
    let classify = WORKFLOW
        .split("\n  l1:\n")
        .next()
        .expect("classify job section");
    assert!(classify.contains("if [[ \"$EVENT_NAME\" == \"workflow_dispatch\" ]]"));
    assert_eq!(classify.matches("--paths src/bin/agenterm.rs").count(), 2);
    assert!(classify.contains("[[ -z \"$base\" || \"$base\" =~ ^0+$ ]]"));
    assert!(!classify.contains("requires_l1_candidate=true"));
}

#[test]
fn six_cell_l1_is_conditional_while_l2_pack_always_runs() {
    let l1 = WORKFLOW
        .split("\n  l1:\n")
        .nth(1)
        .expect("L1 job")
        .split("\n  l2-pack:\n")
        .next()
        .expect("L1 section");
    assert!(l1.contains("needs: classify"));
    assert!(l1.contains("if: needs.classify.outputs.requires_l1_candidate == 'true'"));
    assert_eq!(l1.matches("target:").count(), 6);
    assert_eq!(l1.matches("--features loader").count(), 2);

    let l2 = WORKFLOW
        .split("\n  l2-pack:\n")
        .nth(1)
        .expect("L2 pack job");
    assert!(!l2.contains("needs: classify"));
    assert!(!l2.contains("requires_l1_candidate"));
    assert!(l2.contains("python3 scripts/chassis-ci-pack.py"));
    assert!(l2.contains("python3 scripts/chassis-compose-product-test.py"));
}
