//! Side-effect-free extraction of version-preserving TIDAS reference edges.
//!
//! This crate identifies references and extraction defects only. Target
//! existence, visibility, version selection, closure, and certificates belong
//! to downstream resolvers.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

pub const REFERENCE_EXTRACTION_SCHEMA_VERSION: &str = "tidas.reference-extraction-result.v1";
pub const REFERENCE_EDGE_SCHEMA_VERSION: &str = "tidas.reference-edge.v1";
pub const REFERENCE_ISSUE_SCHEMA_VERSION: &str = "tidas.reference-extraction-issue.v1";
pub const REFERENCE_EXTRACTION_JSON_SCHEMA_V1: &str =
    include_str!("../../../contracts/reference-extraction-result.v1.schema.json");

pub const REFERENCE_ROLE_PROCESS_EXCHANGE_FLOW: &str = "process_exchange_flow";
pub const REFERENCE_ROLE_LCIA_FACTOR_FLOW: &str = "lcia_factor_flow";
pub const REFERENCE_ROLE_LIFECYCLE_MODEL_PROCESS: &str = "lifecycle_model_process";
pub const REFERENCE_ROLE_SUPPORT_DOCUMENT: &str = "support_document";
pub const REFERENCE_ROLES: [&str; 4] = [
    REFERENCE_ROLE_LCIA_FACTOR_FLOW,
    REFERENCE_ROLE_LIFECYCLE_MODEL_PROCESS,
    REFERENCE_ROLE_PROCESS_EXCHANGE_FLOW,
    REFERENCE_ROLE_SUPPORT_DOCUMENT,
];

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ReferenceEdgeV1 {
    pub schema_version: String,
    pub document_key: String,
    pub source_category: String,
    pub target_category: String,
    pub target_uuid: String,
    pub requested_version_state: String,
    pub requested_version: Option<String>,
    pub requested_version_raw: Value,
    pub reference_role: String,
    pub json_path: String,
    pub raw_type: Value,
    pub uri: Value,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ReferenceExtractionIssueV1 {
    pub schema_version: String,
    pub issue_code: String,
    pub severity: String,
    pub document_key: String,
    pub source_category: String,
    pub json_path: String,
    pub reference_role: String,
    pub message: String,
    pub details: BTreeMap<String, Value>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ReferenceExtractionResultV1 {
    pub schema_version: String,
    pub document_key: String,
    pub source_category: String,
    pub edges: Vec<ReferenceEdgeV1>,
    pub issues: Vec<ReferenceExtractionIssueV1>,
}

pub fn extract_references(
    document_key: &str,
    category: &str,
    payload: &Value,
) -> Result<ReferenceExtractionResultV1, ReferenceExtractionError> {
    if document_key.is_empty() {
        return Err(ReferenceExtractionError::EmptyDocumentKey);
    }
    if category.is_empty() {
        return Err(ReferenceExtractionError::EmptyCategory);
    }
    let mut output = ReferenceExtractionResultV1 {
        schema_version: REFERENCE_EXTRACTION_SCHEMA_VERSION.to_owned(),
        document_key: document_key.to_owned(),
        source_category: category.to_owned(),
        edges: Vec::new(),
        issues: Vec::new(),
    };
    walk_references(payload, "$", None, &mut output);
    Ok(output)
}

fn walk_references(
    node: &Value,
    path: &str,
    parent_key: Option<&str>,
    output: &mut ReferenceExtractionResultV1,
) {
    match node {
        Value::Object(object) => {
            if looks_like_reference(object, parent_key) {
                extract_reference(object, path, parent_key, output);
            }
            for (key, value) in object {
                walk_references(value, &format!("{path}.{key}"), Some(key), output);
            }
        }
        Value::Array(items) => {
            for (index, item) in items.iter().enumerate() {
                walk_references(item, &format!("{path}[{index}]"), parent_key, output);
            }
        }
        _ => {}
    }
}

fn looks_like_reference(object: &serde_json::Map<String, Value>, parent_key: Option<&str>) -> bool {
    object.contains_key("@refObjectId")
        || object.contains_key("@uri")
        || parent_key.is_some_and(|key| key.to_ascii_lowercase().contains("referenceto"))
}

fn extract_reference(
    reference: &serde_json::Map<String, Value>,
    path: &str,
    parent_key: Option<&str>,
    output: &mut ReferenceExtractionResultV1,
) {
    let raw_type = reference.get("@type").cloned().unwrap_or(Value::Null);
    let uri = reference.get("@uri").cloned().unwrap_or(Value::Null);
    let target_category = category_from_type(&raw_type).or_else(|| category_from_uri(&uri));
    let role = reference_role(&output.source_category, path, parent_key, target_category);

    if target_category.is_none() {
        output.issues.push(issue(
            output,
            "reference_type_unresolved",
            path,
            role,
            "Reference target type cannot be resolved from @type or @uri.",
            [("raw_type", raw_type.clone()), ("uri", uri.clone())],
        ));
    }

    let raw_id = reference
        .get("@refObjectId")
        .cloned()
        .unwrap_or(Value::Null);
    let Some(raw_id_text) = raw_id.as_str().filter(|value| !value.trim().is_empty()) else {
        output.issues.push(issue(
            output,
            "reference_object_id_missing",
            path,
            role,
            "Recognized reference is missing a non-empty @refObjectId.",
            [
                ("raw_ref_object_id", raw_id),
                ("raw_type", raw_type),
                ("uri", uri),
            ],
        ));
        return;
    };
    let target_uuid = raw_id_text.trim().to_owned();
    if !is_canonical_uuid(&target_uuid) {
        output.issues.push(issue(
            output,
            "reference_uuid_invalid",
            path,
            role,
            "Reference @refObjectId is not a canonical lowercase UUID.",
            [(
                "raw_ref_object_id",
                reference
                    .get("@refObjectId")
                    .cloned()
                    .unwrap_or(Value::Null),
            )],
        ));
    }

    let raw_version = reference.get("@version").cloned().unwrap_or(Value::Null);
    let (version_state, requested_version) = if raw_version.is_null() {
        ("omitted", None)
    } else if let Some(version) = raw_version.as_str().filter(|value| is_version(value)) {
        ("explicit", Some(version.to_owned()))
    } else {
        output.issues.push(issue(
            output,
            "reference_version_invalid",
            path,
            role,
            "Reference @version must match NN.NN or NN.NN.NNN.",
            [("requested_version_raw", raw_version.clone())],
        ));
        ("invalid", raw_version.as_str().map(ToOwned::to_owned))
    };

    if let Some(target_category) = target_category {
        output.edges.push(ReferenceEdgeV1 {
            schema_version: REFERENCE_EDGE_SCHEMA_VERSION.to_owned(),
            document_key: output.document_key.clone(),
            source_category: output.source_category.clone(),
            target_category: target_category.to_owned(),
            target_uuid,
            requested_version_state: version_state.to_owned(),
            requested_version,
            requested_version_raw: raw_version,
            reference_role: role.to_owned(),
            json_path: path.to_owned(),
            raw_type,
            uri,
        });
    }
}

fn issue<const N: usize>(
    output: &ReferenceExtractionResultV1,
    issue_code: &str,
    json_path: &str,
    reference_role: &str,
    message: &str,
    details: [(&str, Value); N],
) -> ReferenceExtractionIssueV1 {
    ReferenceExtractionIssueV1 {
        schema_version: REFERENCE_ISSUE_SCHEMA_VERSION.to_owned(),
        issue_code: issue_code.to_owned(),
        severity: "error".to_owned(),
        document_key: output.document_key.clone(),
        source_category: output.source_category.clone(),
        json_path: json_path.to_owned(),
        reference_role: reference_role.to_owned(),
        message: message.to_owned(),
        details: details
            .into_iter()
            .map(|(key, value)| (key.to_owned(), value))
            .collect(),
    }
}

fn category_from_type(raw_type: &Value) -> Option<&'static str> {
    let normalized = raw_type
        .as_str()?
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_ascii_lowercase();
    match normalized.as_str() {
        "contact" | "contact data set" => Some("contacts"),
        "flow" | "flow data set" => Some("flows"),
        "flow property" | "flow property data set" => Some("flowproperties"),
        "lcia method" | "lcia method data set" => Some("lciamethods"),
        "life cycle model"
        | "life cycle model data set"
        | "lifecycle model"
        | "lifecycle model data set" => Some("lifecyclemodels"),
        "process" | "process data set" => Some("processes"),
        "source" | "source data set" => Some("sources"),
        "unit group" | "unit group data set" => Some("unitgroups"),
        _ => None,
    }
}

fn category_from_uri(uri: &Value) -> Option<&'static str> {
    uri.as_str()?
        .split(['/', '\\'])
        .filter(|part| !part.is_empty())
        .find_map(|part| match part.to_ascii_lowercase().as_str() {
            "contacts" | "contact" => Some("contacts"),
            "flows" | "flow" => Some("flows"),
            "flowproperties" | "flowproperty" | "flow-properties" => Some("flowproperties"),
            "lciamethods" | "lciamethod" | "lcia-methods" => Some("lciamethods"),
            "lifecyclemodels" | "lifecyclemodel" | "life-cycle-models" => Some("lifecyclemodels"),
            "processes" | "process" => Some("processes"),
            "sources" | "source" => Some("sources"),
            "unitgroups" | "unitgroup" | "unit-groups" => Some("unitgroups"),
            _ => None,
        })
}

fn reference_role(
    source_category: &str,
    path: &str,
    parent_key: Option<&str>,
    target_category: Option<&str>,
) -> &'static str {
    let normalized_path = path.to_ascii_lowercase();
    let normalized_key = parent_key.unwrap_or_default().to_ascii_lowercase();
    if source_category == "processes"
        && target_category == Some("flows")
        && normalized_path.contains("exchange")
        && normalized_key == "referencetoflowdataset"
    {
        REFERENCE_ROLE_PROCESS_EXCHANGE_FLOW
    } else if source_category == "lciamethods"
        && target_category == Some("flows")
        && (normalized_path.contains("characterisation")
            || normalized_path.contains("characterization"))
    {
        REFERENCE_ROLE_LCIA_FACTOR_FLOW
    } else if source_category == "lifecyclemodels" && target_category == Some("processes") {
        REFERENCE_ROLE_LIFECYCLE_MODEL_PROCESS
    } else {
        REFERENCE_ROLE_SUPPORT_DOCUMENT
    }
}

fn is_version(value: &str) -> bool {
    let parts: Vec<_> = value.split('.').collect();
    matches!(
        parts.as_slice(),
        [major, minor]
            if major.len() == 2
                && minor.len() == 2
                && major.bytes().all(|byte| byte.is_ascii_digit())
                && minor.bytes().all(|byte| byte.is_ascii_digit())
    ) || matches!(
        parts.as_slice(),
        [major, minor, patch]
            if major.len() == 2
                && minor.len() == 2
                && patch.len() == 3
                && major.bytes().all(|byte| byte.is_ascii_digit())
                && minor.bytes().all(|byte| byte.is_ascii_digit())
                && patch.bytes().all(|byte| byte.is_ascii_digit())
    )
}

fn is_canonical_uuid(value: &str) -> bool {
    value.len() == 36
        && value.bytes().enumerate().all(|(index, byte)| {
            if matches!(index, 8 | 13 | 18 | 23) {
                byte == b'-'
            } else {
                byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)
            }
        })
}

#[derive(Debug, Error, Eq, PartialEq)]
pub enum ReferenceExtractionError {
    #[error("document_key must be non-empty")]
    EmptyDocumentKey,
    #[error("category must be non-empty")]
    EmptyCategory,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frozen_python_golden_fixture_matches_all_rust_results() {
        let fixture: Value = serde_json::from_str(include_str!(
            "../../../tests/fixtures/reference_extraction_v1/golden.json"
        ))
        .unwrap();
        for case in fixture["cases"].as_array().unwrap() {
            let result = extract_references(
                case["document_key"].as_str().unwrap(),
                case["category"].as_str().unwrap(),
                &case["payload"],
            )
            .unwrap();
            if let Some(expected) = case.get("expected") {
                assert_eq!(
                    serde_json::to_value(result).unwrap(),
                    *expected,
                    "{}",
                    case["name"]
                );
            } else {
                let targets: Vec<_> = result
                    .edges
                    .iter()
                    .map(|edge| edge.target_uuid.as_str())
                    .collect();
                let expected: Vec<_> = case["expected_edge_targets"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|value| value.as_str().unwrap())
                    .collect();
                assert_eq!(targets, expected, "{}", case["name"]);
                assert!(result.issues.is_empty(), "{}", case["name"]);
            }
        }
    }

    #[test]
    fn role_vocabulary_and_input_failures_are_closed() {
        assert_eq!(
            REFERENCE_ROLES,
            [
                "lcia_factor_flow",
                "lifecycle_model_process",
                "process_exchange_flow",
                "support_document",
            ]
        );
        assert_eq!(
            extract_references("", "processes", &Value::Null),
            Err(ReferenceExtractionError::EmptyDocumentKey)
        );
        assert_eq!(
            extract_references("key", "", &Value::Null),
            Err(ReferenceExtractionError::EmptyCategory)
        );
    }

    #[test]
    fn emitted_contracts_validate_against_the_checked_in_schema() {
        let fixture: Value = serde_json::from_str(include_str!(
            "../../../tests/fixtures/reference_extraction_v1/golden.json"
        ))
        .unwrap();
        let schema: Value = serde_json::from_str(REFERENCE_EXTRACTION_JSON_SCHEMA_V1).unwrap();
        let validator = jsonschema::draft202012::new(&schema).unwrap();
        for case in fixture["cases"].as_array().unwrap() {
            let result = extract_references(
                case["document_key"].as_str().unwrap(),
                case["category"].as_str().unwrap(),
                &case["payload"],
            )
            .unwrap();
            let instance = serde_json::to_value(result).unwrap();
            let errors: Vec<_> = validator
                .iter_errors(&instance)
                .map(|error| error.to_string())
                .collect();
            assert!(errors.is_empty(), "{}: {errors:?}", case["name"]);
        }
    }
}
