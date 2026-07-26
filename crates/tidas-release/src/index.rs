use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::fs::{self, File};
use std::io::Read;
use std::path::{Component, Path, PathBuf};

use serde::Deserialize;
use sha2::{Digest, Sha256};
use tidas_runtime::MemoryReservation;

use crate::{ReleaseError, ReleaseRuntime};

const INDEX_MEMORY_MULTIPLIER: u64 = 4;
const HASH_BUFFER_BYTES: usize = 1024 * 1024;
const INDEX_SCHEMA: &str = "tiangong.release.canonical-dataset-index.v1";

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawDatasetIndex {
    schema_version: String,
    dataset_count: u64,
    #[allow(dead_code)]
    byte_size: u64,
    #[allow(dead_code)]
    artifact_set_hash: String,
    datasets: Vec<RawDatasetEntry>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawDatasetEntry {
    dataset_type: String,
    role: String,
    uuid: String,
    version: String,
    path: String,
    sha256: String,
    #[allow(dead_code)]
    byte_size: u64,
    canonical_content_hash: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct DatasetEntry {
    pub dataset_type: String,
    pub role: String,
    pub uuid: String,
    pub version: String,
    pub relative_path: String,
    pub sha256: String,
    #[allow(dead_code)]
    pub canonical_content_hash: String,
}

impl DatasetEntry {
    pub fn key(&self) -> String {
        format!(
            "{}:{}:{}",
            self.dataset_type,
            self.uuid.to_lowercase(),
            self.version
        )
    }
}

#[derive(Debug)]
pub(crate) struct DatasetIndex {
    entries: Vec<DatasetEntry>,
    _reservation: MemoryReservation,
}

impl DatasetIndex {
    pub fn load(
        index_path: &Path,
        input_dir: &Path,
        runtime: &ReleaseRuntime,
    ) -> Result<Self, ReleaseError> {
        runtime.cancellation.check()?;
        if !input_dir.is_dir() {
            return Err(ReleaseError::InputNotDirectory(input_dir.to_path_buf()));
        }
        let metadata = fs::metadata(index_path)?;
        let reserved = metadata
            .len()
            .checked_mul(INDEX_MEMORY_MULTIPLIER)
            .ok_or(ReleaseError::SizeOverflow)?;
        let reservation = runtime.memory_budget.reserve(reserved)?;
        let bytes = fs::read(index_path)?;
        let raw: RawDatasetIndex = serde_json::from_slice(&bytes)
            .map_err(|error| ReleaseError::DatasetIndexInvalid(error.to_string()))?;
        if raw.schema_version != INDEX_SCHEMA {
            return Err(ReleaseError::DatasetIndexSchemaUnsupported(
                raw.schema_version,
            ));
        }
        if raw.datasets.is_empty() {
            return Err(ReleaseError::DatasetIndexEmpty);
        }
        if raw.dataset_count
            != u64::try_from(raw.datasets.len()).map_err(|_| ReleaseError::SizeOverflow)?
        {
            return Err(ReleaseError::DatasetIndexInvalid(
                "datasetCount does not match datasets length".to_owned(),
            ));
        }

        let mut seen_keys = BTreeSet::new();
        let mut seen_paths = BTreeSet::new();
        let mut entries = Vec::with_capacity(raw.datasets.len());
        for raw_entry in raw.datasets {
            runtime.cancellation.check()?;
            validate_hash(&raw_entry.sha256)?;
            validate_hash(&raw_entry.canonical_content_hash)?;
            let relative = safe_relative(&raw_entry.path)?;
            let entry = DatasetEntry {
                dataset_type: required(raw_entry.dataset_type, "datasetType")?,
                role: required(raw_entry.role, "role")?,
                uuid: required(raw_entry.uuid, "uuid")?.to_lowercase(),
                version: required(raw_entry.version, "version")?,
                relative_path: relative,
                sha256: raw_entry.sha256.to_lowercase(),
                canonical_content_hash: raw_entry.canonical_content_hash.to_lowercase(),
            };
            let key = entry.key();
            if !seen_keys.insert(key.clone()) {
                return Err(ReleaseError::DuplicateDatasetIdentity(key));
            }
            if !seen_paths.insert(entry.relative_path.clone()) {
                return Err(ReleaseError::DuplicateDatasetPath(
                    entry.relative_path.clone(),
                ));
            }
            let path = contained(input_dir, &entry.relative_path)?;
            let file_metadata = fs::symlink_metadata(&path)
                .map_err(|_| ReleaseError::DatasetFileMissing(entry.relative_path.clone()))?;
            if file_metadata.file_type().is_symlink() {
                return Err(ReleaseError::Symlink(path));
            }
            if !file_metadata.is_file() {
                return Err(ReleaseError::DatasetFileMissing(
                    entry.relative_path.clone(),
                ));
            }
            if sha256_file(&path, runtime)? != entry.sha256 {
                return Err(ReleaseError::DatasetFileHashMismatch(
                    entry.relative_path.clone(),
                ));
            }
            entries.push(entry);
        }
        entries.sort_by_key(DatasetEntry::key);
        Ok(Self {
            entries,
            _reservation: reservation,
        })
    }

    pub fn entries(&self) -> &[DatasetEntry] {
        &self.entries
    }

    pub fn get(&self, key: &str) -> Option<&DatasetEntry> {
        self.entries
            .binary_search_by(|entry| entry.key().as_str().cmp(key))
            .ok()
            .map(|position| &self.entries[position])
    }
}

fn required(value: String, field: &str) -> Result<String, ReleaseError> {
    if value.is_empty() {
        Err(ReleaseError::DatasetIndexInvalid(format!(
            "{field} must not be empty"
        )))
    } else {
        Ok(value)
    }
}

fn validate_hash(value: &str) -> Result<(), ReleaseError> {
    if value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        Ok(())
    } else {
        Err(ReleaseError::DatasetIndexInvalid(format!(
            "invalid SHA-256 value: {value}"
        )))
    }
}

pub(crate) fn safe_relative(raw: &str) -> Result<String, ReleaseError> {
    if raw.is_empty() || raw.contains('\\') {
        return Err(ReleaseError::UnsafePath(PathBuf::from(raw)));
    }
    let path = Path::new(raw);
    if path.is_absolute()
        || path
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(ReleaseError::UnsafePath(path.to_path_buf()));
    }
    if raw
        .split('/')
        .any(|part| part.is_empty() || part == "." || part == ".." || !is_portable_component(part))
    {
        return Err(ReleaseError::UnsafePath(path.to_path_buf()));
    }
    Ok(raw.to_owned())
}

fn is_portable_component(value: &str) -> bool {
    if value.ends_with([' ', '.'])
        || value
            .chars()
            .any(|character| character.is_control() || r#"<>:"|?*"#.contains(character))
    {
        return false;
    }
    let stem = value
        .split_once('.')
        .map_or(value, |(candidate, _)| candidate)
        .to_ascii_uppercase();
    !matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        && !(stem.len() == 4
            && (stem.starts_with("COM") || stem.starts_with("LPT"))
            && matches!(stem.as_bytes()[3], b'1'..=b'9'))
}

pub(crate) fn contained(root: &Path, relative: &str) -> Result<PathBuf, ReleaseError> {
    let safe = safe_relative(relative)?;
    let path = root.join(safe);
    if path.strip_prefix(root).is_err() {
        return Err(ReleaseError::PathOutsideRoot(path));
    }
    Ok(path)
}

pub(crate) fn sha256_file(path: &Path, runtime: &ReleaseRuntime) -> Result<String, ReleaseError> {
    let buffer_bytes = u64::try_from(HASH_BUFFER_BYTES).map_err(|_| ReleaseError::SizeOverflow)?;
    let _reservation = runtime.memory_budget.reserve(buffer_bytes)?;
    let mut file = File::open(path)?;
    let mut buffer = vec![0_u8; HASH_BUFFER_BYTES];
    let mut hasher = Sha256::new();
    loop {
        runtime.cancellation.check()?;
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hex_digest(hasher.finalize()))
}

pub(crate) fn hex_digest(digest: impl AsRef<[u8]>) -> String {
    let bytes = digest.as_ref();
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        write!(&mut output, "{byte:02x}").expect("writing to a String cannot fail");
    }
    output
}
