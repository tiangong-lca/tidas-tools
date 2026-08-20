use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value;
use sha2::{Digest, Sha256};
use tidas_conversion::{ConversionDirection, ConversionRequest, convert_directory};
use walkdir::WalkDir;

use crate::index::{hex_digest, safe_relative};
use crate::{
    INLINE_ITEM_LIMIT, IlcdConversionReportV1, ReleaseError, ReleaseRuntime, RoundtripMismatchV1,
    SemanticRoundtripReportV1,
};

const FILE_MEMORY_MULTIPLIER: u64 = 8;

pub(crate) fn convert_tidas_to_ilcd(
    input_dir: &Path,
    output_dir: &Path,
    runtime: &ReleaseRuntime,
) -> Result<IlcdConversionReportV1, ReleaseError> {
    runtime.cancellation.check()?;
    if !input_dir.is_dir() {
        return Err(ReleaseError::InputNotDirectory(input_dir.to_path_buf()));
    }
    let conversion = convert_directory(&ConversionRequest {
        input_dir: input_dir.to_path_buf(),
        output_dir: output_dir.to_path_buf(),
        direction: ConversionDirection::TidasToIlcd,
        cancellation: runtime.cancellation.clone(),
        memory_budget: runtime.memory_budget.clone(),
        queue_capacity: runtime.queue_capacity,
        progress: None,
    })?;
    let files = dataset_files(input_dir, runtime)?;
    let mut input_bytes = 0_u64;
    let mut conversion_hasher = Sha256::new();
    for source in &files.paths {
        runtime.cancellation.check()?;
        let relative = source
            .strip_prefix(input_dir)
            .map_err(|_| ReleaseError::PathOutsideRoot(source.clone()))?;
        let relative_string = path_to_portable(relative)?;
        let metadata = fs::metadata(source)?;
        let bytes = fs::read(source)?;
        let target = output_dir.join("data").join(relative).with_extension("xml");
        let xml = fs::read(&target)?;
        let source_hash = hex_digest(Sha256::digest(&bytes));
        let output_hash = hex_digest(Sha256::digest(&xml));
        update_record_hash(
            &mut conversion_hasher,
            &[&relative_string, &source_hash, &output_hash],
        );
        input_bytes = input_bytes
            .checked_add(metadata.len())
            .ok_or(ReleaseError::SizeOverflow)?;
    }
    Ok(IlcdConversionReportV1 {
        dataset_count: u64::try_from(files.paths.len()).map_err(|_| ReleaseError::SizeOverflow)?,
        input_bytes,
        output_bytes: conversion.output_bytes,
        conversion_set_sha256: hex_digest(conversion_hasher.finalize()),
        output_tree_sha256: conversion.output_tree_sha256,
        asset_fingerprint: conversion.asset_fingerprint,
    })
}

pub(crate) fn semantic_roundtrip(
    tidas_dir: &Path,
    ilcd_dir: &Path,
    runtime: &ReleaseRuntime,
) -> Result<SemanticRoundtripReportV1, ReleaseError> {
    if !tidas_dir.is_dir() {
        return Err(ReleaseError::InputNotDirectory(tidas_dir.to_path_buf()));
    }
    if !ilcd_dir.is_dir() {
        return Err(ReleaseError::InputNotDirectory(ilcd_dir.to_path_buf()));
    }
    let ilcd_data_dir = ilcd_dir.join("data");
    if !ilcd_data_dir.is_dir() {
        return Err(ReleaseError::InputNotDirectory(ilcd_data_dir));
    }
    let restored_workspace = tempfile::tempdir()?;
    let restored_dir = restored_workspace.path().join("restored");
    convert_directory(&ConversionRequest {
        input_dir: ilcd_data_dir,
        output_dir: restored_dir.clone(),
        direction: ConversionDirection::IlcdToTidas,
        cancellation: runtime.cancellation.clone(),
        memory_budget: runtime.memory_budget.clone(),
        queue_capacity: runtime.queue_capacity,
        progress: None,
    })?;
    let mut dataset_count = 0_u64;
    let mut mismatch_count = 0_u64;
    let mut mismatches = Vec::new();
    let mut semantic_hasher = Sha256::new();
    let files = dataset_files(tidas_dir, runtime)?;
    for source in &files.paths {
        runtime.cancellation.check()?;
        let relative = source
            .strip_prefix(tidas_dir)
            .map_err(|_| ReleaseError::PathOutsideRoot(source.clone()))?;
        let relative_string = path_to_portable(relative)?;
        let restored_path = restored_dir.join("data").join(relative);
        if !restored_path.is_file() {
            update_record_hash(&mut semantic_hasher, &[&relative_string, "xml-missing"]);
            record_mismatch(
                &mut mismatches,
                &mut mismatch_count,
                RoundtripMismatchV1 {
                    path: relative_string,
                    location: "$".to_owned(),
                    code: "xml-missing".to_owned(),
                },
            )?;
            continue;
        }
        let source_metadata = fs::metadata(source)?;
        let restored_metadata = fs::metadata(&restored_path)?;
        let reserved = source_metadata
            .len()
            .checked_add(restored_metadata.len())
            .and_then(|bytes| bytes.checked_mul(FILE_MEMORY_MULTIPLIER))
            .ok_or(ReleaseError::SizeOverflow)?;
        let _reservation = runtime.memory_budget.reserve(reserved)?;
        let source_value: Value =
            serde_json::from_slice(&fs::read(source)?).map_err(|source_error| {
                ReleaseError::DatasetJson {
                    path: source.clone(),
                    source: source_error,
                }
            })?;
        let converted_value: Value = serde_json::from_slice(&fs::read(&restored_path)?)?;
        let normalized_source = normalize(source_value, None);
        let normalized_converted = normalize(converted_value, None);
        let source_hash = canonical_hash(&normalized_source)?;
        let converted_hash = canonical_hash(&normalized_converted)?;
        update_record_hash(
            &mut semantic_hasher,
            &[&relative_string, &source_hash, &converted_hash],
        );
        dataset_count = dataset_count
            .checked_add(1)
            .ok_or(ReleaseError::SizeOverflow)?;
        if let Some(location) = first_difference(&normalized_source, &normalized_converted, "$") {
            record_mismatch(
                &mut mismatches,
                &mut mismatch_count,
                RoundtripMismatchV1 {
                    path: relative_string,
                    location,
                    code: "semantic-roundtrip-mismatch".to_owned(),
                },
            )?;
        }
    }
    Ok(SemanticRoundtripReportV1 {
        ok: mismatch_count == 0,
        dataset_count,
        mismatch_count,
        semantic_set_sha256: hex_digest(semantic_hasher.finalize()),
        mismatches_truncated: usize::try_from(mismatch_count)
            .map_or(true, |count| count > mismatches.len()),
        mismatches,
    })
}

struct PathCatalog {
    paths: Vec<PathBuf>,
    _reservation: tidas_runtime::MemoryReservation,
}

fn dataset_files(root: &Path, runtime: &ReleaseRuntime) -> Result<PathCatalog, ReleaseError> {
    let mut files = Vec::new();
    for entry in WalkDir::new(root)
        .min_depth(1)
        .max_depth(2)
        .follow_links(false)
    {
        runtime.cancellation.check()?;
        let entry = entry?;
        if entry.file_type().is_symlink() {
            return Err(ReleaseError::Symlink(entry.into_path()));
        }
        if entry.file_type().is_file()
            && entry.path().extension().and_then(|value| value.to_str()) == Some("json")
        {
            files.push(entry.into_path());
        }
    }
    files.sort();
    let estimate = files.iter().try_fold(0_u64, |total, path| {
        let bytes =
            u64::try_from(path.as_os_str().len()).map_err(|_| ReleaseError::SizeOverflow)?;
        total
            .checked_add(bytes.checked_add(128).ok_or(ReleaseError::SizeOverflow)?)
            .ok_or(ReleaseError::SizeOverflow)
    })?;
    let reservation = runtime.memory_budget.reserve(estimate)?;
    Ok(PathCatalog {
        paths: files,
        _reservation: reservation,
    })
}

fn normalize(value: Value, field_name: Option<&str>) -> Value {
    match value {
        Value::Object(object) => {
            if object.is_empty() {
                return Value::Null;
            }
            let mut entries: Vec<(String, Value)> = object.into_iter().collect();
            entries.sort_by(|left, right| left.0.cmp(&right.0));
            let normalized = entries
                .into_iter()
                .map(|(key, child)| {
                    let value = normalize(child, Some(&key));
                    (key, value)
                })
                .collect();
            Value::Object(normalized)
        }
        Value::Array(items) => {
            let mut normalized: Vec<Value> = items
                .into_iter()
                .map(|item| normalize(item, field_name))
                .collect();
            if normalized.len() == 1 {
                normalized.remove(0)
            } else {
                Value::Array(normalized)
            }
        }
        Value::Bool(value) => Value::String(if value { "true" } else { "false" }.to_owned()),
        Value::Number(value) => Value::String(value.to_string()),
        Value::String(value) if field_name == Some("@uri") => {
            value.strip_suffix(".xml").map_or_else(
                || Value::String(value.clone()),
                |prefix| Value::String(format!("{prefix}.json")),
            )
        }
        other => other,
    }
}

fn first_difference(left: &Value, right: &Value, location: &str) -> Option<String> {
    match (left, right) {
        (Value::Object(left), Value::Object(right)) => {
            let left_keys: Vec<&String> = left.keys().collect();
            let right_keys: Vec<&String> = right.keys().collect();
            if left_keys != right_keys {
                return Some(location.to_owned());
            }
            left.iter().find_map(|(key, value)| {
                first_difference(value, &right[key], &format!("{location}/{key}"))
            })
        }
        (Value::Array(left), Value::Array(right)) => {
            if left.len() != right.len() {
                return Some(location.to_owned());
            }
            left.iter()
                .zip(right)
                .enumerate()
                .find_map(|(index, (left, right))| {
                    first_difference(left, right, &format!("{location}/{index}"))
                })
        }
        _ if std::mem::discriminant(left) != std::mem::discriminant(right) || left != right => {
            Some(location.to_owned())
        }
        _ => None,
    }
}

fn record_mismatch(
    mismatches: &mut Vec<RoundtripMismatchV1>,
    mismatch_count: &mut u64,
    mismatch: RoundtripMismatchV1,
) -> Result<(), ReleaseError> {
    *mismatch_count = mismatch_count
        .checked_add(1)
        .ok_or(ReleaseError::SizeOverflow)?;
    if mismatches.len() < INLINE_ITEM_LIMIT {
        mismatches.push(mismatch);
    }
    Ok(())
}

fn canonical_hash(value: &Value) -> Result<String, ReleaseError> {
    Ok(hex_digest(Sha256::digest(serde_json::to_vec(value)?)))
}

fn update_record_hash(hasher: &mut Sha256, values: &[&str]) {
    for value in values {
        hasher.update(value.len().to_le_bytes());
        hasher.update(value.as_bytes());
    }
}

fn path_to_portable(path: &Path) -> Result<String, ReleaseError> {
    let value = path
        .components()
        .map(|component| {
            component
                .as_os_str()
                .to_str()
                .map(ToOwned::to_owned)
                .ok_or_else(|| ReleaseError::UnsafePath(path.to_path_buf()))
        })
        .collect::<Result<Vec<_>, _>>()?
        .join("/");
    safe_relative(&value)
}
