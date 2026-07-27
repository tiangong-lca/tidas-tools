use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tidas_import::{ImportRequest, ImportTarget, SourceFormat, run_import};
use tidas_release::{
    RELEASE_REPORT_JSON_SCHEMA_V1, ReleaseProfile, ReleaseRequest, ReleaseRuntime, UNIT_PROFILE,
    run_release,
};
use tidas_runtime::{CancellationToken, MemoryBudget, RuntimeError};
use walkdir::WalkDir;

const UNIT_ID: &str = "11111111-1111-4111-8111-111111111111";
const FLOW_ID: &str = "22222222-2222-4222-8222-222222222222";
const MODEL_ID: &str = "c58f567c-c631-5a3a-90d9-c0cec7290cf8";
const RESULT_ID: &str = "ba3386d3-39c0-5a48-ae4d-e7ad90ec4996";
const VERSION: &str = "01.00.000";

fn runtime() -> ReleaseRuntime {
    ReleaseRuntime {
        cancellation: CancellationToken::default(),
        memory_budget: MemoryBudget::new(64 * 1024 * 1024),
        queue_capacity: 16,
    }
}

fn reference(dataset_type: &str, id: &str, category: &str) -> Value {
    json!({
        "@type": dataset_type,
        "@refObjectId": id,
        "@version": VERSION,
        "@uri": format!("../{category}/{id}_{VERSION}.json")
    })
}

fn write_dataset(
    root: &Path,
    relative: &str,
    document: &Value,
    dataset_type: &str,
    role: &str,
    id: &str,
) -> Value {
    let path = root.join(relative);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let mut bytes = serde_json::to_vec_pretty(document).unwrap();
    bytes.push(b'\n');
    fs::write(&path, &bytes).unwrap();
    json!({
        "datasetType": dataset_type,
        "role": role,
        "uuid": id,
        "version": VERSION,
        "path": relative,
        "sha256": hex(Sha256::digest(&bytes)),
        "byteSize": bytes.len(),
        "canonicalContentHash": hex(Sha256::digest(serde_json::to_vec(document).unwrap()))
    })
}

#[allow(clippy::too_many_lines)]
fn fixture(root: &Path) -> (PathBuf, PathBuf) {
    let tidas = root.join("tidas");
    let mut entries = vec![
        write_dataset(
            &tidas,
            &format!("flows/{FLOW_ID}_{VERSION}.json"),
            &json!({
                "flowDataSet": {
                    "flowInformation": {
                        "dataSetInformation": {"common:UUID": FLOW_ID}
                    }
                }
            }),
            "flow",
            "support",
            FLOW_ID,
        ),
        write_dataset(
            &tidas,
            &format!("processes/{UNIT_ID}_{VERSION}.json"),
            &json!({
                "processDataSet": {
                    "processInformation": {
                        "dataSetInformation": {"common:UUID": UNIT_ID}
                    },
                    "exchanges": {
                        "exchange": {
                            "referenceToFlowDataSet": reference(
                                "flow data set",
                                FLOW_ID,
                                "flows"
                            )
                        }
                    }
                }
            }),
            "process",
            "unit_process",
            UNIT_ID,
        ),
        write_dataset(
            &tidas,
            &format!("processes/{RESULT_ID}_{VERSION}.json"),
            &json!({
                "processDataSet": {
                    "processInformation": {
                        "dataSetInformation": {"common:UUID": RESULT_ID}
                    },
                    "exchanges": {
                        "exchange": {
                            "referenceToFlowDataSet": reference(
                                "flow data set",
                                FLOW_ID,
                                "flows"
                            )
                        }
                    }
                }
            }),
            "process",
            "result_process",
            RESULT_ID,
        ),
        write_dataset(
            &tidas,
            &format!("lifecyclemodels/{MODEL_ID}_{VERSION}.json"),
            &json!({
                "lifeCycleModelDataSet": {
                    "lifeCycleModelInformation": {
                        "dataSetInformation": {
                            "common:UUID": MODEL_ID,
                            "referenceToResultingProcess": reference(
                                "process data set",
                                RESULT_ID,
                                "processes"
                            )
                        },
                        "technology": {
                            "processes": {
                                "processInstance": {
                                    "referenceToProcess": reference(
                                        "process data set",
                                        UNIT_ID,
                                        "processes"
                                    )
                                }
                            }
                        }
                    }
                }
            }),
            "lifecyclemodel",
            "lifecycle_model",
            MODEL_ID,
        ),
    ];
    entries.sort_by(|left, right| {
        left["path"]
            .as_str()
            .unwrap()
            .cmp(right["path"].as_str().unwrap())
    });
    let index = root.join("canonical-dataset-index.json");
    let mut bytes = serde_json::to_vec_pretty(&json!({
        "schemaVersion": "tiangong.release.canonical-dataset-index.v1",
        "datasetCount": entries.len(),
        "byteSize": entries.iter().map(|entry| entry["byteSize"].as_u64().unwrap()).sum::<u64>(),
        "artifactSetHash": "0".repeat(64),
        "datasets": entries
    }))
    .unwrap();
    bytes.push(b'\n');
    fs::write(&index, bytes).unwrap();
    (tidas, index)
}

#[test]
fn exact_closure_and_schema_ordered_roundtrip_match_the_frozen_python_fixture() {
    let temporary = tempfile::tempdir().unwrap();
    let (tidas, index) = fixture(temporary.path());
    let closure = run_release(
        &ReleaseRequest::ValidateClosure {
            input_dir: tidas.clone(),
            dataset_index: index,
            profile: ReleaseProfile::UnitProcess,
        },
        &runtime(),
    )
    .unwrap()
    .closure
    .unwrap();
    assert_eq!(closure.profile_id, UNIT_PROFILE);
    assert_eq!(closure.dataset_count, 2);
    assert_eq!(
        closure.dataset_keys,
        [
            format!("flow:{FLOW_ID}:{VERSION}"),
            format!("process:{UNIT_ID}:{VERSION}")
        ]
    );
    assert!(!closure.dataset_keys_truncated);

    let ilcd = temporary.path().join("ilcd");
    let conversion = run_release(
        &ReleaseRequest::ConvertIlcd {
            input_dir: tidas.clone(),
            output_dir: ilcd.clone(),
        },
        &runtime(),
    )
    .unwrap()
    .conversion
    .unwrap();
    assert_eq!(conversion.dataset_count, 4);
    let process_xml =
        fs::read_to_string(ilcd.join(format!("data/processes/{UNIT_ID}_{VERSION}.xml"))).unwrap();
    assert!(
        process_xml.find("processInformation").unwrap() < process_xml.find("exchanges").unwrap()
    );
    assert!(process_xml.contains(&format!("{FLOW_ID}_{VERSION}.xml")));

    let roundtrip = run_release(
        &ReleaseRequest::SemanticRoundtrip {
            tidas_dir: tidas,
            ilcd_dir: ilcd,
        },
        &runtime(),
    )
    .unwrap()
    .roundtrip
    .unwrap();
    assert!(roundtrip.ok, "{:?}", roundtrip.mismatches);
    assert_eq!(roundtrip.dataset_count, 4);
}

#[test]
fn missing_exact_reference_version_fails_closed() {
    let temporary = tempfile::tempdir().unwrap();
    let (tidas, index) = fixture(temporary.path());
    let unit_path = tidas.join(format!("processes/{UNIT_ID}_{VERSION}.json"));
    let mut document: Value = serde_json::from_slice(&fs::read(&unit_path).unwrap()).unwrap();
    document["processDataSet"]["exchanges"]["exchange"]["referenceToFlowDataSet"]
        .as_object_mut()
        .unwrap()
        .remove("@version");
    let mut body = serde_json::to_vec_pretty(&document).unwrap();
    body.push(b'\n');
    fs::write(&unit_path, &body).unwrap();
    let mut index_value: Value = serde_json::from_slice(&fs::read(&index).unwrap()).unwrap();
    let entry = index_value["datasets"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|entry| entry["uuid"] == UNIT_ID)
        .unwrap();
    entry["sha256"] = Value::String(hex(Sha256::digest(&body)));
    fs::write(&index, serde_json::to_vec_pretty(&index_value).unwrap()).unwrap();

    let error = run_release(
        &ReleaseRequest::ValidateClosure {
            input_dir: tidas,
            dataset_index: index,
            profile: ReleaseProfile::UnitProcess,
        },
        &runtime(),
    )
    .unwrap_err();
    assert!(matches!(
        error,
        tidas_release::ReleaseError::ReferenceVersionMissing(_)
    ));
}

#[test]
fn large_closure_report_is_capped_and_explicitly_marked_truncated() {
    let temporary = tempfile::tempdir().unwrap();
    let tidas = temporary.path().join("tidas");
    let mut entries = Vec::new();
    for index in 0..257 {
        let id = format!("00000000-0000-4000-8000-{index:012}");
        entries.push(write_dataset(
            &tidas,
            &format!("flows/{id}_{VERSION}.json"),
            &json!({
                "flowDataSet": {
                    "flowInformation": {
                        "dataSetInformation": {"common:UUID": id.clone()}
                    }
                }
            }),
            "flow",
            "unit_process",
            &id,
        ));
    }
    let index_path = temporary.path().join("large-index.json");
    let mut index_bytes = serde_json::to_vec_pretty(&json!({
        "schemaVersion": "tiangong.release.canonical-dataset-index.v1",
        "datasetCount": entries.len(),
        "byteSize": entries.iter().map(|entry| entry["byteSize"].as_u64().unwrap()).sum::<u64>(),
        "artifactSetHash": "0".repeat(64),
        "datasets": entries
    }))
    .unwrap();
    index_bytes.push(b'\n');
    fs::write(&index_path, index_bytes).unwrap();
    let closure = run_release(
        &ReleaseRequest::ValidateClosure {
            input_dir: tidas,
            dataset_index: index_path,
            profile: ReleaseProfile::UnitProcess,
        },
        &runtime(),
    )
    .unwrap()
    .closure
    .unwrap();
    assert_eq!(closure.dataset_count, 257);
    assert_eq!(closure.dataset_keys.len(), 256);
    assert!(closure.dataset_keys_truncated);
}

#[test]
fn cancellation_never_publishes_conversion_output() {
    let temporary = tempfile::tempdir().unwrap();
    let (tidas, _) = fixture(temporary.path());
    let output = temporary.path().join("cancelled");
    fs::create_dir(&output).unwrap();
    fs::write(output.join("sentinel"), b"keep").unwrap();
    let cancellation = CancellationToken::default();
    cancellation.cancel();
    let runtime = ReleaseRuntime {
        cancellation,
        memory_budget: MemoryBudget::new(64 * 1024 * 1024),
        queue_capacity: 16,
    };
    let error = run_release(
        &ReleaseRequest::ConvertIlcd {
            input_dir: tidas,
            output_dir: output.clone(),
        },
        &runtime,
    )
    .unwrap_err();
    assert!(matches!(
        error,
        tidas_release::ReleaseError::Runtime(RuntimeError::Cancelled)
    ));
    assert_eq!(fs::read(output.join("sentinel")).unwrap(), b"keep");
}

#[test]
fn output_ancestor_or_input_directory_is_rejected_before_mutation() {
    let temporary = tempfile::tempdir().unwrap();
    let input = temporary.path().join("input");
    fs::create_dir(&input).unwrap();
    fs::write(input.join("sentinel"), b"keep").unwrap();
    let error = run_release(
        &ReleaseRequest::ConvertIlcd {
            input_dir: input.clone(),
            output_dir: temporary.path().to_path_buf(),
        },
        &runtime(),
    )
    .unwrap_err();
    assert!(matches!(
        error,
        tidas_release::ReleaseError::OutputInsideInput(_)
    ));
    assert_eq!(fs::read(input.join("sentinel")).unwrap(), b"keep");

    let same_error = run_release(
        &ReleaseRequest::ConvertIlcd {
            input_dir: input.clone(),
            output_dir: input.clone(),
        },
        &runtime(),
    )
    .unwrap_err();
    assert!(matches!(
        same_error,
        tidas_release::ReleaseError::OutputInsideInput(_)
    ));
    assert_eq!(fs::read(input.join("sentinel")).unwrap(), b"keep");
}

#[test]
fn native_valid_tree_builds_four_repeatable_self_contained_packages() {
    let temporary = tempfile::tempdir().unwrap();
    let source = temporary.path().join("source.csv");
    fs::write(
        &source,
        b"{SimaPro 9.5}\n{CSV separator: semicolon}\n\nProcess\n\nProcess name\nSteel production\n\nComment\nrelease fixture\n\nProducts\nSteel | production route | GLO;kg;1\n\nEmissions to air\nCarbon dioxide;air;kg;2.5\n\nEnd\n",
    )
    .unwrap();
    let imported = temporary.path().join("imported");
    run_import(&ImportRequest {
        source,
        requested_format: Some(SourceFormat::SimaproCsv),
        output_dir: imported.clone(),
        target: ImportTarget::Tidas,
        write_mapping: false,
        write_process_bundles: false,
        cancellation: CancellationToken::default(),
        memory_budget: MemoryBudget::new(128 * 1024 * 1024),
        queue_capacity: 16,
        max_entry_bytes: 8 * 1024 * 1024,
        max_issue_bytes: 64 * 1024,
    })
    .unwrap();
    let tidas = imported.join("tidas");
    let index = write_index_for_valid_package(&tidas, temporary.path());

    let first_output = temporary.path().join("release-a");
    let first_report = run_release(
        &ReleaseRequest::BuildPackages {
            tidas_dir: tidas.clone(),
            dataset_index: index.clone(),
            output_dir: first_output.clone(),
        },
        &runtime(),
    )
    .unwrap();
    let schema: Value = serde_json::from_str(RELEASE_REPORT_JSON_SCHEMA_V1).unwrap();
    let validator = jsonschema::validator_for(&schema).unwrap();
    assert!(validator.is_valid(&serde_json::to_value(&first_report).unwrap()));
    let first = first_report.build.unwrap();
    assert_eq!(first.packages.len(), 4);
    assert!(first.packages.iter().all(|package| package.self_contained));
    assert!(first.roundtrip.ok);
    assert!(first.tidas_validation.summary.ok);
    assert!(first.ilcd_validation.summary.ok);
    assert_eq!(
        fs::read_dir(&first_output).unwrap().count(),
        4,
        "only the four final archives are published"
    );

    let second = run_release(
        &ReleaseRequest::BuildPackages {
            tidas_dir: tidas,
            dataset_index: index,
            output_dir: temporary.path().join("release-b"),
        },
        &runtime(),
    )
    .unwrap()
    .build
    .unwrap();
    assert_eq!(first.artifact_set_sha256, second.artifact_set_sha256);
    assert_eq!(
        first
            .packages
            .iter()
            .map(|package| &package.artifact.sha256)
            .collect::<Vec<_>>(),
        second
            .packages
            .iter()
            .map(|package| &package.artifact.sha256)
            .collect::<Vec<_>>()
    );
}

fn write_index_for_valid_package(tidas: &Path, root: &Path) -> PathBuf {
    let mut entries = Vec::new();
    for item in WalkDir::new(tidas).min_depth(2).max_depth(2) {
        let item = item.unwrap();
        if !item.file_type().is_file()
            || item.path().extension().and_then(|value| value.to_str()) != Some("json")
        {
            continue;
        }
        let relative = item.path().strip_prefix(tidas).unwrap();
        let category = relative
            .parent()
            .and_then(Path::file_name)
            .and_then(|value| value.to_str())
            .unwrap();
        let dataset_type = match category {
            "contacts" => "contact",
            "flows" => "flow",
            "flowproperties" => "flowproperty",
            "unitgroups" => "unitgroup",
            "processes" => "process",
            "sources" => "source",
            "lciamethods" => "lciamethod",
            "lifecyclemodels" => "lifecyclemodel",
            other => panic!("unexpected category {other}"),
        };
        let bytes = fs::read(item.path()).unwrap();
        let document: Value = serde_json::from_slice(&bytes).unwrap();
        let uuid = find_string(&document, "common:UUID").unwrap();
        let version =
            find_string(&document, "common:dataSetVersion").unwrap_or_else(|| VERSION.to_owned());
        let role = match category {
            "flows" => "unit_process",
            "processes" => "result_process",
            _ => "support",
        };
        entries.push(json!({
            "datasetType": dataset_type,
            "role": role,
            "uuid": uuid,
            "version": version,
            "path": relative.to_string_lossy().replace('\\', "/"),
            "sha256": hex(Sha256::digest(&bytes)),
            "byteSize": bytes.len(),
            "canonicalContentHash": hex(Sha256::digest(serde_json::to_vec(&document).unwrap()))
        }));
    }
    entries.sort_by(|left, right| {
        left["path"]
            .as_str()
            .unwrap()
            .cmp(right["path"].as_str().unwrap())
    });
    let path = root.join("valid-canonical-dataset-index.json");
    let mut bytes = serde_json::to_vec_pretty(&json!({
        "schemaVersion": "tiangong.release.canonical-dataset-index.v1",
        "datasetCount": entries.len(),
        "byteSize": entries.iter().map(|entry| entry["byteSize"].as_u64().unwrap()).sum::<u64>(),
        "artifactSetHash": "0".repeat(64),
        "datasets": entries
    }))
    .unwrap();
    bytes.push(b'\n');
    fs::write(&path, bytes).unwrap();
    path
}

fn find_string(value: &Value, key: &str) -> Option<String> {
    match value {
        Value::Object(object) => object
            .get(key)
            .and_then(Value::as_str)
            .map(ToOwned::to_owned)
            .or_else(|| object.values().find_map(|child| find_string(child, key))),
        Value::Array(items) => items.iter().find_map(|child| find_string(child, key)),
        _ => None,
    }
}

fn hex(bytes: impl AsRef<[u8]>) -> String {
    bytes.as_ref().iter().fold(
        String::with_capacity(bytes.as_ref().len() * 2),
        |mut output, byte| {
            write!(&mut output, "{byte:02x}").unwrap();
            output
        },
    )
}
