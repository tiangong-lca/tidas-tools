use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::index::{DatasetEntry, DatasetIndex, contained, hex_digest};
use crate::{INLINE_ITEM_LIMIT, ReferenceClosureReportV1, ReleaseError, ReleaseRuntime};

pub const UNIT_PROFILE: &str = "unit-process-full-closure.v1";
pub const RESULT_PROFILE: &str = "standalone-lifecyclemodel-result-full-closure.v1";

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ReleaseProfile {
    UnitProcess,
    StandaloneResult,
}

impl ReleaseProfile {
    #[must_use]
    pub const fn id(self) -> &'static str {
        match self {
            Self::UnitProcess => UNIT_PROFILE,
            Self::StandaloneResult => RESULT_PROFILE,
        }
    }
}

pub(crate) fn resolve(
    input_dir: &Path,
    index: &DatasetIndex,
    profile: ReleaseProfile,
    runtime: &ReleaseRuntime,
) -> Result<(Vec<DatasetEntry>, ReferenceClosureReportV1), ReleaseError> {
    let root_keys: Vec<String> = index
        .entries()
        .iter()
        .filter(|entry| match profile {
            ReleaseProfile::UnitProcess => entry.role == "unit_process",
            ReleaseProfile::StandaloneResult => {
                matches!(entry.role.as_str(), "lifecycle_model" | "result_process")
            }
        })
        .map(DatasetEntry::key)
        .collect();
    if root_keys.is_empty() {
        return Err(ReleaseError::ProfileRootsMissing(profile.id().to_owned()));
    }
    let root_count = u64::try_from(root_keys.len()).map_err(|_| ReleaseError::SizeOverflow)?;
    let mut pending = root_keys;
    pending.reverse();
    let mut selected = BTreeMap::<String, DatasetEntry>::new();
    let mut reference_count = 0_u64;
    while let Some(key) = pending.pop() {
        runtime.cancellation.check()?;
        if selected.contains_key(&key) {
            continue;
        }
        let entry = index
            .get(&key)
            .ok_or_else(|| ReleaseError::ReferenceClosureMissing(key.clone()))?
            .clone();
        let path = contained(input_dir, &entry.relative_path)?;
        let metadata = fs::metadata(&path)?;
        let reserve = metadata
            .len()
            .checked_mul(4)
            .ok_or(ReleaseError::SizeOverflow)?;
        let _reservation = runtime.memory_budget.reserve(reserve)?;
        let bytes = fs::read(&path)?;
        let document: Value =
            serde_json::from_slice(&bytes).map_err(|source| ReleaseError::DatasetJson {
                path: path.clone(),
                source,
            })?;
        let mut references = BTreeSet::new();
        walk_references(&document, "$", None, &mut references)?;
        reference_count = reference_count
            .checked_add(u64::try_from(references.len()).map_err(|_| ReleaseError::SizeOverflow)?)
            .ok_or(ReleaseError::SizeOverflow)?;
        for (referenced, location) in references.into_iter().rev() {
            if index.get(&referenced).is_none() {
                return Err(ReleaseError::ReferenceClosureMissing(format!(
                    "{} {location} -> {referenced}",
                    entry.key()
                )));
            }
            if !selected.contains_key(&referenced) {
                pending.push(referenced);
            }
        }
        selected.insert(key, entry);
    }

    let entries: Vec<DatasetEntry> = selected.into_values().collect();
    let dataset_count = u64::try_from(entries.len()).map_err(|_| ReleaseError::SizeOverflow)?;
    let all_keys: Vec<String> = entries.iter().map(DatasetEntry::key).collect();
    let closure_sha256 = hash_strings(&all_keys)?;
    let truncated = all_keys.len() > INLINE_ITEM_LIMIT;
    let dataset_keys = all_keys.into_iter().take(INLINE_ITEM_LIMIT).collect();
    Ok((
        entries,
        ReferenceClosureReportV1 {
            profile_id: profile.id().to_owned(),
            root_count,
            dataset_count,
            reference_count,
            closure_sha256,
            dataset_keys,
            dataset_keys_truncated: truncated,
        },
    ))
}

pub(crate) fn verify_result_contains_unit(
    unit: &[DatasetEntry],
    result: &[DatasetEntry],
) -> Result<(), ReleaseError> {
    let result_keys: BTreeSet<String> = result.iter().map(DatasetEntry::key).collect();
    for key in unit.iter().map(DatasetEntry::key) {
        if !result_keys.contains(&key) {
            return Err(ReleaseError::StandaloneMissingUnitClosure(key));
        }
    }
    Ok(())
}

fn walk_references(
    value: &Value,
    location: &str,
    parent_key: Option<&str>,
    output: &mut BTreeSet<(String, String)>,
) -> Result<(), ReleaseError> {
    match value {
        Value::Object(object) => {
            if !parent_key.is_some_and(is_preceding_version_reference)
                && let (Some(reference_id), Some(reference_type)) = (
                    object.get("@refObjectId").and_then(Value::as_str),
                    object.get("@type").and_then(Value::as_str),
                )
                && let Some(dataset_type) = reference_dataset_type(reference_type)
            {
                let version = object
                    .get("@version")
                    .and_then(Value::as_str)
                    .filter(|value| !value.is_empty())
                    .ok_or_else(|| {
                        ReleaseError::ReferenceVersionMissing(format!(
                            "{location} -> {reference_type} {reference_id}"
                        ))
                    })?;
                output.insert((
                    format!(
                        "{}:{}:{}",
                        dataset_type,
                        reference_id.to_lowercase(),
                        version
                    ),
                    location.to_owned(),
                ));
            }
            for (key, child) in object {
                walk_references(child, &format!("{location}/{key}"), Some(key), output)?;
            }
        }
        Value::Array(items) => {
            for (index, child) in items.iter().enumerate() {
                walk_references(child, &format!("{location}/{index}"), parent_key, output)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn is_preceding_version_reference(key: &str) -> bool {
    key.rsplit(':')
        .next()
        .is_some_and(|name| name.eq_ignore_ascii_case("referenceToPrecedingDataSetVersion"))
}

fn reference_dataset_type(value: &str) -> Option<&'static str> {
    match value.to_ascii_lowercase().as_str() {
        "contact data set" => Some("contact"),
        "flow data set" => Some("flow"),
        "flow property data set" => Some("flowproperty"),
        "unit group data set" => Some("unitgroup"),
        "process data set" => Some("process"),
        "source data set" => Some("source"),
        "lcia method data set" => Some("lciamethod"),
        "life cycle model data set" => Some("lifecyclemodel"),
        _ => None,
    }
}

fn hash_strings(values: &[String]) -> Result<String, ReleaseError> {
    let bytes = serde_json::to_vec(values)?;
    Ok(hex_digest(Sha256::digest(bytes)))
}
