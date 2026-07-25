use std::fs;
use std::process::Command;

use tidas_contracts::{CommandNameV1, DiagnosticV1, OperationReportV1};

const TIDAS_ENVIRONMENT: &[&str] = &[
    "TIDAS_CONFIG",
    "TIDAS_LOG",
    "TIDAS_PROGRESS",
    "TIDAS_MEMORY_BUDGET_MIB",
    "TIDAS_QUEUE_CAPACITY",
];

fn tidas() -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_tidas"));
    for variable in TIDAS_ENVIRONMENT {
        command.env_remove(variable);
    }
    command
}

fn json_output(args: &[&str]) -> (std::process::Output, serde_json::Value) {
    let output = tidas().args(args).output().unwrap();
    let payload = serde_json::from_slice(&output.stdout).unwrap();
    (output, payload)
}

#[test]
fn help_exposes_the_unified_command_and_runtime_contract() {
    let output = tidas().arg("--help").output().unwrap();
    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    let stdout = String::from_utf8(output.stdout).unwrap();
    for command in [
        "convert", "import", "export", "validate", "release", "ruleset", "version",
    ] {
        assert!(
            stdout.contains(command),
            "{command} missing from:\n{stdout}"
        );
    }
    for option in [
        "--format",
        "--report",
        "--config",
        "--log-level",
        "--progress",
        "--memory-budget-mib",
        "--queue-capacity",
        "--completion",
    ] {
        assert!(stdout.contains(option), "{option} missing from:\n{stdout}");
    }
    assert!(stdout.contains("No configuration file is loaded implicitly"));
    assert!(stdout.contains("Stdout contains only"));
    for legacy in [
        "\n  help ",
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
fn every_product_command_has_discoverable_help() {
    for command in [
        "convert", "import", "export", "validate", "release", "ruleset", "version",
    ] {
        let output = tidas().args([command, "--help"]).output().unwrap();
        assert!(output.status.success(), "{command}");
        assert!(output.stderr.is_empty(), "{command}");
        let stdout = String::from_utf8(output.stdout).unwrap();
        assert!(
            stdout.contains(&format!("Usage: tidas {command}")),
            "{stdout}"
        );
    }
}

#[test]
fn version_json_is_stable_parseable_and_context_complete() {
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
    let typed: OperationReportV1 = serde_json::from_slice(&first.stdout).unwrap();
    assert_eq!(typed.command, CommandNameV1::Version);
    let payload: serde_json::Value = serde_json::from_slice(&first.stdout).unwrap();
    assert_eq!(payload["schema_version"], "tidas.operation-report.v1");
    assert_eq!(
        payload["invocation"]["schema_version"],
        "tidas.invocation-context.v1"
    );
    assert_eq!(payload["exit_class"], "success");
    assert_eq!(payload["invocation"]["config_source"], "none");
    assert_eq!(
        payload["invocation"]["memory_budget_bytes"],
        512 * 1024 * 1024
    );
    assert_eq!(payload["invocation"]["queue_capacity"], 256);
    assert_eq!(
        payload["invocation"]["input_policy"],
        "explicit-path-or-dash"
    );
    assert_eq!(payload["invocation"]["report_destination"], "stdout");
    assert_eq!(payload["invocation"]["diagnostic_destination"], "stderr");
    assert!(payload["summary"]["asset_count"].as_u64().unwrap() >= 79);
}

#[test]
fn cli_options_override_environment_and_environment_overrides_defaults() {
    let output = tidas()
        .env("TIDAS_CONFIG", "environment.toml")
        .env("TIDAS_LOG", "info")
        .env("TIDAS_PROGRESS", "always")
        .env("TIDAS_MEMORY_BUDGET_MIB", "256")
        .env("TIDAS_QUEUE_CAPACITY", "32")
        .args([
            "version",
            "--format",
            "json",
            "--config",
            "command.toml",
            "--log-level",
            "debug",
            "--progress",
            "never",
            "--memory-budget-mib",
            "384",
            "--queue-capacity",
            "48",
        ])
        .output()
        .unwrap();
    assert!(output.status.success());
    let payload: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let invocation = &payload["invocation"];
    assert_eq!(invocation["config_source"], "cli");
    assert_eq!(invocation["config_path"], "command.toml");
    assert_eq!(invocation["log_level"], "debug");
    assert_eq!(invocation["progress_mode"], "never");
    assert_eq!(invocation["progress_enabled"], false);
    assert_eq!(invocation["memory_budget_bytes"], 384 * 1024 * 1024);
    assert_eq!(invocation["queue_capacity"], 48);

    let (_, environment_payload) = json_output(&["version", "--format", "json"]);
    assert_eq!(environment_payload["invocation"]["config_source"], "none");
}

#[test]
fn environment_config_is_explicitly_reported() {
    let output = tidas()
        .env("TIDAS_CONFIG", "environment.toml")
        .args(["version", "--format", "json"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let payload: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(payload["invocation"]["config_source"], "environment");
    assert_eq!(payload["invocation"]["config_path"], "environment.toml");
}

#[test]
fn report_file_keeps_stdout_clean_and_records_the_destination() {
    let directory = tempfile::tempdir().unwrap();
    let report_path = directory.path().join("version.json");
    let output = tidas()
        .args([
            "version",
            "--format",
            "json",
            "--report",
            report_path.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(output.stdout.is_empty());
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .contains("wrote version report")
    );
    let payload: serde_json::Value =
        serde_json::from_slice(&fs::read(report_path).unwrap()).unwrap();
    assert_eq!(payload["invocation"]["report_destination"], "file");
}

#[test]
fn completion_scripts_are_deterministic_and_do_not_add_a_product_command() {
    for shell in ["bash", "elvish", "fish", "powershell", "zsh"] {
        let first = tidas().args(["--completion", shell]).output().unwrap();
        let second = tidas().args(["--completion", shell]).output().unwrap();
        assert!(first.status.success(), "{shell}");
        assert!(first.stderr.is_empty(), "{shell}");
        assert_eq!(first.stdout, second.stdout, "{shell}");
        assert!(!first.stdout.is_empty(), "{shell}");
    }
}

#[test]
fn unavailable_commands_return_a_machine_report_without_python_fallback() {
    for command in [
        "convert", "import", "export", "validate", "release", "ruleset",
    ] {
        let (output, payload) = json_output(&[command, "--format", "json"]);
        assert_eq!(output.status.code(), Some(69), "{command}");
        assert!(output.stderr.is_empty(), "{command}");
        assert_eq!(payload["command"], command);
        assert_eq!(payload["exit_class"], "unavailable");
        assert_eq!(payload["completeness"], "not-started");
        assert_eq!(payload["diagnostics"][0]["code"], "feature_not_migrated");
    }
}

#[test]
fn malformed_usage_is_actionable_and_uses_the_usage_exit_class() {
    for args in [
        vec![],
        vec!["unknown"],
        vec!["--memory-budget-mib", "0", "version"],
        vec!["--queue-capacity", "0", "version"],
        vec!["--completion", "bash", "version"],
        vec!["--completion", "bash", "--report", "completion.txt"],
    ] {
        let output = tidas().args(args).output().unwrap();
        assert_eq!(output.status.code(), Some(64));
        let stderr = String::from_utf8(output.stderr).unwrap();
        assert!(stderr.contains("Usage:") || stderr.contains("try '--help'"));
    }
}

#[test]
fn io_failures_use_the_io_exit_class() {
    let directory = tempfile::tempdir().unwrap();
    let missing_parent = directory.path().join("missing").join("report.json");
    let output = tidas()
        .args([
            "version",
            "--format",
            "json",
            "--report",
            missing_parent.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(74));
    assert!(output.stdout.is_empty());
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .contains("failed to render its report")
    );
}

#[test]
fn migration_parity_fixture_matches_the_rust_surface() {
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/cli-parity-v1.json")).unwrap();
    assert_eq!(fixture["schema_version"], "tidas.cli-parity.v1");
    for case in fixture["command_cases"].as_array().unwrap() {
        let args: Vec<_> = case["rust_args"]
            .as_array()
            .unwrap()
            .iter()
            .map(|value| value.as_str().unwrap())
            .collect();
        let output = tidas().args(args).output().unwrap();
        let expected_exit_code =
            i32::try_from(case["expected_exit_code"].as_i64().unwrap()).unwrap();
        assert_eq!(
            output.status.code(),
            Some(expected_exit_code),
            "{}",
            case["name"]
        );
        if let Some(exit_class) = case["expected_exit_class"].as_str() {
            let payload: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
            assert_eq!(payload["exit_class"], exit_class, "{}", case["name"]);
            assert_eq!(
                payload["completeness"], case["expected_completeness"],
                "{}",
                case["name"]
            );
        }
    }
    for case in fixture["contract_cases"].as_array().unwrap() {
        let report = OperationReportV1::completed_with_issues(
            CommandNameV1::Validate,
            DiagnosticV1::new(
                case["diagnostic_code"].as_str().unwrap(),
                "The frozen Python oracle reported a data issue.",
            ),
        );
        let payload: serde_json::Value =
            serde_json::from_slice(&report.to_canonical_json_line().unwrap()).unwrap();
        assert_eq!(payload["command"], case["command"], "{}", case["name"]);
        assert_eq!(
            payload["status"], case["expected_status"],
            "{}",
            case["name"]
        );
        assert_eq!(
            payload["exit_class"], case["expected_exit_class"],
            "{}",
            case["name"]
        );
        assert_eq!(
            payload["completeness"], case["expected_completeness"],
            "{}",
            case["name"]
        );
        assert_eq!(
            report.exit_class.code(),
            u8::try_from(case["expected_exit_code"].as_u64().unwrap()).unwrap(),
            "{}",
            case["name"]
        );
    }
}
