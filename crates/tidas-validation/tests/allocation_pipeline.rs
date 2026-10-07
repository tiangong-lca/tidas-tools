use serde_json::{Value, json};
use std::{fs, path::Path};
use tidas_runtime::{CancellationToken, MemoryBudget};
use tidas_validation::{ValidationIssueEventV1, ValidationRequest, validate_tidas_package};

fn request(root: &Path) -> ValidationRequest {
    ValidationRequest {
        input_dir: root.to_owned(),
        issue_spool: Some(root.join("issues.jsonl")),
        cancellation: CancellationToken::default(),
        memory_budget: MemoryBudget::new(16 * 1024 * 1024),
        queue_capacity: 16,
        progress: None,
    }
}
fn fixture(root: &Path, kind: &str, direction: &str) {
    fs::create_dir_all(root.join("flows")).unwrap();
    fs::create_dir_all(root.join("processes")).unwrap();
    let mut process: Value =
        serde_json::from_str(include_str!("fixtures/allocation-process-template.json")).unwrap();
    let mut flow: Value =
        serde_json::from_str(include_str!("fixtures/flow-name-schema-v1/template.json")).unwrap();
    flow["flowDataSet"]["flowInformation"]["dataSetInformation"]["name"] = json!({"baseName":{"@xml:lang":"en","#text":"Allocation fixture"},"treatmentStandardsRoutes":{"@xml:lang":"en","#text":"Fixture route"},"mixAndLocationTypes":{"@xml:lang":"en","#text":"Global"}});
    flow["flowDataSet"]["modellingAndValidation"]["LCIMethod"]["typeOfDataSet"] = json!(kind);
    process["processDataSet"]["processInformation"]["quantitativeReference"] =
        json!({"@type":"Reference flow(s)","referenceToReferenceFlow":"0"});
    for row in process["processDataSet"]["exchanges"]["exchange"]
        .as_array_mut()
        .unwrap()
    {
        row["referenceToFlowDataSet"]["@refObjectId"] =
            json!("11111111-1111-4111-8111-111111111111");
        row["referenceToFlowDataSet"]["@version"] = json!("00.00.001");
    }
    process["processDataSet"]["exchanges"]["exchange"][0]["exchangeDirection"] = json!(direction);
    process["processDataSet"]["exchanges"]["exchange"][1]["allocations"] =
        json!({"allocation":{"@internalReferenceToCoProduct":"0","@allocatedFraction":"100"}});
    fs::write(
        root.join("flows/exact.json"),
        serde_json::to_vec(&flow).unwrap(),
    )
    .unwrap();
    fs::write(
        root.join("processes/process.json"),
        serde_json::to_vec(&process).unwrap(),
    )
    .unwrap();
}
fn issues(root: &Path) -> Vec<ValidationIssueEventV1> {
    fs::read_to_string(root.join("issues.jsonl"))
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}
#[test]
fn exact_product_waste_input_output_package_coverage_is_complete() {
    for kind in ["Product flow", "Waste flow"] {
        for direction in ["Input", "Output"] {
            let temp = tempfile::tempdir().unwrap();
            fixture(temp.path(), kind, direction);
            let output = validate_tidas_package(&request(temp.path())).unwrap();
            let schema: Value =
                serde_json::from_str(tidas_validation::VALIDATION_SUMMARY_JSON_SCHEMA_V1).unwrap();
            assert!(
                jsonschema::validator_for(&schema)
                    .unwrap()
                    .is_valid(&serde_json::to_value(&output.summary).unwrap())
            );
            let coverage = output.summary.semantic_coverage.unwrap();
            assert!(coverage.complete);
            assert_eq!(coverage.checks["allocation-target-type"].passed, 1);
            assert!(
                output.summary.ok,
                "{kind}/{direction}: {:?}",
                issues(temp.path())
            );
        }
    }
}
#[test]
fn missing_inexact_ambiguous_and_elementary_metadata_never_claim_full_pass() {
    for mode in ["missing", "wrong-version", "ambiguous", "elementary"] {
        let temp = tempfile::tempdir().unwrap();
        fixture(temp.path(), "Waste flow", "Input");
        let path = temp.path().join("flows/exact.json");
        if mode == "missing" {
            fs::remove_file(&path).unwrap();
        } else if mode == "ambiguous" {
            fs::copy(&path, temp.path().join("flows/duplicate.json")).unwrap();
        } else {
            let mut flow: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
            if mode == "wrong-version" {
                flow["flowDataSet"]["administrativeInformation"]["publicationAndOwnership"]["common:dataSetVersion"] =
                    json!("99.00.001");
            } else {
                flow["flowDataSet"]["modellingAndValidation"]["LCIMethod"]["typeOfDataSet"] =
                    json!("Elementary flow");
            }
            fs::write(&path, serde_json::to_vec(&flow).unwrap()).unwrap();
        }
        let output = validate_tidas_package(&request(temp.path())).unwrap();
        let coverage = output.summary.semantic_coverage.unwrap();
        assert!(!output.summary.ok);
        assert_eq!(coverage.complete, mode == "elementary");
        let expected = match mode {
            "missing" => "allocation_flow_evidence_unavailable",
            "wrong-version" => "allocation_flow_version_mismatch",
            "ambiguous" => "allocation_flow_evidence_ambiguous",
            _ => "allocation_target_flow_type_invalid",
        };
        assert!(
            issues(temp.path())
                .iter()
                .any(|issue| issue.issue.issue_code == expected),
            "{mode}"
        );
    }
}

#[test]
fn drift_cancellation_and_budget_failure_do_not_publish_success() {
    let temp = tempfile::tempdir().unwrap();
    fixture(temp.path(), "Waste flow", "Input");
    let mut req = request(temp.path());
    let flow_path = temp.path().join("flows/exact.json");
    req.progress = Some(tidas_validation::ValidationProgressReporter::new(
        move |progress| {
            if progress.phase == "started" {
                fs::write(&flow_path, b"{}").unwrap();
            }
        },
    ));
    assert!(matches!(
        validate_tidas_package(&req),
        Err(tidas_validation::ValidationError::FlowEvidenceDrift(_))
    ));
    assert!(!temp.path().join("issues.jsonl").exists());
    let temp = tempfile::tempdir().unwrap();
    fixture(temp.path(), "Waste flow", "Input");
    let mut req = request(temp.path());
    req.cancellation.cancel();
    assert!(matches!(
        validate_tidas_package(&req),
        Err(tidas_validation::ValidationError::Runtime(_))
    ));
    req.cancellation = CancellationToken::default();
    req.memory_budget = MemoryBudget::new(1);
    assert!(matches!(
        validate_tidas_package(&req),
        Err(tidas_validation::ValidationError::Runtime(_))
    ));
    assert!(!temp.path().join("issues.jsonl").exists());
}

#[test]
fn batch_evidence_is_manifest_scoped_and_reports_incomplete_strict_admission() {
    use sha2::{Digest, Sha256};
    use std::fmt::Write;
    use tidas_validation::{
        BatchValidationRequest, DOCUMENT_VALIDATION_PROFILE, run_document_validation_batch,
    };
    for include_flow in [false, true] {
        let temp = tempfile::tempdir().unwrap();
        fixture(temp.path(), "Waste flow", "Input");
        let mut manifest = String::new();
        for (category, name, dataset_type, id, version) in [
            (
                "processes",
                "process",
                "process",
                "d1dcaaee-0412-41ab-bd2d-a193f2b5e553",
                "00.00.001",
            ),
            (
                "flows",
                "exact",
                "flow",
                "11111111-1111-4111-8111-111111111111",
                "00.00.001",
            ),
        ] {
            if category == "flows" && !include_flow {
                continue;
            }
            let relative = format!("{category}/{name}.json");
            let bytes = fs::read(temp.path().join(&relative)).unwrap();
            let mut hash = String::new();
            for byte in Sha256::digest(&bytes) {
                write!(&mut hash, "{byte:02x}").unwrap();
            }
            let item = json!({"document_key":format!("{dataset_type}:{id}:{version}"),"category":category,"relative_path":relative,"content_sha256":hash,"identity":{"dataset_type":dataset_type,"dataset_id":id,"dataset_version":version}});
            writeln!(&mut manifest, "{item}").unwrap();
        }
        let path = temp.path().join("manifest.jsonl");
        fs::write(&path, manifest).unwrap();
        let output = run_document_validation_batch(&BatchValidationRequest {
            validation: request(temp.path()),
            input_manifest: path,
            event_spool: Some(temp.path().join("events.jsonl")),
            profile: DOCUMENT_VALIDATION_PROFILE.to_owned(),
        })
        .unwrap();
        let mut schema: Value =
            serde_json::from_str(tidas_validation::VALIDATION_FINAL_EVENT_JSON_SCHEMA_V1).unwrap();
        schema["properties"]["fingerprints"] =
            serde_json::from_str(tidas_validation::VALIDATION_DESCRIBE_JSON_SCHEMA_V1).unwrap();
        assert!(
            jsonschema::validator_for(&schema)
                .unwrap()
                .is_valid(&serde_json::to_value(&output.final_event).unwrap())
        );
        let coverage = output.final_event.summary.semantic_coverage.unwrap();
        assert_eq!(coverage.complete, include_flow);
        assert_eq!(output.final_event.summary.error_count == 0, include_flow);
    }
}

#[test]
fn dense_reference_and_legacy_projections_obey_budget_before_spool_publication() {
    for legacy in [false, true] {
        let temp = tempfile::tempdir().unwrap();
        dense_fixture(temp.path(), legacy);
        let path = temp.path().join("processes/process.json");
        let req = request(temp.path());
        // The old linear guard fits; dense projections still exceed this budget.
        let linear = fs::metadata(&path).unwrap().len() * 72
            + fs::metadata(temp.path().join("flows/exact.json"))
                .unwrap()
                .len()
                * 8
            + 131_072;
        assert!(linear < req.memory_budget.limit(), "{legacy}: {linear}");
        let error = validate_tidas_package(&req).unwrap_err();
        assert!(
            matches!(
                error,
                tidas_validation::ValidationError::Runtime(
                    tidas_runtime::RuntimeError::BudgetExceeded { .. }
                )
            ),
            "{error}"
        );
        assert!(!temp.path().join("issues.jsonl").exists());
    }
}

fn dense_fixture(root: &Path, legacy: bool) {
    fixture(root, "Waste flow", "Output");
    let path = root.join("processes/process.json");
    let mut process: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    let prototype = process["processDataSet"]["exchanges"]["exchange"][0].clone();
    let rows = (0..250)
        .map(|i| {
            let mut row = prototype.clone();
            row["@dataSetInternalID"] = json!(i.to_string());
            if legacy {
                row["allocations"] = json!({"allocation":{"@allocatedFraction":"0.4"}});
            }
            row
        })
        .collect::<Vec<_>>();
    process["processDataSet"]["exchanges"]["exchange"] = json!(rows);
    if !legacy {
        process["processDataSet"]["processInformation"]["quantitativeReference"]["referenceToReferenceFlow"] =
            json!((0..250).map(|i| i.to_string()).collect::<Vec<_>>());
    }
    fs::write(&path, serde_json::to_vec(&process).unwrap()).unwrap();
}

#[test]
fn batch_dense_projections_fail_budget_without_success_events() {
    use sha2::{Digest, Sha256};
    use std::fmt::Write;
    use tidas_validation::{
        BatchValidationRequest, DOCUMENT_VALIDATION_PROFILE, run_document_validation_batch,
    };
    for legacy in [false, true] {
        let temp = tempfile::tempdir().unwrap();
        dense_fixture(temp.path(), legacy);
        let bytes = fs::read(temp.path().join("processes/process.json")).unwrap();
        let mut hash = String::new();
        for byte in Sha256::digest(&bytes) {
            write!(&mut hash, "{byte:02x}").unwrap();
        }
        let manifest = temp.path().join("manifest.jsonl");
        let item = json!({"document_key":"process:dense:00.00.001","category":"processes","relative_path":"processes/process.json","content_sha256":hash,"identity":{"dataset_type":"process","dataset_id":"d1dcaaee-0412-41ab-bd2d-a193f2b5e553","dataset_version":"00.00.001"}});
        fs::write(&manifest, format!("{item}\n")).unwrap();
        let events = temp.path().join("events.jsonl");
        let result = run_document_validation_batch(&BatchValidationRequest {
            validation: request(temp.path()),
            input_manifest: manifest,
            event_spool: Some(events.clone()),
            profile: DOCUMENT_VALIDATION_PROFILE.to_owned(),
        });
        assert!(matches!(
            result,
            Err(tidas_validation::ValidationError::Runtime(
                tidas_runtime::RuntimeError::BudgetExceeded { .. }
            ))
        ));
        assert!(!events.exists());
    }
}
