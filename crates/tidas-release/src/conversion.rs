use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value;
use sha2::{Digest, Sha256};
use tidas_assets::{asset_fingerprint, bundled_assets};
use tidas_conversion::{TidasSchemaOrderer, convert_json_to_xml, convert_xml_to_json};
use walkdir::WalkDir;

use crate::index::{hex_digest, safe_relative, sha256_file};
use crate::transaction::StagedDirectory;
use crate::{
    INLINE_ITEM_LIMIT, IlcdConversionReportV1, ReleaseError, ReleaseRuntime, RoundtripMismatchV1,
    SemanticRoundtripReportV1,
};

const FILE_MEMORY_MULTIPLIER: u64 = 8;
const EILCD_ASSET_PREFIX: &str = "assets/eilcd/";

pub(crate) fn convert_tidas_to_ilcd(
    input_dir: &Path,
    output_dir: &Path,
    runtime: &ReleaseRuntime,
) -> Result<IlcdConversionReportV1, ReleaseError> {
    runtime.cancellation.check()?;
    if !input_dir.is_dir() {
        return Err(ReleaseError::InputNotDirectory(input_dir.to_path_buf()));
    }
    reject_nested_output(input_dir, output_dir)?;
    let orderer = TidasSchemaOrderer::from_bundled_assets()?;
    let staging = StagedDirectory::new(output_dir)?;
    let mut dataset_count = 0_u64;
    let mut input_bytes = 0_u64;
    let mut conversion_hasher = Sha256::new();

    let files = dataset_files(input_dir, runtime)?;
    for source in &files.paths {
        runtime.cancellation.check()?;
        let relative = source
            .strip_prefix(input_dir)
            .map_err(|_| ReleaseError::PathOutsideRoot(source.clone()))?;
        let relative_string = path_to_portable(relative)?;
        let category = relative
            .parent()
            .and_then(Path::file_name)
            .and_then(|value| value.to_str())
            .ok_or_else(|| ReleaseError::UnsafePath(relative.to_path_buf()))?;
        let metadata = fs::metadata(source)?;
        let reserved = metadata
            .len()
            .checked_mul(FILE_MEMORY_MULTIPLIER)
            .ok_or(ReleaseError::SizeOverflow)?;
        let _reservation = runtime.memory_budget.reserve(reserved)?;
        let bytes = fs::read(source)?;
        let document: Value =
            serde_json::from_slice(&bytes).map_err(|source_error| ReleaseError::DatasetJson {
                path: source.clone(),
                source: source_error,
            })?;
        let mut ordered_document = orderer.order_document(&document, category)?;
        convert_reference_uris(&mut ordered_document);
        let ordered_bytes = serde_json::to_vec(&ordered_document)?;
        let xml = convert_json_to_xml(&ordered_bytes, &runtime.cancellation)?;
        let output_relative = Path::new("data").join(relative).with_extension("xml");
        let target = staging.path().join(&output_relative);
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&target, &xml)?;
        let source_hash = hex_digest(Sha256::digest(&bytes));
        let output_hash = hex_digest(Sha256::digest(&xml));
        update_record_hash(
            &mut conversion_hasher,
            &[&relative_string, &source_hash, &output_hash],
        );
        dataset_count = dataset_count
            .checked_add(1)
            .ok_or(ReleaseError::SizeOverflow)?;
        input_bytes = input_bytes
            .checked_add(metadata.len())
            .ok_or(ReleaseError::SizeOverflow)?;
    }
    copy_ilcd_assets(staging.path(), runtime)?;
    let (output_bytes, output_tree_sha256) = hash_tree(staging.path(), runtime)?;
    runtime.cancellation.check()?;
    staging.commit()?;
    Ok(IlcdConversionReportV1 {
        dataset_count,
        input_bytes,
        output_bytes,
        conversion_set_sha256: hex_digest(conversion_hasher.finalize()),
        output_tree_sha256,
        asset_fingerprint: asset_fingerprint()?,
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
        let xml_path = ilcd_dir.join("data").join(relative).with_extension("xml");
        if !xml_path.is_file() {
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
        let xml_metadata = fs::metadata(&xml_path)?;
        let reserved = source_metadata
            .len()
            .checked_add(xml_metadata.len())
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
        let converted_bytes = convert_xml_to_json(&fs::read(&xml_path)?, &runtime.cancellation)?;
        let converted_value: Value = serde_json::from_slice(&converted_bytes)?;
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

fn convert_reference_uris(value: &mut Value) {
    match value {
        Value::Object(object) => {
            for (key, child) in object {
                if key == "@uri" {
                    if let Some(uri) = child.as_str()
                        && let Some(prefix) = uri.strip_suffix(".json")
                    {
                        *child = Value::String(format!("{prefix}.xml"));
                    }
                } else {
                    convert_reference_uris(child);
                }
            }
        }
        Value::Array(items) => {
            for item in items {
                convert_reference_uris(item);
            }
        }
        _ => {}
    }
}

fn copy_ilcd_assets(target: &Path, runtime: &ReleaseRuntime) -> Result<(), ReleaseError> {
    for asset in bundled_assets()
        .into_iter()
        .filter(|asset| asset.path.starts_with(EILCD_ASSET_PREFIX))
    {
        runtime.cancellation.check()?;
        let relative = asset
            .path
            .strip_prefix(EILCD_ASSET_PREFIX)
            .expect("filtered asset has the eILCD prefix");
        safe_relative(relative)?;
        let destination = target.join(relative);
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(destination, asset.bytes)?;
    }
    Ok(())
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

fn hash_tree(root: &Path, runtime: &ReleaseRuntime) -> Result<(u64, String), ReleaseError> {
    let mut file_count = 0_u64;
    let mut path_bytes = 0_u64;
    for entry in WalkDir::new(root).follow_links(false) {
        runtime.cancellation.check()?;
        let entry = entry?;
        if entry.file_type().is_symlink() {
            return Err(ReleaseError::Symlink(entry.into_path()));
        }
        if entry.file_type().is_file() {
            file_count = file_count
                .checked_add(1)
                .ok_or(ReleaseError::SizeOverflow)?;
            path_bytes = path_bytes
                .checked_add(
                    u64::try_from(entry.path().as_os_str().len())
                        .map_err(|_| ReleaseError::SizeOverflow)?
                        .checked_add(128)
                        .ok_or(ReleaseError::SizeOverflow)?,
                )
                .ok_or(ReleaseError::SizeOverflow)?;
        }
    }
    let _path_reservation = runtime.memory_budget.reserve(path_bytes)?;
    let capacity = usize::try_from(file_count).map_err(|_| ReleaseError::SizeOverflow)?;
    let mut files = Vec::with_capacity(capacity);
    for entry in WalkDir::new(root).follow_links(false) {
        runtime.cancellation.check()?;
        let entry = entry?;
        if entry.file_type().is_file() {
            files.push(entry.into_path());
        }
    }
    files.sort();
    let mut bytes = 0_u64;
    let mut hasher = Sha256::new();
    for file in files {
        let relative = file
            .strip_prefix(root)
            .map_err(|_| ReleaseError::PathOutsideRoot(file.clone()))?;
        let name = path_to_portable(relative)?;
        let metadata = fs::metadata(&file)?;
        bytes = bytes
            .checked_add(metadata.len())
            .ok_or(ReleaseError::SizeOverflow)?;
        let hash = sha256_file(&file, runtime)?;
        update_record_hash(&mut hasher, &[&name, &hash]);
    }
    Ok((bytes, hex_digest(hasher.finalize())))
}

fn reject_nested_output(input: &Path, output: &Path) -> Result<(), ReleaseError> {
    let input = fs::canonicalize(input)?;
    let candidate = if output.exists() {
        fs::canonicalize(output)?
    } else {
        let parent = output.parent().unwrap_or_else(|| Path::new("."));
        fs::canonicalize(parent)?.join(
            output
                .file_name()
                .ok_or_else(|| ReleaseError::UnsafePath(output.to_path_buf()))?,
        )
    };
    if candidate.starts_with(&input) {
        Err(ReleaseError::OutputInsideInput(output.to_path_buf()))
    } else {
        Ok(())
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
