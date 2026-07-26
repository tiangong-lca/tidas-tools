use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tidas_runtime::CancellationToken;

use crate::ExportError;

const CATEGORIES: &[&str] = &[
    "contacts",
    "flowproperties",
    "flows",
    "lciamethods",
    "lifecyclemodels",
    "processes",
    "sources",
    "unitgroups",
];
const PRECEDING_KEYS: &[&str] = &[
    "common:referenceToPrecedingDataSetVersion",
    "referenceToPrecedingDataSetVersion",
];

static VERSIONED_FILE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(?P<uuid>[0-9a-fA-F-]{36})_(?P<version>\d{2}\.\d{2}\.\d{3})\.json$")
        .expect("the versioned file regex is valid")
});
static VERSIONED_URI: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?P<uuid>[0-9a-fA-F-]{36})_(?P<version>\d{2}\.\d{2}\.\d{3})(?P<suffix>\.(?:xml|json))$",
    )
    .expect("the versioned URI regex is valid")
});
static UNVERSIONED_URI: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?P<uuid>[0-9a-fA-F-]{36})(?P<suffix>\.(?:xml|json))$")
        .expect("the unversioned URI regex is valid")
});

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct VersionNormalizationV1 {
    pub scanned_records: u64,
    pub dataset_count: u64,
    pub duplicate_dataset_count: u64,
    pub kept_records: u64,
    pub removed_records: u64,
    pub rewritten_files: u64,
    pub updated_references: u64,
    pub removed_preceding_references: u64,
}

struct Record {
    category: String,
    uuid: String,
    version: String,
    path: PathBuf,
}

pub fn normalize_package_versions(
    package_dir: &Path,
    cancellation: &CancellationToken,
) -> Result<VersionNormalizationV1, ExportError> {
    cancellation.check()?;
    let records = load_records(package_dir, cancellation)?;
    let mut versions = BTreeMap::<(String, String), BTreeSet<String>>::new();
    for record in &records {
        versions
            .entry((record.category.clone(), record.uuid.clone()))
            .or_default()
            .insert(record.version.clone());
    }
    let latest = versions
        .iter()
        .filter_map(|(key, values)| values.last().map(|value| (key.clone(), value.clone())))
        .collect::<BTreeMap<_, _>>();
    let mut summary = VersionNormalizationV1 {
        scanned_records: u64::try_from(records.len())
            .map_err(|_| tidas_runtime::RuntimeError::SizeOverflow)?,
        dataset_count: u64::try_from(versions.len())
            .map_err(|_| tidas_runtime::RuntimeError::SizeOverflow)?,
        duplicate_dataset_count: u64::try_from(
            versions.values().filter(|values| values.len() > 1).count(),
        )
        .map_err(|_| tidas_runtime::RuntimeError::SizeOverflow)?,
        kept_records: 0,
        removed_records: 0,
        rewritten_files: 0,
        updated_references: 0,
        removed_preceding_references: 0,
    };

    for record in &records {
        cancellation.check()?;
        let key = (record.category.clone(), record.uuid.clone());
        if latest.get(&key) != Some(&record.version) {
            continue;
        }
        summary.kept_records += 1;
        let bytes = fs::read(&record.path)?;
        let mut payload: Value = serde_json::from_slice(&bytes)?;
        if normalize_value(&mut payload, &latest, &versions, &mut summary, cancellation)? {
            let mut output = serde_json::to_vec_pretty(&payload)?;
            output.push(b'\n');
            fs::write(&record.path, output)?;
            summary.rewritten_files += 1;
        }
    }
    for record in &records {
        cancellation.check()?;
        let key = (record.category.clone(), record.uuid.clone());
        if latest.get(&key) != Some(&record.version) {
            fs::remove_file(&record.path)?;
            summary.removed_records += 1;
        }
    }
    Ok(summary)
}

fn load_records(
    package_dir: &Path,
    cancellation: &CancellationToken,
) -> Result<Vec<Record>, ExportError> {
    let mut records = Vec::new();
    for category in CATEGORIES {
        let directory = package_dir.join(category);
        if !directory.is_dir() {
            continue;
        }
        let mut paths = fs::read_dir(directory)?
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .collect::<Vec<_>>();
        paths.sort();
        for path in paths {
            cancellation.check()?;
            let Some(name) = path.file_name().and_then(|value| value.to_str()) else {
                continue;
            };
            let Some(captures) = VERSIONED_FILE.captures(name) else {
                continue;
            };
            records.push(Record {
                category: (*category).to_owned(),
                uuid: captures["uuid"].to_owned(),
                version: captures["version"].to_owned(),
                path,
            });
        }
    }
    Ok(records)
}

fn normalize_value(
    node: &mut Value,
    latest: &BTreeMap<(String, String), String>,
    versions: &BTreeMap<(String, String), BTreeSet<String>>,
    summary: &mut VersionNormalizationV1,
    cancellation: &CancellationToken,
) -> Result<bool, ExportError> {
    cancellation.check()?;
    let mut changed = false;
    match node {
        Value::Object(object) => {
            let keys = object.keys().cloned().collect::<Vec<_>>();
            for key in keys {
                let mut remove = false;
                if let Some(Value::Object(reference)) = object.get_mut(&key)
                    && reference.contains_key("@refObjectId")
                    && let Some((category, uuid, version)) = infer_target(reference)
                {
                    let dataset = (category, uuid.clone());
                    if PRECEDING_KEYS.contains(&key.as_str())
                        && versions.get(&dataset).is_some_and(|items| items.len() > 1)
                    {
                        remove = true;
                    } else if let (Some(current), Some(reference_version)) =
                        (latest.get(&dataset), version)
                        && reference_version != *current
                    {
                        reference.insert("@version".to_owned(), Value::String(current.clone()));
                        if let Some(Value::String(uri)) = reference.get_mut("@uri") {
                            *uri = rewrite_uri(uri, &uuid, current);
                        }
                        summary.updated_references += 1;
                        changed = true;
                    }
                }
                if remove {
                    object.remove(&key);
                    summary.removed_preceding_references += 1;
                    changed = true;
                    continue;
                }
                if let Some(child) = object.get_mut(&key) {
                    changed |= normalize_value(child, latest, versions, summary, cancellation)?;
                }
            }
        }
        Value::Array(values) => {
            for value in values {
                changed |= normalize_value(value, latest, versions, summary, cancellation)?;
            }
        }
        _ => {}
    }
    Ok(changed)
}

fn infer_target(
    reference: &serde_json::Map<String, Value>,
) -> Option<(String, String, Option<String>)> {
    let uri = reference
        .get("@uri")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let category = CATEGORIES
        .iter()
        .find(|category| {
            uri.contains(&format!("/{category}/")) || uri.contains(&format!("../{category}/"))
        })
        .map(|value| (*value).to_owned())
        .or_else(|| {
            reference
                .get("@type")
                .and_then(Value::as_str)
                .and_then(category_from_type)
                .map(str::to_owned)
        })?;
    let mut uuid = reference
        .get("@refObjectId")
        .and_then(Value::as_str)
        .map(str::to_owned);
    let mut version = reference
        .get("@version")
        .and_then(Value::as_str)
        .map(str::to_owned);
    if let Some(captures) = VERSIONED_URI.captures(uri) {
        uuid.get_or_insert_with(|| captures["uuid"].to_owned());
        version.get_or_insert_with(|| captures["version"].to_owned());
    } else if let Some(captures) = UNVERSIONED_URI.captures(uri) {
        uuid.get_or_insert_with(|| captures["uuid"].to_owned());
    }
    Some((category, uuid?, version))
}

fn category_from_type(value: &str) -> Option<&'static str> {
    match value.trim().to_ascii_lowercase().as_str() {
        "contact data set" => Some("contacts"),
        "flow property data set" => Some("flowproperties"),
        "flow data set" => Some("flows"),
        "lcia method data set" => Some("lciamethods"),
        "life cycle model data set" | "lifecycle model data set" => Some("lifecyclemodels"),
        "process data set" => Some("processes"),
        "source data set"
        | "compliance system"
        | "ilcddataformatreference"
        | "datasetformat"
        | "ilcdformatglobalreference" => Some("sources"),
        "unit group data set" => Some("unitgroups"),
        _ => None,
    }
}

fn rewrite_uri(uri: &str, uuid: &str, latest: &str) -> String {
    let Some(captures) = VERSIONED_URI.captures(uri) else {
        return uri.to_owned();
    };
    if captures.name("uuid").map(|value| value.as_str()) != Some(uuid) {
        return uri.to_owned();
    }
    let matched = captures.get(0).expect("a successful match has a range");
    format!(
        "{}{}_{}{}",
        &uri[..matched.start()],
        uuid,
        latest,
        &captures["suffix"]
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_json(path: &Path, payload: &Value) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let mut bytes = serde_json::to_vec_pretty(payload).unwrap();
        bytes.push(b'\n');
        fs::write(path, bytes).unwrap();
    }

    #[test]
    fn matches_python_golden_version_selection_and_reference_rewrite() {
        let temporary = tempfile::tempdir().unwrap();
        let package = temporary.path();
        let contact = "11111111-1111-1111-1111-111111111111";
        let flow = "22222222-2222-2222-2222-222222222222";
        let process = "33333333-3333-3333-3333-333333333333";
        write_json(
            &package.join(format!("contacts/{contact}_01.00.005.json")),
            &serde_json::json!({"contactDataSet":{"common:UUID":contact}}),
        );
        write_json(
            &package.join(format!("contacts/{contact}_01.00.006.json")),
            &serde_json::json!({"contactDataSet":{"common:UUID":contact}}),
        );
        write_json(
            &package.join(format!("flows/{flow}_01.01.000.json")),
            &serde_json::json!({"flowDataSet":{"common:UUID":flow}}),
        );
        write_json(
            &package.join(format!("flows/{flow}_01.01.001.json")),
            &serde_json::json!({"flowDataSet":{"administrativeInformation":{"publicationAndOwnership":{
                "common:referenceToOwnershipOfDataSet":{
                    "@refObjectId":contact,"@type":"contact data set","@version":"01.00.005",
                    "@uri":format!("../contacts/{contact}_01.00.005.xml")
                },
                "common:referenceToPrecedingDataSetVersion":{
                    "@refObjectId":flow,"@type":"flow data set","@version":"01.01.000",
                    "@uri":format!("../flows/{flow}_01.01.000.xml")
                }
            }}}}),
        );
        write_json(
            &package.join(format!("processes/{process}_01.01.000.json")),
            &serde_json::json!({"processDataSet":{"exchanges":{"exchange":[{
                "referenceToFlowDataSet":{
                    "@refObjectId":flow,"@type":"flow data set","@version":"01.01.000",
                    "@uri":format!("../flows/{flow}_01.01.000.xml")
                }
            }]}}}),
        );

        let summary = normalize_package_versions(package, &CancellationToken::default()).unwrap();
        assert_eq!(summary.duplicate_dataset_count, 2);
        assert_eq!(summary.removed_records, 2);
        assert_eq!(summary.updated_references, 2);
        assert_eq!(summary.removed_preceding_references, 1);
        assert!(
            !package
                .join(format!("contacts/{contact}_01.00.005.json"))
                .exists()
        );
        let flow_payload: Value = serde_json::from_slice(
            &fs::read(package.join(format!("flows/{flow}_01.01.001.json"))).unwrap(),
        )
        .unwrap();
        let publication =
            &flow_payload["flowDataSet"]["administrativeInformation"]["publicationAndOwnership"];
        assert_eq!(
            publication["common:referenceToOwnershipOfDataSet"]["@version"],
            "01.00.006"
        );
        assert!(
            publication
                .get("common:referenceToPrecedingDataSetVersion")
                .is_none()
        );
    }
}
