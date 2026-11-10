use std::process::Command;

fn cli() -> Command {
    Command::new(env!("CARGO_BIN_EXE_vibe-memory"))
}

fn assert_success(output: &std::process::Output) {
    assert!(
        output.status.success(),
        "status: {:?}\nstdout: {}\nstderr: {}",
        output.status.code(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}

#[test]
fn reports_native_version() {
    let output = cli().arg("--version").output().unwrap();
    assert_success(&output);
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.starts_with("vibe-memory 3."));
}

#[test]
fn emits_compact_template_json() {
    let output = cli().args(["template", "entity"]).output().unwrap();
    assert_success(&output);
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert_eq!(stdout.lines().count(), 1);
    assert!(stdout.contains("\"entity_type\":\"Person\""));
}

#[test]
fn emits_failure_reproduction_template() {
    let output = cli()
        .args(["template", "failure-reproduction"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        value["metadata"]["vibe_memory_failure_reproduction"]["type"],
        "failure_reproduction"
    );
}

#[test]
fn emits_evidence_impact_review_template() {
    let output = cli()
        .args(["template", "evidence-impact-review"])
        .output()
        .unwrap();
    assert_success(&output);
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        value["metadata"]["vibe_memory_evidence_impact_review"]["type"],
        "evidence_impact_review"
    );
}

#[test]
fn emits_agent_state_and_skill_execution_templates() {
    let state = cli().args(["template", "agent-state"]).output().unwrap();
    assert_success(&state);
    let state: serde_json::Value = serde_json::from_slice(&state.stdout).unwrap();
    assert_eq!(
        state["metadata"]["vibe_memory_agent_state"]["type"],
        "agent_state"
    );

    let execution = cli()
        .args(["template", "skill-execution"])
        .output()
        .unwrap();
    assert_success(&execution);
    let execution: serde_json::Value = serde_json::from_slice(&execution.stdout).unwrap();
    assert_eq!(
        execution["metadata"]["vibe_memory_skill_execution"]["type"],
        "skill_execution"
    );
}

#[test]
fn keeps_legacy_commands_out_of_help() {
    let output = cli().arg("--help").output().unwrap();
    assert_success(&output);
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("recall"));
    assert!(!stdout.contains("request-feature"));
    assert!(stdout.contains("maintenance-audit"));
    assert!(stdout.contains("evidence-impact-candidates"));
    assert!(stdout.contains("agent-memory-export"));
    assert!(stdout.contains("audit"));
}

#[test]
fn emits_agent_memory_repo_templates() {
    for kind in ["agent-memory-diff", "agent-memory-apply"] {
        let output = cli().args(["template", kind]).output().unwrap();
        assert_success(&output);
        let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(value["entries"][0]["path"], "MEMORY.md");
        assert!(value["entries"][0]["keywords"].is_array());
    }
}
