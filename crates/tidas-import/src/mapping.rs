use std::fmt::Write as _;
use std::fs::{self, File};
use std::io::{BufReader, BufWriter, Read, Write};
use std::path::Path;

use flate2::{Compression, GzBuilder};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use tempfile::Builder;
use thiserror::Error;
use tidas_runtime::{CancellationToken, MemoryBudget, RuntimeError};

use crate::store::{CanonicalStore, StoreError};

pub const MAPPING_CSV_COLUMNS: [&str; 24] = [
    "row_id",
    "source_format",
    "source_object",
    "source_entity_type",
    "source_entity_id",
    "source_entity_name",
    "source_exchange_id",
    "source_field_path",
    "source_field_name",
    "source_value",
    "target_dataset_type",
    "target_dataset_id",
    "target_dataset_name",
    "target_file",
    "target_scope",
    "target_field_path",
    "target_field_name",
    "target_value",
    "mapping_status",
    "mapping_category",
    "placeholder_kind",
    "trace_marker",
    "needs_review",
    "reviewer_notes",
];
const ENTITY_TYPES: [&str; 8] = [
    "contacts",
    "sources",
    "unitgroups",
    "flowproperties",
    "flows",
    "processes",
    "lciamethods",
    "lifecyclemodels",
];
const HASH_BUFFER_BYTES: usize = 1024 * 1024;

pub struct MappingCsvRequest<'a> {
    pub store: &'a CanonicalStore,
    pub output_path: &'a Path,
    pub source_format: &'a str,
    pub cancellation: &'a CancellationToken,
    pub memory_budget: &'a MemoryBudget,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct MappingCsvReportV1 {
    pub row_count: u64,
    pub output_bytes: u64,
    pub output_sha256: String,
    pub peak_accounted_memory_bytes: u64,
}

pub fn write_mapping_csv_gz(
    request: &MappingCsvRequest<'_>,
) -> Result<MappingCsvReportV1, MappingCsvError> {
    request.cancellation.check()?;
    let parent = request
        .output_path
        .parent()
        .unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)?;
    let staging = Builder::new()
        .prefix(".tidas-mapping-")
        .tempdir_in(parent)?;
    let staged_path = staging.path().join("mapping.csv.gz");
    let file = BufWriter::new(File::create(&staged_path)?);
    let mut gzip = GzBuilder::new()
        .mtime(0)
        .operating_system(255)
        .write(file, Compression::new(6));
    write_csv_row(&mut gzip, MAPPING_CSV_COLUMNS)?;
    let row_count = write_mapping_rows(&mut gzip, request)?;
    gzip.try_finish()?;
    drop(gzip);
    replace_file(&staged_path, request.output_path)?;
    let (output_bytes, output_sha256) = hash_file(
        request.output_path,
        request.cancellation,
        request.memory_budget,
    )?;
    Ok(MappingCsvReportV1 {
        row_count,
        output_bytes,
        output_sha256,
        peak_accounted_memory_bytes: request.memory_budget.peak(),
    })
}

fn write_mapping_rows(
    writer: &mut impl Write,
    request: &MappingCsvRequest<'_>,
) -> Result<u64, MappingCsvError> {
    let mut row_count = 0_u64;
    for entity_type in ENTITY_TYPES {
        for entity in request.store.iter_type(entity_type)? {
            request.cancellation.check()?;
            let entity = entity?;
            row_count = row_count
                .checked_add(1)
                .ok_or(MappingCsvError::SizeOverflow)?;
            let row_id = row_count.to_string();
            let external_id = entity.external_id.as_deref().unwrap_or(&entity.internal_id);
            let name = entity.name.as_deref().unwrap_or("");
            let target_file = format!("tidas/{entity_type}/{}.json", entity.internal_id);
            let status = if entity.external_id.is_some() {
                "formal"
            } else {
                "generated"
            };
            write_csv_row(
                writer,
                [
                    row_id.as_str(),
                    request.source_format,
                    "",
                    entity_type,
                    external_id,
                    name,
                    "",
                    "@id",
                    "@id",
                    external_id,
                    entity_type,
                    &entity.internal_id,
                    name,
                    &target_file,
                    "dataset",
                    "common:UUID",
                    "common:UUID",
                    &entity.internal_id,
                    status,
                    "identifier",
                    "",
                    "",
                    if status == "generated" {
                        "true"
                    } else {
                        "false"
                    },
                    "",
                ],
            )?;
            write_entity_detail_rows(
                writer,
                request.source_format,
                &entity,
                &target_file,
                &mut row_count,
            )?;
            if entity_type == "processes" {
                for (index, exchange) in request
                    .store
                    .iter_process_exchanges(&entity.internal_id)?
                    .enumerate()
                {
                    request.cancellation.check()?;
                    row_count = row_count
                        .checked_add(1)
                        .ok_or(MappingCsvError::SizeOverflow)?;
                    write_exchange_row(
                        writer,
                        request.source_format,
                        &entity.internal_id,
                        name,
                        &target_file,
                        row_count,
                        index,
                        &exchange?,
                    )?;
                }
            }
        }
    }
    Ok(row_count)
}

fn write_entity_detail_rows(
    writer: &mut impl Write,
    source_format: &str,
    entity: &crate::model::CanonicalEntity,
    target_file: &str,
    row_count: &mut u64,
) -> Result<(), MappingCsvError> {
    for (source_field, target_path, target_field, category) in [
        (
            "CASNumber",
            "flowInformation.dataSetInformation.CASNumber",
            "CASNumber",
            "cas",
        ),
        (
            "sumFormula",
            "flowInformation.dataSetInformation.sumFormula",
            "sumFormula",
            "chemical_identity",
        ),
        (
            "location",
            "processInformation.geography.locationOfOperationSupplyOrProduction.@location",
            "@location",
            "geography",
        ),
        (
            "referenceYear",
            "processInformation.time.common:referenceYear",
            "common:referenceYear",
            "time",
        ),
    ] {
        let Some(value) = scalar_text(entity.raw.get(source_field)) else {
            continue;
        };
        write_detail_row(
            writer,
            row_count,
            &DetailRow {
                source_format,
                entity,
                target_file,
                source_field_path: source_field,
                source_value: &value,
                target_field_path: target_path,
                target_field,
                target_value: &value,
                mapping_status: "formal",
                mapping_category: category,
                placeholder_kind: "",
                trace_marker: "",
                needs_review: "false",
            },
        )?;
    }
    if !entity.category_path.is_empty() || entity.raw.contains_key("flowType") {
        let value = if entity.category_path.is_empty() {
            scalar_text(entity.raw.get("flowType")).unwrap_or_default()
        } else {
            entity.category_path.join("/")
        };
        write_detail_row(
            writer,
            row_count,
            &DetailRow {
                source_format,
                entity,
                target_file,
                source_field_path: "category_path",
                source_value: &value,
                target_field_path: "dataSetInformation.classificationInformation.common:classification.common:other.tidasimport:sourceTrace",
                target_field: "tidasimport:sourceTrace",
                target_value: &value,
                mapping_status: "trace_only",
                mapping_category: "classification",
                placeholder_kind: "",
                trace_marker: "TIDAS_IMPORT_TRACE_V1",
                needs_review: "true",
            },
        )?;
    }
    if entity.entity_type == "processes" {
        write_detail_row(
            writer,
            row_count,
            &DetailRow {
                source_format,
                entity,
                target_file,
                source_field_path: "",
                source_value: "",
                target_field_path: "modellingAndValidation.dataSourcesTreatmentAndRepresentativeness.annualSupplyOrProductionVolume.#text",
                target_field: "#text",
                target_value: "0 kg/year; source production volume unavailable",
                mapping_status: "placeholder",
                mapping_category: "metadata",
                placeholder_kind: "SOURCE_PRODUCTION_VOLUME_UNAVAILABLE",
                trace_marker: "",
                needs_review: "true",
            },
        )?;
    }
    Ok(())
}

struct DetailRow<'a> {
    source_format: &'a str,
    entity: &'a crate::model::CanonicalEntity,
    target_file: &'a str,
    source_field_path: &'a str,
    source_value: &'a str,
    target_field_path: &'a str,
    target_field: &'a str,
    target_value: &'a str,
    mapping_status: &'a str,
    mapping_category: &'a str,
    placeholder_kind: &'a str,
    trace_marker: &'a str,
    needs_review: &'a str,
}

fn write_detail_row(
    writer: &mut impl Write,
    row_count: &mut u64,
    row: &DetailRow<'_>,
) -> Result<(), MappingCsvError> {
    *row_count = row_count
        .checked_add(1)
        .ok_or(MappingCsvError::SizeOverflow)?;
    let row_id = row_count.to_string();
    let source_id = row
        .entity
        .external_id
        .as_deref()
        .unwrap_or(&row.entity.internal_id);
    let name = row.entity.name.as_deref().unwrap_or("");
    write_csv_row(
        writer,
        [
            row_id.as_str(),
            row.source_format,
            "",
            &row.entity.entity_type,
            source_id,
            name,
            "",
            row.source_field_path,
            row.source_field_path,
            row.source_value,
            &row.entity.entity_type,
            &row.entity.internal_id,
            name,
            row.target_file,
            "dataset",
            row.target_field_path,
            row.target_field,
            row.target_value,
            row.mapping_status,
            row.mapping_category,
            row.placeholder_kind,
            row.trace_marker,
            row.needs_review,
            "",
        ],
    )
}

#[allow(clippy::too_many_arguments)]
fn write_exchange_row(
    writer: &mut impl Write,
    source_format: &str,
    process_id: &str,
    process_name: &str,
    target_file: &str,
    row_id: u64,
    index: usize,
    exchange: &Map<String, Value>,
) -> Result<(), MappingCsvError> {
    let source_exchange_id = text(exchange.get("internalId"))
        .map_or_else(|| index.saturating_add(1).to_string(), ToOwned::to_owned);
    let amount = text(exchange.get("amount")).unwrap_or("0");
    let target_path = format!("exchanges.exchange[{index}].meanAmount");
    let row_id = row_id.to_string();
    write_csv_row(
        writer,
        [
            row_id.as_str(),
            source_format,
            "",
            "processes",
            process_id,
            process_name,
            &source_exchange_id,
            "exchanges.amount",
            "amount",
            amount,
            "processes",
            process_id,
            process_name,
            target_file,
            "exchange",
            &target_path,
            "meanAmount",
            amount,
            "formal",
            "exchange",
            "",
            "",
            "false",
            "",
        ],
    )
}

fn write_csv_row<'a>(
    writer: &mut impl Write,
    fields: impl IntoIterator<Item = &'a str>,
) -> Result<(), MappingCsvError> {
    let mut first = true;
    for field in fields {
        if !first {
            writer.write_all(b",")?;
        }
        if field
            .bytes()
            .any(|byte| matches!(byte, b',' | b'"' | b'\n' | b'\r'))
        {
            writer.write_all(b"\"")?;
            let mut first_part = true;
            for part in field.split('"') {
                if !first_part {
                    writer.write_all(b"\"\"")?;
                }
                writer.write_all(part.as_bytes())?;
                first_part = false;
            }
            writer.write_all(b"\"")?;
        } else {
            writer.write_all(field.as_bytes())?;
        }
        first = false;
    }
    writer.write_all(b"\n")?;
    Ok(())
}

fn text(value: Option<&Value>) -> Option<&str> {
    match value {
        Some(Value::String(value)) => Some(value),
        _ => None,
    }
}

fn scalar_text(value: Option<&Value>) -> Option<String> {
    match value {
        Some(Value::String(value)) => Some(value.clone()),
        Some(Value::Number(value)) => Some(value.to_string()),
        Some(Value::Bool(value)) => Some(value.to_string()),
        _ => None,
    }
}

fn replace_file(source: &Path, target: &Path) -> Result<(), MappingCsvError> {
    if !target.exists() {
        fs::rename(source, target)?;
        return Ok(());
    }
    let parent = target.parent().unwrap_or_else(|| Path::new("."));
    let backup = Builder::new()
        .prefix(".tidas-mapping-backup-")
        .tempdir_in(parent)?;
    let previous = backup.path().join("previous");
    fs::rename(target, &previous)?;
    if let Err(source_error) = fs::rename(source, target) {
        let restore = fs::rename(&previous, target);
        return match restore {
            Ok(()) => Err(MappingCsvError::Io(source_error)),
            Err(restore_error) => Err(MappingCsvError::CommitRollback {
                source: source_error,
                restore: restore_error,
            }),
        };
    }
    Ok(())
}

fn hash_file(
    path: &Path,
    cancellation: &CancellationToken,
    budget: &MemoryBudget,
) -> Result<(u64, String), MappingCsvError> {
    let reservation =
        u64::try_from(HASH_BUFFER_BYTES).map_err(|_| MappingCsvError::SizeOverflow)?;
    let _reservation = budget.reserve(reservation)?;
    let mut buffer = vec![0_u8; HASH_BUFFER_BYTES];
    let mut reader = BufReader::new(File::open(path)?);
    let mut hasher = Sha256::new();
    let mut bytes = 0_u64;
    loop {
        cancellation.check()?;
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        bytes = bytes
            .checked_add(u64::try_from(read).map_err(|_| MappingCsvError::SizeOverflow)?)
            .ok_or(MappingCsvError::SizeOverflow)?;
        hasher.update(&buffer[..read]);
    }
    let digest = hasher.finalize();
    let mut hash = String::with_capacity(digest.len() * 2);
    for byte in digest {
        write!(&mut hash, "{byte:02x}").expect("writing to a String cannot fail");
    }
    Ok((bytes, hash))
}

#[derive(Debug, Error)]
pub enum MappingCsvError {
    #[error("mapping CSV size overflow")]
    SizeOverflow,
    #[error(
        "failed to commit mapping CSV and restore previous file: commit={source}; restore={restore}"
    )]
    CommitRollback {
        source: std::io::Error,
        restore: std::io::Error,
    },
    #[error("mapping CSV I/O failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("mapping CSV runtime failed: {0}")]
    Runtime(#[from] RuntimeError),
    #[error("mapping CSV canonical store failed: {0}")]
    Store(#[from] StoreError),
}

#[cfg(test)]
mod tests {
    use flate2::read::GzDecoder;
    use tempfile::tempdir;

    use super::*;
    use crate::model::CanonicalEntity;

    #[test]
    fn gzip_mapping_is_deterministic_and_streams_exchange_rows() {
        let directory = tempdir().unwrap();
        let mut store = CanonicalStore::create(Some(directory.path())).unwrap();
        let process = CanonicalEntity {
            entity_type: "processes".to_owned(),
            internal_id: "process".to_owned(),
            external_id: Some("external-process".to_owned()),
            name: Some("Process, quoted \"name\"".to_owned()),
            category_path: Vec::new(),
            raw: Map::new(),
        };
        store.add(&process).unwrap();
        store.begin_process_exchanges("process").unwrap();
        for index in 0..10_000 {
            store
                .add_process_exchange(
                    "process",
                    &Map::from_iter([
                        ("internalId".to_owned(), Value::String(index.to_string())),
                        ("amount".to_owned(), Value::String("1.25".to_owned())),
                    ]),
                )
                .unwrap();
        }
        let cancellation = CancellationToken::default();
        let memory_budget = MemoryBudget::new(4 * 1024 * 1024);
        let first = directory.path().join("first.csv.gz");
        let second = directory.path().join("second.csv.gz");
        let write = |path| {
            write_mapping_csv_gz(&MappingCsvRequest {
                store: &store,
                output_path: path,
                source_format: "fixture",
                cancellation: &cancellation,
                memory_budget: &memory_budget,
            })
            .unwrap()
        };
        let first_report = write(&first);
        let second_report = write(&second);
        assert_eq!(first_report.row_count, 10_002);
        assert_eq!(first_report.output_sha256, second_report.output_sha256);
        let mut csv = String::new();
        GzDecoder::new(File::open(first).unwrap())
            .read_to_string(&mut csv)
            .unwrap();
        assert_eq!(csv.lines().count(), 10_003);
        assert!(csv.contains("\"Process, quoted \"\"name\"\"\""));
        assert_eq!(memory_budget.used(), 0);
    }

    #[test]
    fn expert_mapping_preserves_cas_trace_and_placeholder_classes() {
        let directory = tempdir().unwrap();
        let mut store = CanonicalStore::create(Some(directory.path())).unwrap();
        store
            .add(&CanonicalEntity {
                entity_type: "flows".to_owned(),
                internal_id: "flow".to_owned(),
                external_id: Some("external-flow".to_owned()),
                name: Some("Carbon dioxide".to_owned()),
                category_path: vec!["Elementary flows".to_owned(), "air".to_owned()],
                raw: Map::from_iter([
                    ("CASNumber".to_owned(), Value::String("124-38-9".to_owned())),
                    (
                        "flowType".to_owned(),
                        Value::String("ELEMENTARY_FLOW".to_owned()),
                    ),
                ]),
            })
            .unwrap();
        store
            .add(&CanonicalEntity {
                entity_type: "processes".to_owned(),
                internal_id: "process".to_owned(),
                external_id: Some("external-process".to_owned()),
                name: Some("Process".to_owned()),
                category_path: Vec::new(),
                raw: Map::new(),
            })
            .unwrap();
        store.begin_process_exchanges("process").unwrap();
        let output = directory.path().join("mapping.csv.gz");
        write_mapping_csv_gz(&MappingCsvRequest {
            store: &store,
            output_path: &output,
            source_format: "fixture",
            cancellation: &CancellationToken::default(),
            memory_budget: &MemoryBudget::new(4 * 1024 * 1024),
        })
        .unwrap();
        let mut csv = String::new();
        GzDecoder::new(File::open(output).unwrap())
            .read_to_string(&mut csv)
            .unwrap();
        assert!(csv.contains("CASNumber,124-38-9"));
        assert!(csv.contains("trace_only,classification,,TIDAS_IMPORT_TRACE_V1,true"));
        assert!(csv.contains("placeholder,metadata,SOURCE_PRODUCTION_VOLUME_UNAVAILABLE,,true"));
    }
}
