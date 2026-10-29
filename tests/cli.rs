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
fn keeps_legacy_commands_out_of_help() {
    let output = cli().arg("--help").output().unwrap();
    assert_success(&output);
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("recall"));
    assert!(!stdout.contains("request-feature"));
}
