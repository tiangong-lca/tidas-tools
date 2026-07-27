use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{AssetError, sha256_hex};

pub const SCHEMA_LOCK_PATH: &str = "assets/tidas/schema.lock.json";
const SCHEMA_ROOT: &str = "assets/tidas";
const LOCALIZED_KEY: &str = "description";

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SchemaLockV1 {
    allowed_localized_keys: Vec<String>,
    generation: SchemaLockGeneration,
    schema_root: String,
    schema_sets: BTreeMap<String, SchemaSetLock>,
    translation_pairs: TranslationPairLock,
    version: u8,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
struct SchemaLockGeneration {
    mode: String,
    tool: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
struct SchemaSetLock {
    content_aggregate_sha256: String,
    contract_aggregate_sha256: String,
    file_count: usize,
    files: BTreeMap<String, SchemaFileLock>,
    path: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
struct SchemaFileLock {
    content_sha256: String,
    contract_sha256: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
struct TranslationPairLock {
    contract_aggregate_sha256: String,
    file_count: usize,
    files: BTreeMap<String, TranslationPairFileLock>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
#[allow(clippy::struct_field_names)] // Wire names preserve the historical lock schema.
struct TranslationPairFileLock {
    contract_sha256: String,
    en_content_sha256: String,
    zh_content_sha256: String,
}

struct CollectedSchemaSet {
    documents: BTreeMap<String, Value>,
    content_hashes: BTreeMap<String, String>,
}

pub fn schema_lock_from_filesystem(repo_root: &Path) -> Result<SchemaLockV1, AssetError> {
    let schema_root = repo_root.join(SCHEMA_ROOT);
    let en = collect_schema_set(&schema_root.join("schemas"))?;
    let zh = collect_schema_set(&schema_root.join("schemas_zh"))?;

    let en_names: BTreeSet<&str> = en.documents.keys().map(String::as_str).collect();
    let zh_names: BTreeSet<&str> = zh.documents.keys().map(String::as_str).collect();
    if en_names != zh_names {
        return Err(AssetError::SchemaLock(format!(
            "English and Chinese schema file sets differ; missing_zh={:?}, missing_en={:?}",
            en_names.difference(&zh_names).collect::<Vec<_>>(),
            zh_names.difference(&en_names).collect::<Vec<_>>()
        )));
    }

    validate_local_refs(&en.documents, "en")?;
    validate_local_refs(&zh.documents, "zh")?;

    let mut en_files = BTreeMap::new();
    let mut zh_files = BTreeMap::new();
    let mut pair_files = BTreeMap::new();
    for name in en.documents.keys() {
        let en_contract = contract_node(
            en.documents
                .get(name)
                .expect("schema name came from the same map"),
        );
        let zh_contract = contract_node(
            zh.documents
                .get(name)
                .expect("schema file sets were verified equal"),
        );
        if en_contract != zh_contract {
            return Err(AssetError::SchemaLock(format!(
                "{name}: English and Chinese contracts differ after removing `{LOCALIZED_KEY}`"
            )));
        }
        let contract_sha256 = sha256_json(&en_contract)?;
        let en_content_sha256 = en
            .content_hashes
            .get(name)
            .expect("every parsed document has a content hash")
            .clone();
        let zh_content_sha256 = zh
            .content_hashes
            .get(name)
            .expect("every parsed document has a content hash")
            .clone();
        en_files.insert(
            name.clone(),
            SchemaFileLock {
                content_sha256: en_content_sha256.clone(),
                contract_sha256: contract_sha256.clone(),
            },
        );
        zh_files.insert(
            name.clone(),
            SchemaFileLock {
                content_sha256: zh_content_sha256.clone(),
                contract_sha256: contract_sha256.clone(),
            },
        );
        pair_files.insert(
            name.clone(),
            TranslationPairFileLock {
                contract_sha256,
                en_content_sha256,
                zh_content_sha256,
            },
        );
    }

    let mut schema_sets = BTreeMap::new();
    schema_sets.insert("en".to_owned(), schema_set_lock("schemas", en_files)?);
    schema_sets.insert("zh".to_owned(), schema_set_lock("schemas_zh", zh_files)?);
    let pair_contracts: BTreeMap<&str, &str> = pair_files
        .iter()
        .map(|(name, lock)| (name.as_str(), lock.contract_sha256.as_str()))
        .collect();

    Ok(SchemaLockV1 {
        allowed_localized_keys: vec![LOCALIZED_KEY.to_owned()],
        generation: SchemaLockGeneration {
            mode: "deterministic-v1".to_owned(),
            tool: "tidas-asset-lock".to_owned(),
        },
        schema_root: SCHEMA_ROOT.to_owned(),
        schema_sets,
        translation_pairs: TranslationPairLock {
            contract_aggregate_sha256: sha256_json(&pair_contracts)?,
            file_count: pair_files.len(),
            files: pair_files,
        },
        version: 1,
    })
}

pub fn write_schema_lock(repo_root: &Path) -> Result<PathBuf, AssetError> {
    let lock = schema_lock_from_filesystem(repo_root)?;
    let output_path = repo_root.join(SCHEMA_LOCK_PATH);
    let mut bytes = serde_json::to_vec_pretty(&lock)?;
    bytes.push(b'\n');
    fs::write(&output_path, bytes)?;
    Ok(output_path)
}

pub fn check_filesystem_schema_lock(repo_root: &Path) -> Result<(), AssetError> {
    let expected = schema_lock_from_filesystem(repo_root)?;
    let actual: SchemaLockV1 =
        serde_json::from_slice(&fs::read(repo_root.join(SCHEMA_LOCK_PATH))?)?;
    if actual == expected {
        Ok(())
    } else {
        Err(AssetError::SchemaLock(format!(
            "{SCHEMA_LOCK_PATH} is stale; run `cargo run -p tidas-assets --bin tidas-asset-lock -- write`"
        )))
    }
}

fn collect_schema_set(schema_dir: &Path) -> Result<CollectedSchemaSet, AssetError> {
    if !schema_dir.is_dir() {
        return Err(AssetError::SchemaLock(format!(
            "missing schema directory {}",
            schema_dir.display()
        )));
    }
    let mut paths = fs::read_dir(schema_dir)?
        .map(|entry| entry.map(|item| item.path()))
        .collect::<Result<Vec<_>, _>>()?;
    paths.retain(|path| path.extension().and_then(|value| value.to_str()) == Some("json"));
    paths.sort();

    let mut documents = BTreeMap::new();
    let mut content_hashes = BTreeMap::new();
    for path in paths {
        let name = path
            .file_name()
            .and_then(|value| value.to_str())
            .ok_or_else(|| {
                AssetError::SchemaLock(format!("non-UTF-8 schema path {}", path.display()))
            })?
            .to_owned();
        let bytes = fs::read(&path)?;
        let document: Value = serde_json::from_slice(&bytes)?;
        jsonschema::draft7::meta::validate(&document).map_err(|error| {
            AssetError::SchemaLock(format!(
                "{} is not a valid Draft 7 schema: {error}",
                path.display()
            ))
        })?;
        content_hashes.insert(name.clone(), sha256_hex(&normalize_line_endings(&bytes)));
        documents.insert(name, document);
    }
    Ok(CollectedSchemaSet {
        documents,
        content_hashes,
    })
}

fn validate_local_refs(
    documents: &BTreeMap<String, Value>,
    language: &str,
) -> Result<(), AssetError> {
    for (current_name, document) in documents {
        visit_refs(document, &mut |reference| {
            if reference.starts_with("http://") || reference.starts_with("https://") {
                return Ok(());
            }
            let (file_part, pointer) = reference
                .split_once('#')
                .map_or((reference, ""), |(file, pointer)| (file, pointer));
            let target = if file_part.is_empty() {
                Path::new(current_name)
            } else {
                Path::new(file_part)
            };
            if target.is_absolute()
                || target
                    .components()
                    .any(|component| component == Component::ParentDir)
            {
                return Err(AssetError::SchemaLock(format!(
                    "{language}:{current_name}: unsupported local reference `{reference}`"
                )));
            }
            let target_name = target
                .file_name()
                .and_then(|value| value.to_str())
                .ok_or_else(|| {
                    AssetError::SchemaLock(format!(
                        "{language}:{current_name}: invalid local reference `{reference}`"
                    ))
                })?;
            let target_document = documents.get(target_name).ok_or_else(|| {
                AssetError::SchemaLock(format!(
                    "{language}:{current_name}: missing local reference target `{target_name}`"
                ))
            })?;
            if !pointer_exists(target_document, pointer)? {
                return Err(AssetError::SchemaLock(format!(
                    "{language}:{current_name}: missing JSON pointer `#{pointer}` in `{target_name}`"
                )));
            }
            Ok(())
        })?;
    }
    Ok(())
}

fn visit_refs(
    value: &Value,
    visitor: &mut impl FnMut(&str) -> Result<(), AssetError>,
) -> Result<(), AssetError> {
    match value {
        Value::Object(object) => {
            for (key, child) in object {
                if key == "$ref" {
                    if let Some(reference) = child.as_str() {
                        visitor(reference)?;
                    }
                } else {
                    visit_refs(child, visitor)?;
                }
            }
        }
        Value::Array(items) => {
            for item in items {
                visit_refs(item, visitor)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn pointer_exists(document: &Value, pointer: &str) -> Result<bool, AssetError> {
    if pointer.is_empty() {
        return Ok(true);
    }
    if !pointer.starts_with('/') {
        return Err(AssetError::SchemaLock(format!(
            "JSON pointer must start with `/`: {pointer}"
        )));
    }
    let mut current = document;
    for raw_token in pointer[1..].split('/') {
        let token = decode_percent(raw_token)?
            .replace("~1", "/")
            .replace("~0", "~");
        match current {
            Value::Object(object) => {
                let Some(next) = object.get(&token) else {
                    return Ok(false);
                };
                current = next;
            }
            Value::Array(items) => {
                let Ok(index) = token.parse::<usize>() else {
                    return Ok(false);
                };
                let Some(next) = items.get(index) else {
                    return Ok(false);
                };
                current = next;
            }
            _ => return Ok(false),
        }
    }
    Ok(true)
}

fn decode_percent(value: &str) -> Result<String, AssetError> {
    let input = value.as_bytes();
    let mut decoded = Vec::with_capacity(input.len());
    let mut index = 0;
    while index < input.len() {
        if input[index] == b'%' {
            let Some(high) = input.get(index + 1).and_then(|byte| hex_digit(*byte)) else {
                return Err(AssetError::SchemaLock(format!(
                    "invalid percent encoding in JSON pointer token `{value}`"
                )));
            };
            let Some(low) = input.get(index + 2).and_then(|byte| hex_digit(*byte)) else {
                return Err(AssetError::SchemaLock(format!(
                    "invalid percent encoding in JSON pointer token `{value}`"
                )));
            };
            decoded.push(high * 16 + low);
            index += 3;
        } else {
            decoded.push(input[index]);
            index += 1;
        }
    }
    String::from_utf8(decoded).map_err(|_| {
        AssetError::SchemaLock(format!(
            "percent-decoded JSON pointer token is not UTF-8: `{value}`"
        ))
    })
}

const fn hex_digit(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        b'A'..=b'F' => Some(value - b'A' + 10),
        _ => None,
    }
}

fn contract_node(value: &Value) -> Value {
    match value {
        Value::Object(object) => Value::Object(
            object
                .iter()
                .filter(|(key, _)| key.as_str() != LOCALIZED_KEY)
                .map(|(key, child)| (key.clone(), contract_node(child)))
                .collect(),
        ),
        Value::Array(items) => Value::Array(items.iter().map(contract_node).collect()),
        _ => value.clone(),
    }
}

fn schema_set_lock(
    path: &str,
    files: BTreeMap<String, SchemaFileLock>,
) -> Result<SchemaSetLock, AssetError> {
    let content_hashes: BTreeMap<&str, &str> = files
        .iter()
        .map(|(name, lock)| (name.as_str(), lock.content_sha256.as_str()))
        .collect();
    let contract_hashes: BTreeMap<&str, &str> = files
        .iter()
        .map(|(name, lock)| (name.as_str(), lock.contract_sha256.as_str()))
        .collect();
    Ok(SchemaSetLock {
        content_aggregate_sha256: sha256_json(&content_hashes)?,
        contract_aggregate_sha256: sha256_json(&contract_hashes)?,
        file_count: files.len(),
        files,
        path: path.to_owned(),
    })
}

fn sha256_json(value: &impl Serialize) -> Result<String, AssetError> {
    Ok(sha256_hex(&serde_json::to_vec(value)?))
}

fn normalize_line_endings(bytes: &[u8]) -> Vec<u8> {
    let mut normalized = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'\r' && bytes.get(index + 1) == Some(&b'\n') {
            normalized.push(b'\n');
            index += 2;
        } else {
            normalized.push(bytes[index]);
            index += 1;
        }
    }
    normalized
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_pointer_supports_objects_arrays_and_escapes() {
        let document = serde_json::json!({"a/b": [{"~key": true}], "with space": true});
        assert!(pointer_exists(&document, "/a~1b/0/~0key").unwrap());
        assert!(pointer_exists(&document, "/with%20space").unwrap());
        assert!(!pointer_exists(&document, "/a~1b/1").unwrap());
    }

    #[test]
    fn contract_nodes_drop_only_localized_descriptions() {
        let document = serde_json::json!({
            "description": "localized",
            "properties": {"description": {"type": "string"}},
            "title": "stable"
        });
        assert_eq!(
            contract_node(&document),
            serde_json::json!({"properties": {}, "title": "stable"})
        );
    }
}
