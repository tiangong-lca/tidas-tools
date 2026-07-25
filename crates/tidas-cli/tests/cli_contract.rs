use std::process::Command;

fn tidas() -> Command {
    Command::new(env!("CARGO_BIN_EXE_tidas"))
}

#[test]
fn help_exposes_only_the_unified_command_tree() {
    let output = tidas().arg("--help").output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    for command in [
        "convert", "import", "export", "validate", "release", "ruleset", "version",
    ] {
        assert!(
            stdout.contains(command),
            "{command} missing from:\n{stdout}"
        );
    }
    for legacy in [
        "tidas-convert",
        "tidas-import",
        "tidas-export",
        "tidas-validate",
        "tidas-release-tool",
    ] {
        assert!(!stdout.contains(legacy), "{legacy} leaked into:\n{stdout}");
    }
}

#[test]
fn version_json_is_stable_and_parseable() {
    let first = tidas()
        .args(["version", "--format", "json"])
        .output()
        .unwrap();
    let second = tidas()
        .args(["version", "--format", "json"])
        .output()
        .unwrap();
    assert!(first.status.success());
    assert!(first.stderr.is_empty());
    assert_eq!(first.stdout, second.stdout);
    let payload: serde_json::Value = serde_json::from_slice(&first.stdout).unwrap();
    assert_eq!(payload["schema_version"], "tidas.operation-report.v1");
    assert_eq!(payload["exit_class"], "success");
    assert!(payload["summary"]["asset_count"].as_u64().unwrap() >= 79);
}

#[test]
fn unmigrated_command_fails_without_python_fallback() {
    let output = tidas()
        .args(["validate", "--format", "json"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(69));
    assert!(output.stderr.is_empty());
    let payload: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(payload["exit_class"], "unavailable");
    assert_eq!(payload["completeness"], "not-started");
}

#[test]
fn malformed_usage_is_actionable_and_uses_the_usage_exit_class() {
    let output = tidas().arg("unknown").output().unwrap();
    assert_eq!(output.status.code(), Some(64));
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("unrecognized subcommand"));
    assert!(stderr.contains("--help"));
}
