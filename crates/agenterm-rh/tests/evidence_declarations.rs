use std::path::Path;

#[test]
fn qualification_evidence_matches_static_task_declarations() {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let manifest: serde_json::Value = serde_json::from_slice(
        &std::fs::read(repo.join("scripts/qualification-gates.json"))
            .expect("read qualification gates"),
    )
    .expect("parse qualification gates");

    for gate in manifest["required_gates"]
        .as_array()
        .expect("required_gates")
    {
        let expected = gate["evidence"].as_array().expect("gate evidence");
        if expected.is_empty() {
            continue;
        }
        let id = gate["id"].as_str().expect("gate id");
        let source = std::fs::read_to_string(repo.join(format!("scripts/rh/{id}.rh")))
            .unwrap_or_else(|error| panic!("read evidence suite {id}: {error}"));
        let actual = agenterm_rh::static_evidence_declarations(&source)
            .unwrap_or_else(|error| panic!("parse evidence suite {id}: {error}"));
        let expected = expected
            .iter()
            .map(|value| value.as_str().expect("evidence id"))
            .collect::<Vec<_>>();
        assert_eq!(actual, expected, "stale evidence declarations for {id}");
    }
}

#[test]
fn quality_gate_lists_evidence_without_compiling_or_running_smoke_packs() {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let source =
        std::fs::read_to_string(repo.join("scripts/rh/check.rh")).expect("read quality gate");
    let start = source
        .find("fn evidence_list_spec(")
        .expect("evidence_list_spec");
    let tail = &source[start..];
    let end = tail.find("\n}").expect("evidence_list_spec end");
    let function = &tail[..end];
    assert!(function.contains("[\"rh\", \"evidence-list\"]"));
    assert!(!function.contains("[\"rh\", \"run\"]"));
    assert!(function.contains("spec(worker_program, arguments, 10000"));
}
