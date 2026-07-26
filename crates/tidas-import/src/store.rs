use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::fs::{self, File};
use std::io::{BufRead, BufReader, BufWriter, Lines, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use tempfile::TempDir;
use thiserror::Error;

use crate::model::CanonicalEntity;

#[derive(Debug)]
pub struct CanonicalStore {
    root: TempDir,
    counts: BTreeMap<String, u64>,
}

impl CanonicalStore {
    pub fn create(parent: Option<&Path>) -> Result<Self, StoreError> {
        let root = match parent {
            Some(parent) => tempfile::Builder::new()
                .prefix(".tidas-import-spool-")
                .tempdir_in(parent)?,
            None => tempfile::Builder::new()
                .prefix("tidas-import-spool-")
                .tempdir()?,
        };
        Ok(Self {
            root,
            counts: BTreeMap::new(),
        })
    }

    pub fn add(&mut self, entity: &CanonicalEntity) -> Result<(), StoreError> {
        validate_file_key("entity_type", &entity.entity_type)?;
        validate_file_key("internal_id", &entity.internal_id)?;
        let entity_path = self.entity_path(&entity.entity_type, &entity.internal_id);
        let replacing = entity_path.is_file();
        let previous = replacing
            .then(|| read_json::<CanonicalEntity>(&entity_path))
            .transpose()?;
        write_json_atomic(&entity_path, entity)?;
        if !replacing {
            append_json_line(&self.order_path(&entity.entity_type), &entity.internal_id)?;
        }
        if let Some(previous_external_id) = previous
            .as_ref()
            .and_then(|previous| previous.external_id.as_deref())
            .filter(|previous| Some(*previous) != entity.external_id.as_deref())
        {
            let stale_pointer = self.external_path(&entity.entity_type, previous_external_id);
            if stale_pointer.is_file() {
                fs::remove_file(stale_pointer)?;
            }
        }
        if let Some(external_id) = entity.external_id.as_deref() {
            validate_key("external_id", external_id)?;
            let pointer_path = self.external_path(&entity.entity_type, external_id);
            write_json_atomic(
                &pointer_path,
                &ExternalPointer {
                    internal_id: entity.internal_id.clone(),
                },
            )?;
        }
        if !replacing {
            *self.counts.entry(entity.entity_type.clone()).or_default() += 1;
        }
        Ok(())
    }

    pub fn get(
        &self,
        entity_type: &str,
        internal_id: &str,
    ) -> Result<Option<CanonicalEntity>, StoreError> {
        validate_file_key("entity_type", entity_type)?;
        validate_file_key("internal_id", internal_id)?;
        read_json_optional(&self.entity_path(entity_type, internal_id))
    }

    pub fn get_by_external_id(
        &self,
        entity_type: &str,
        external_id: &str,
    ) -> Result<Option<CanonicalEntity>, StoreError> {
        validate_file_key("entity_type", entity_type)?;
        validate_key("external_id", external_id)?;
        let pointer: Option<ExternalPointer> =
            read_json_optional(&self.external_path(entity_type, external_id))?;
        match pointer {
            Some(pointer) => self.get(entity_type, &pointer.internal_id),
            None => Ok(None),
        }
    }

    #[must_use]
    pub fn counts(&self) -> &BTreeMap<String, u64> {
        &self.counts
    }

    pub fn remove_type(&mut self, entity_type: &str) -> Result<(), StoreError> {
        validate_file_key("entity_type", entity_type)?;
        for directory in [
            self.root
                .path()
                .join("entities")
                .join(key_hash(entity_type)),
            self.root
                .path()
                .join("external")
                .join(key_hash(entity_type)),
        ] {
            if directory.exists() {
                fs::remove_dir_all(directory)?;
            }
        }
        let order = self.order_path(entity_type);
        if order.exists() {
            fs::remove_file(order)?;
        }
        self.counts.remove(entity_type);
        Ok(())
    }

    pub fn iter_type(&self, entity_type: &str) -> Result<EntityIter<'_>, StoreError> {
        validate_file_key("entity_type", entity_type)?;
        let order_path = self.order_path(entity_type);
        let lines = if order_path.is_file() {
            Some(BufReader::new(File::open(order_path)?).lines())
        } else {
            None
        };
        Ok(EntityIter {
            store: self,
            entity_type: entity_type.to_owned(),
            lines,
        })
    }

    pub fn begin_process_exchanges(&self, process_id: &str) -> Result<(), StoreError> {
        validate_file_key("process_id", process_id)?;
        let path = self.exchange_path(process_id);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        BufWriter::new(File::create(path)?).flush()?;
        Ok(())
    }

    pub fn add_process_exchange(
        &self,
        process_id: &str,
        exchange: &Map<String, Value>,
    ) -> Result<(), StoreError> {
        validate_file_key("process_id", process_id)?;
        append_json_line(&self.exchange_path(process_id), exchange)
    }

    pub fn iter_process_exchanges(&self, process_id: &str) -> Result<ExchangeIter, StoreError> {
        validate_file_key("process_id", process_id)?;
        let path = self.exchange_path(process_id);
        let lines = if path.is_file() {
            Some(BufReader::new(File::open(path)?).lines())
        } else {
            None
        };
        Ok(ExchangeIter { lines })
    }

    pub fn rewrite_process_exchanges<E>(
        &self,
        process_id: &str,
        mut transform: impl FnMut(&mut Map<String, Value>) -> Result<(), E>,
    ) -> Result<(), E>
    where
        E: From<StoreError>,
    {
        validate_file_key("process_id", process_id).map_err(E::from)?;
        let path = self.exchange_path(process_id);
        let temporary = path.with_extension("jsonl.tmp");
        let mut writer = BufWriter::new(
            File::create(&temporary)
                .map_err(StoreError::from)
                .map_err(E::from)?,
        );
        for exchange in self.iter_process_exchanges(process_id).map_err(E::from)? {
            let mut exchange = exchange.map_err(E::from)?;
            transform(&mut exchange)?;
            serde_json::to_writer(&mut writer, &exchange)
                .map_err(StoreError::from)
                .map_err(E::from)?;
            writer
                .write_all(b"\n")
                .map_err(StoreError::from)
                .map_err(E::from)?;
        }
        writer.flush().map_err(StoreError::from).map_err(E::from)?;
        replace_file(&temporary, &path)
            .map_err(StoreError::from)
            .map_err(E::from)?;
        Ok(())
    }

    fn entity_path(&self, entity_type: &str, internal_id: &str) -> PathBuf {
        self.root
            .path()
            .join("entities")
            .join(key_hash(entity_type))
            .join(format!("{}.json", key_hash(internal_id)))
    }

    fn external_path(&self, entity_type: &str, external_id: &str) -> PathBuf {
        self.root
            .path()
            .join("external")
            .join(key_hash(entity_type))
            .join(format!("{}.json", key_hash(external_id)))
    }

    fn order_path(&self, entity_type: &str) -> PathBuf {
        self.root
            .path()
            .join("order")
            .join(format!("{}.jsonl", key_hash(entity_type)))
    }

    fn exchange_path(&self, process_id: &str) -> PathBuf {
        self.root
            .path()
            .join("exchanges")
            .join(format!("{}.jsonl", key_hash(process_id)))
    }
}

pub struct EntityIter<'a> {
    store: &'a CanonicalStore,
    entity_type: String,
    lines: Option<Lines<BufReader<File>>>,
}

impl Iterator for EntityIter<'_> {
    type Item = Result<CanonicalEntity, StoreError>;

    fn next(&mut self) -> Option<Self::Item> {
        let line = self.lines.as_mut()?.next()?;
        Some(
            line.map_err(StoreError::from)
                .and_then(|line| serde_json::from_str::<String>(&line).map_err(StoreError::from))
                .and_then(|internal_id| {
                    self.store.get(&self.entity_type, &internal_id)?.ok_or(
                        StoreError::MissingOrderedEntity {
                            entity_type: self.entity_type.clone(),
                            internal_id,
                        },
                    )
                }),
        )
    }
}

pub struct ExchangeIter {
    lines: Option<Lines<BufReader<File>>>,
}

impl Iterator for ExchangeIter {
    type Item = Result<Map<String, Value>, StoreError>;

    fn next(&mut self) -> Option<Self::Item> {
        let line = self.lines.as_mut()?.next()?;
        Some(
            line.map_err(StoreError::from)
                .and_then(|line| serde_json::from_str(&line).map_err(StoreError::from)),
        )
    }
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ExternalPointer {
    internal_id: String,
}

fn validate_key(label: &'static str, value: &str) -> Result<(), StoreError> {
    if value.is_empty() || value.len() > 4096 || value.chars().any(char::is_control) {
        return Err(StoreError::InvalidKey {
            label,
            value: value.to_owned(),
        });
    }
    Ok(())
}

fn validate_file_key(label: &'static str, value: &str) -> Result<(), StoreError> {
    validate_key(label, value)?;
    if matches!(value, "." | "..")
        || value
            .chars()
            .any(|character| matches!(character, '/' | '\\'))
    {
        return Err(StoreError::InvalidKey {
            label,
            value: value.to_owned(),
        });
    }
    Ok(())
}

fn key_hash(value: &str) -> String {
    let digest = Sha256::digest(value.as_bytes());
    let mut output = String::with_capacity(digest.len() * 2);
    for byte in digest {
        write!(&mut output, "{byte:02x}").expect("writing to a String is infallible");
    }
    output
}

fn write_json_atomic<T: Serialize>(path: &Path, value: &T) -> Result<(), StoreError> {
    let parent = path.parent().ok_or(StoreError::MissingParent)?;
    fs::create_dir_all(parent)?;
    let temporary = path.with_extension("json.tmp");
    {
        let mut writer = BufWriter::new(File::create(&temporary)?);
        serde_json::to_writer(&mut writer, value)?;
        writer.write_all(b"\n")?;
        writer.flush()?;
    }
    replace_file(&temporary, path)?;
    Ok(())
}

fn replace_file(source: &Path, target: &Path) -> std::io::Result<()> {
    if !target.exists() {
        return fs::rename(source, target);
    }

    let parent = target.parent().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "canonical store target has no parent",
        )
    })?;
    let backup = tempfile::Builder::new()
        .prefix(".tidas-import-store-backup-")
        .tempdir_in(parent)?;
    let previous = backup.path().join("previous");
    fs::rename(target, &previous)?;
    if let Err(commit_error) = fs::rename(source, target) {
        return match fs::rename(&previous, target) {
            Ok(()) => Err(commit_error),
            Err(restore_error) => Err(std::io::Error::other(format!(
                "failed to replace canonical store file and restore the previous file: \
                 commit={commit_error}; restore={restore_error}"
            ))),
        };
    }
    Ok(())
}

fn append_json_line<T: Serialize>(path: &Path, value: &T) -> Result<(), StoreError> {
    let parent = path.parent().ok_or(StoreError::MissingParent)?;
    fs::create_dir_all(parent)?;
    let mut writer = BufWriter::new(
        fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)?,
    );
    serde_json::to_writer(&mut writer, value)?;
    writer.write_all(b"\n")?;
    writer.flush()?;
    Ok(())
}

fn read_json_optional<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<Option<T>, StoreError> {
    if !path.is_file() {
        return Ok(None);
    }
    read_json(path).map(Some)
}

fn read_json<T: for<'de> Deserialize<'de>>(path: impl AsRef<Path>) -> Result<T, StoreError> {
    Ok(serde_json::from_reader(BufReader::new(File::open(
        path.as_ref(),
    )?))?)
}

#[derive(Debug, Error)]
pub enum StoreError {
    #[error("canonical store key {label} is invalid: {value:?}")]
    InvalidKey { label: &'static str, value: String },
    #[error("canonical store path has no parent")]
    MissingParent,
    #[error("canonical order references missing {entity_type} entity {internal_id}")]
    MissingOrderedEntity {
        entity_type: String,
        internal_id: String,
    },
    #[error("canonical store I/O failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("canonical store JSON failed: {0}")]
    Json(#[from] serde_json::Error),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entity(internal_id: &str, external_id: Option<&str>) -> CanonicalEntity {
        CanonicalEntity {
            entity_type: "flows".to_owned(),
            internal_id: internal_id.to_owned(),
            external_id: external_id.map(str::to_owned),
            name: Some(format!("Flow {internal_id}")),
            category_path: vec!["Products".to_owned()],
            raw: serde_json::Map::new(),
        }
    }

    #[test]
    fn disk_store_replaces_by_id_and_resolves_external_ids() {
        let mut store = CanonicalStore::create(None).unwrap();
        store.add(&entity("b", Some("external-b"))).unwrap();
        store.add(&entity("a", None)).unwrap();
        let mut replacement = entity("b", Some("external-b"));
        replacement.name = Some("Replacement".to_owned());
        store.add(&replacement).unwrap();

        assert_eq!(store.counts().get("flows"), Some(&2));
        assert_eq!(
            store
                .get_by_external_id("flows", "external-b")
                .unwrap()
                .unwrap()
                .name
                .as_deref(),
            Some("Replacement")
        );
        let names = store
            .iter_type("flows")
            .unwrap()
            .map(|entity| entity.unwrap().name.unwrap())
            .collect::<Vec<_>>();
        assert_eq!(names, ["Replacement", "Flow a"]);
        assert!(
            store
                .get_by_external_id("flows", "external-b")
                .unwrap()
                .is_some()
        );

        replacement.external_id = Some("external-b-new".to_owned());
        store.add(&replacement).unwrap();
        assert!(
            store
                .get_by_external_id("flows", "external-b")
                .unwrap()
                .is_none()
        );
        assert!(
            store
                .get_by_external_id("flows", "external-b-new")
                .unwrap()
                .is_some()
        );
        assert_eq!(store.iter_type("flows").unwrap().count(), 2);
    }

    #[test]
    fn invalid_keys_fail_before_touching_the_spool() {
        let mut store = CanonicalStore::create(None).unwrap();
        let mut invalid = entity("a", None);
        invalid.entity_type = String::new();
        assert!(matches!(
            store.add(&invalid),
            Err(StoreError::InvalidKey {
                label: "entity_type",
                ..
            })
        ));
        invalid.entity_type = "flows".to_owned();
        invalid.internal_id = "../escape".to_owned();
        assert!(matches!(
            store.add(&invalid),
            Err(StoreError::InvalidKey {
                label: "internal_id",
                ..
            })
        ));
    }

    #[test]
    fn process_exchanges_are_streamed_from_disk_in_source_order() {
        let store = CanonicalStore::create(None).unwrap();
        store.begin_process_exchanges("process").unwrap();
        store
            .add_process_exchange(
                "process",
                &Map::from_iter([("id".to_owned(), Value::from(1))]),
            )
            .unwrap();
        store
            .add_process_exchange(
                "process",
                &Map::from_iter([("id".to_owned(), Value::from(2))]),
            )
            .unwrap();
        let ids = store
            .iter_process_exchanges("process")
            .unwrap()
            .map(|item| item.unwrap()["id"].as_i64().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(ids, [1, 2]);
    }

    #[test]
    fn process_exchanges_can_be_rewritten_without_collecting_the_spool() {
        let store = CanonicalStore::create(None).unwrap();
        store.begin_process_exchanges("process").unwrap();
        for amount in ["1", "2"] {
            store
                .add_process_exchange(
                    "process",
                    &Map::from_iter([("amount".to_owned(), Value::String(amount.to_owned()))]),
                )
                .unwrap();
        }
        store
            .rewrite_process_exchanges::<StoreError>("process", |exchange| {
                exchange.insert("normalized".to_owned(), Value::Bool(true));
                Ok(())
            })
            .unwrap();
        let exchanges = store
            .iter_process_exchanges("process")
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(exchanges.len(), 2);
        assert!(exchanges.iter().all(|value| value["normalized"] == true));
    }

    #[test]
    fn auxiliary_entity_types_can_be_removed_after_resolution() {
        let mut store = CanonicalStore::create(None).unwrap();
        store.add(&entity("a", Some("external-a"))).unwrap();
        assert_eq!(store.counts()["flows"], 1);
        store.remove_type("flows").unwrap();
        assert!(!store.counts().contains_key("flows"));
        assert!(store.get("flows", "a").unwrap().is_none());
        assert!(
            store
                .get_by_external_id("flows", "external-a")
                .unwrap()
                .is_none()
        );
        assert_eq!(store.iter_type("flows").unwrap().count(), 0);
    }
}
