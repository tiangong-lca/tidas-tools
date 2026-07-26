use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value;
use tempfile::tempdir;
use tidas_import::{ImportRequest, ImportTarget, run_import};
use tidas_runtime::{CancellationToken, MemoryBudget};

const SOURCE: &[u8] = include_bytes!("fixtures/python-oracle-v1/simapro.csv");
const EXPECTED: &str = include_str!("fixtures/python-oracle-v1/simapro.expected.json");

#[test]
fn frozen_python_simapro_oracle_matches_complete_rust_import() {
    let directory = tempdir().unwrap();
    let source = directory.path().join("simapro.csv");
    fs::write(&source, SOURCE).unwrap();
    let output = directory.path().join("output");
    let first = run_fixture(&source, &output);
    let first_mapping = fs::read(output.join("mapping.csv.gz")).unwrap();
    let first_semantics = emitted_semantics(&output);
    let second = run_fixture(&source, &output);
    let second_mapping = fs::read(output.join("mapping.csv.gz")).unwrap();
    let second_semantics = emitted_semantics(&output);
    let relocated_directory = tempdir().unwrap();
    let relocated_source = relocated_directory.path().join("simapro.csv");
    let relocated_output = relocated_directory.path().join("output");
    fs::write(&relocated_source, SOURCE).unwrap();
    let relocated = run_fixture(&relocated_source, &relocated_output);
    let expected: Value = serde_json::from_str(EXPECTED).unwrap();

    assert_eq!(first.detected_format.as_str(), expected["source_format"]);
    assert_eq!(
        serde_json::to_value(&first.object_counts).unwrap(),
        expected["object_counts"]
    );
    assert_eq!(first.warning_count, expected["warning_count"]);
    assert_eq!(first.error_count, expected["error_count"]);
    assert_eq!(first.tidas_validation_issue_count, 0);
    assert_eq!(first.ilcd_validation_issue_count, Some(0));
    assert_eq!(first_semantics, expected["process"]);
    assert_eq!(second_semantics, first_semantics);
    assert_repeated_outputs_match(&first, &second, &first_mapping, &second_mapping);
    assert_relocated_outputs_match(&first, &relocated, &first_mapping, &relocated_output);

    let generated = &expected["generated_unit"];
    assert!(
        output
            .join("tidas/unitgroups")
            .join(json_name(generated, "unit_group_id"))
            .is_file()
    );
    assert!(
        output
            .join("tidas/flowproperties")
            .join(json_name(generated, "flow_property_id"))
            .is_file()
    );
    for (category, key) in [
        ("contacts", "contact_id"),
        ("sources", "format_source_id"),
        ("sources", "compliance_source_id"),
    ] {
        assert!(
            output
                .join("tidas")
                .join(category)
                .join(json_name(&expected["fixed_dependencies"], key))
                .is_file()
        );
    }
    assert!(output.join("process-bundles/index.json").is_file());
}

fn run_fixture(source: &Path, output: &Path) -> tidas_import::ImportExecutionReportV1 {
    run_import(&ImportRequest {
        source: source.to_path_buf(),
        requested_format: None,
        output_dir: output.to_path_buf(),
        target: ImportTarget::Both,
        write_mapping: true,
        write_process_bundles: true,
        cancellation: CancellationToken::default(),
        memory_budget: MemoryBudget::new(32 * 1024 * 1024),
        queue_capacity: 2,
        max_entry_bytes: 1024 * 1024,
        max_issue_bytes: 64 * 1024,
    })
    .unwrap()
}

fn assert_repeated_outputs_match(
    first: &tidas_import::ImportExecutionReportV1,
    second: &tidas_import::ImportExecutionReportV1,
    first_mapping: &[u8],
    second_mapping: &[u8],
) {
    assert_eq!(
        first.tidas_package.output_tree_sha256,
        second.tidas_package.output_tree_sha256
    );
    assert_eq!(
        first.ilcd_conversion.as_ref().unwrap().output_tree_sha256,
        second.ilcd_conversion.as_ref().unwrap().output_tree_sha256
    );
    assert_eq!(
        first.mapping.as_ref().unwrap().output_sha256,
        second.mapping.as_ref().unwrap().output_sha256
    );
    assert_eq!(first_mapping, second_mapping);
}

fn assert_relocated_outputs_match(
    first: &tidas_import::ImportExecutionReportV1,
    relocated: &tidas_import::ImportExecutionReportV1,
    first_mapping: &[u8],
    relocated_output: &Path,
) {
    assert_eq!(
        first.tidas_package.output_tree_sha256,
        relocated.tidas_package.output_tree_sha256
    );
    assert_eq!(
        first.ilcd_conversion.as_ref().unwrap().output_tree_sha256,
        relocated
            .ilcd_conversion
            .as_ref()
            .unwrap()
            .output_tree_sha256
    );
    assert_eq!(
        first.mapping.as_ref().unwrap().output_sha256,
        relocated.mapping.as_ref().unwrap().output_sha256
    );
    assert_eq!(
        first_mapping,
        fs::read(relocated_output.join("mapping.csv.gz")).unwrap()
    );
}

fn json_name(value: &Value, key: &str) -> String {
    format!("{}.json", value[key].as_str().unwrap())
}

fn emitted_semantics(output: &Path) -> Value {
    let process_path = only_json(output.join("tidas/processes"));
    let process: Value = serde_json::from_slice(&fs::read(process_path).unwrap()).unwrap();
    let dataset = &process["processDataSet"];
    let exchanges = dataset["exchanges"]["exchange"]
        .as_array()
        .unwrap()
        .iter()
        .map(|exchange| {
            serde_json::json!({
                "flow": exchange["referenceToFlowDataSet"]["common:shortDescription"]["#text"],
                "direction": exchange["exchangeDirection"],
                "amount": exchange["meanAmount"],
            })
        })
        .collect::<Vec<_>>();
    serde_json::json!({
        "name": dataset["processInformation"]["dataSetInformation"]["name"]["baseName"]["#text"],
        "functional_unit": dataset["processInformation"]["quantitativeReference"]["functionalUnitOrOther"]["#text"],
        "exchanges": exchanges,
    })
}

fn only_json(directory: PathBuf) -> PathBuf {
    let mut files = fs::read_dir(directory)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "json")
        })
        .collect::<Vec<_>>();
    files.sort();
    assert_eq!(files.len(), 1);
    files.pop().unwrap()
}
