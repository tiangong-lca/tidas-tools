use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use tidas_assets::{AssetKind, bundled_assets};
use tidas_runtime::{CancellationToken, MemoryBudget};
use tidas_validation::{ValidationIssueEventV1, ValidationRequest, validate_ilcd_package};

#[derive(Debug, Deserialize)]
struct Expectations {
    schema_version: String,
    python_oracle: String,
    input_format: String,
    document_count: u64,
    issue_codes: Vec<String>,
    expected_exit_code: u8,
}

fn fixture_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/ilcd-parity-v1")
}

#[test]
fn rust_ilcd_validation_matches_frozen_python_success_and_malformed_semantics() {
    let expected: Expectations =
        serde_json::from_slice(&fs::read(fixture_root().join("expectations.json")).unwrap())
            .unwrap();
    let directory = tempfile::tempdir().unwrap();
    let valid = bundled_assets()
        .into_iter()
        .find(|asset| {
            asset.kind == AssetKind::XmlReference
                && asset.path.ends_with("ILCDLocations_Reference.xml")
        })
        .unwrap();
    fs::write(directory.path().join("a-valid.xml"), valid.bytes).unwrap();
    fs::write(
        directory.path().join("b-schema-invalid.xml"),
        br#"<ILCDLocations xmlns="http://lca.jrc.it/ILCD/Locations"/>"#,
    )
    .unwrap();
    fs::write(directory.path().join("c-malformed.xml"), b"<ILCDLocations>").unwrap();
    let spool = directory.path().join("issues.jsonl");

    let output = validate_ilcd_package(&ValidationRequest {
        input_dir: directory.path().to_path_buf(),
        issue_spool: Some(spool.clone()),
        cancellation: CancellationToken::default(),
        memory_budget: MemoryBudget::new(16 * 1024 * 1024),
        queue_capacity: 8,
        progress: None,
    })
    .unwrap();
    let events: Vec<ValidationIssueEventV1> = fs::read(spool)
        .unwrap()
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.is_empty())
        .map(|line| serde_json::from_slice(line).unwrap())
        .collect();
    let issue_codes: Vec<_> = events
        .iter()
        .map(|event| event.issue.issue_code.clone())
        .collect();

    assert_eq!(expected.schema_version, "tidas.ilcd-validation-parity.v1");
    assert!(!expected.python_oracle.is_empty());
    assert_eq!(output.summary.input_format, expected.input_format);
    assert_eq!(output.summary.document_count, expected.document_count);
    assert_eq!(issue_codes, expected.issue_codes);
    assert_eq!(expected.expected_exit_code, 2);
}
