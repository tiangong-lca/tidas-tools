use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use tidas_runtime::{CancellationToken, MemoryBudget};
use tidas_validation::{ValidationIssueEventV1, ValidationRequest, validate_tidas_package};

#[derive(Debug, Deserialize)]
struct Expectations {
    schema_version: String,
    python_oracle: String,
    input_format: String,
    category: String,
    document_count: u64,
    issue_count: u64,
    issue_code: String,
    severity: String,
    file_path: String,
    expected_exit_code: u8,
}

fn fixture_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/parity-v1")
}

#[test]
fn rust_validation_matches_the_frozen_python_semantics() {
    let root = fixture_root();
    let expected: Expectations =
        serde_json::from_slice(&fs::read(root.join("expectations.json")).unwrap()).unwrap();
    let temporary = tempfile::tempdir().unwrap();
    let issue_spool = temporary.path().join("issues.jsonl");
    let output = validate_tidas_package(&ValidationRequest {
        input_dir: root.join("package"),
        issue_spool: Some(issue_spool.clone()),
        cancellation: CancellationToken::default(),
        memory_budget: MemoryBudget::new(16 * 1024 * 1024),
        queue_capacity: 16,
        progress: None,
    })
    .unwrap();
    let events: Vec<ValidationIssueEventV1> = fs::read(issue_spool)
        .unwrap()
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.is_empty())
        .map(|line| serde_json::from_slice(line).unwrap())
        .collect();

    assert_eq!(expected.schema_version, "tidas.validation-parity.v1");
    assert!(!expected.python_oracle.is_empty());
    assert_eq!(output.summary.input_format, expected.input_format);
    assert_eq!(output.summary.document_count, expected.document_count);
    assert_eq!(output.summary.issue_count, expected.issue_count);
    assert_eq!(output.summary.categories[0].category, expected.category);
    assert_eq!(events.len() as u64, expected.issue_count);
    assert_eq!(events[0].issue.issue_code, expected.issue_code);
    assert_eq!(
        serde_json::to_value(events[0].issue.severity).unwrap(),
        expected.severity
    );
    assert_eq!(events[0].issue.file_path, expected.file_path);
    assert_eq!(expected.expected_exit_code, 2);
}
