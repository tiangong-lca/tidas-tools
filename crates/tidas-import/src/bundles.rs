use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tempfile::{Builder, TempDir};
use thiserror::Error;
use tidas_runtime::{CancellationToken, MemoryBudget, RuntimeError};
use walkdir::WalkDir;

use crate::store::{CanonicalStore, StoreError};

const BUNDLE_CATEGORIES: [&str; 6] = [
    "contacts",
    "sources",
    "unitgroups",
    "flowproperties",
    "flows",
    "processes",
];

pub struct ProcessBundleRequest<'a> {
    pub store: &'a CanonicalStore,
    pub tidas_dir: &'a Path,
    pub output_dir: &'a Path,
    pub cancellation: &'a CancellationToken,
    pub memory_budget: &'a MemoryBudget,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProcessBundleReportV1 {
    pub process_count: u64,
    pub unresolved_reference_count: u64,
    pub peak_accounted_memory_bytes: u64,
}

pub fn write_process_bundles(
    request: &ProcessBundleRequest<'_>,
) -> Result<ProcessBundleReportV1, ProcessBundleError> {
    request.cancellation.check()?;
    if !request.tidas_dir.is_dir() {
        return Err(ProcessBundleError::InputNotDirectory(
            request.tidas_dir.to_path_buf(),
        ));
    }
    let staging = StagedDirectory::new(request.output_dir)?;
    let aggregate_unresolved = staging.path().join(".unresolved.jsonl");
    let mut aggregate_writer = BufWriter::new(File::create(&aggregate_unresolved)?);
    let index_path = staging.path().join("index.json");
    let mut index = BufWriter::new(File::create(&index_path)?);
    index.write_all(br#"{"schema_version":1,"bundles":["#)?;
    let mut process_count = 0_u64;
    let mut unresolved_count = 0_u64;
    for process in request.store.iter_type("processes")? {
        request.cancellation.check()?;
        let process = process?;
        let bundle_dir = staging.path().join(&process.internal_id);
        let tidas_bundle = bundle_dir.join("tidas");
        create_category_dirs(&tidas_bundle)?;
        let unresolved_path = bundle_dir.join(".unresolved.jsonl");
        let mut unresolved = BufWriter::new(File::create(&unresolved_path)?);
        let (counts, process_unresolved) = materialize_process_bundle(
            request,
            &process.internal_id,
            &tidas_bundle,
            &mut unresolved,
            &mut aggregate_writer,
        )?;
        unresolved.flush()?;
        unresolved_count = unresolved_count
            .checked_add(process_unresolved)
            .ok_or(ProcessBundleError::SizeOverflow)?;
        write_manifest(
            &bundle_dir,
            &process.internal_id,
            &counts,
            &unresolved_path,
            process_unresolved,
            request,
        )?;
        fs::remove_file(unresolved_path)?;
        if process_count > 0 {
            index.write_all(b",")?;
        }
        serde_json::to_writer(
            &mut index,
            &json!({
                "process_id": process.internal_id,
                "manifest": format!("{}/manifest.json", process.internal_id),
                "tidas_dir": format!("{}/tidas", process.internal_id),
                "dependency_counts": counts,
            }),
        )?;
        process_count = process_count
            .checked_add(1)
            .ok_or(ProcessBundleError::SizeOverflow)?;
    }
    aggregate_writer.flush()?;
    index.write_all(br#"],"unresolved_references":["#)?;
    stream_jsonl_array(&mut index, &aggregate_unresolved)?;
    index.write_all(b"]}\n")?;
    index.flush()?;
    fs::remove_file(aggregate_unresolved)?;
    request.cancellation.check()?;
    staging.commit()?;
    Ok(ProcessBundleReportV1 {
        process_count,
        unresolved_reference_count: unresolved_count,
        peak_accounted_memory_bytes: request.memory_budget.peak(),
    })
}

fn materialize_process_bundle(
    request: &ProcessBundleRequest<'_>,
    process_id: &str,
    bundle: &Path,
    unresolved: &mut impl Write,
    aggregate_unresolved: &mut impl Write,
) -> Result<(BTreeMap<String, u64>, u64), ProcessBundleError> {
    let mut counts = BTreeMap::new();
    copy_dependency(
        request,
        bundle,
        process_id,
        "processes",
        process_id,
        &mut counts,
        unresolved,
        aggregate_unresolved,
    )?;
    for (category, id) in [
        ("contacts", crate::writers::contact_id_for_import()),
        ("sources", crate::writers::format_source_id_for_import()),
        ("sources", crate::writers::compliance_source_id_for_import()),
    ] {
        copy_dependency(
            request,
            bundle,
            process_id,
            category,
            &id,
            &mut counts,
            unresolved,
            aggregate_unresolved,
        )?;
    }
    let mut unresolved_count = 0_u64;
    for exchange in request.store.iter_process_exchanges(process_id)? {
        request.cancellation.check()?;
        let exchange = exchange?;
        let Some(flow_id) = exchange.get("flowRefId").and_then(Value::as_str) else {
            unresolved_count = unresolved_count.saturating_add(1);
            write_unresolved(
                unresolved,
                aggregate_unresolved,
                process_id,
                "flows",
                "",
                "exchange.flowRefId",
            )?;
            continue;
        };
        if !copy_dependency(
            request,
            bundle,
            process_id,
            "flows",
            flow_id,
            &mut counts,
            unresolved,
            aggregate_unresolved,
        )? {
            unresolved_count = unresolved_count.saturating_add(1);
            continue;
        }
        let Some(flow) = request.store.get("flows", flow_id)? else {
            continue;
        };
        let Some(property_id) = flow.raw.get("flowPropertyRefId").and_then(Value::as_str) else {
            continue;
        };
        if !copy_dependency(
            request,
            bundle,
            process_id,
            "flowproperties",
            property_id,
            &mut counts,
            unresolved,
            aggregate_unresolved,
        )? {
            unresolved_count = unresolved_count.saturating_add(1);
            continue;
        }
        if let Some(property) = request.store.get("flowproperties", property_id)?
            && let Some(unit_group_id) = property.raw.get("unitGroupRefId").and_then(Value::as_str)
            && !copy_dependency(
                request,
                bundle,
                process_id,
                "unitgroups",
                unit_group_id,
                &mut counts,
                unresolved,
                aggregate_unresolved,
            )?
        {
            unresolved_count = unresolved_count.saturating_add(1);
        }
    }
    Ok((counts, unresolved_count))
}

#[allow(clippy::too_many_arguments)]
fn copy_dependency(
    request: &ProcessBundleRequest<'_>,
    bundle: &Path,
    process_id: &str,
    category: &str,
    id: &str,
    counts: &mut BTreeMap<String, u64>,
    unresolved: &mut impl Write,
    aggregate_unresolved: &mut impl Write,
) -> Result<bool, ProcessBundleError> {
    request.cancellation.check()?;
    let source = request.tidas_dir.join(category).join(format!("{id}.json"));
    if !source.is_file() {
        write_unresolved(
            unresolved,
            aggregate_unresolved,
            process_id,
            category,
            id,
            "dependency",
        )?;
        return Ok(false);
    }
    let target = bundle.join(category).join(format!("{id}.json"));
    if !target.exists() {
        fs::copy(source, target)?;
        *counts.entry(category.to_owned()).or_default() += 1;
    }
    Ok(true)
}

fn write_unresolved(
    local: &mut impl Write,
    aggregate: &mut impl Write,
    process_id: &str,
    category: &str,
    id: &str,
    path: &str,
) -> Result<(), ProcessBundleError> {
    let value = json!({
        "process_id": process_id,
        "category": category,
        "ref_id": id,
        "path": path,
    });
    serde_json::to_writer(&mut *local, &value)?;
    local.write_all(b"\n")?;
    serde_json::to_writer(&mut *aggregate, &value)?;
    aggregate.write_all(b"\n")?;
    Ok(())
}

fn write_manifest(
    bundle_dir: &Path,
    process_id: &str,
    counts: &BTreeMap<String, u64>,
    unresolved_path: &Path,
    unresolved_count: u64,
    request: &ProcessBundleRequest<'_>,
) -> Result<(), ProcessBundleError> {
    let mut writer = BufWriter::new(File::create(bundle_dir.join("manifest.json"))?);
    writer.write_all(b"{")?;
    serde_json::to_writer(&mut writer, "schema_version")?;
    writer.write_all(b":1,")?;
    serde_json::to_writer(&mut writer, "process_id")?;
    writer.write_all(b":")?;
    serde_json::to_writer(&mut writer, process_id)?;
    writer.write_all(b",\"bundle_tidas_dir\":\"tidas\",\"dependency_counts\":")?;
    serde_json::to_writer(&mut writer, counts)?;
    writer.write_all(b",\"files\":{")?;
    for (category_index, category) in BUNDLE_CATEGORIES.iter().enumerate() {
        request.cancellation.check()?;
        if category_index > 0 {
            writer.write_all(b",")?;
        }
        serde_json::to_writer(&mut writer, category)?;
        writer.write_all(b":[")?;
        let category_dir = bundle_dir.join("tidas").join(category);
        let mut first = true;
        for entry in WalkDir::new(&category_dir).max_depth(1).sort_by_file_name() {
            let entry = entry?;
            if !entry.file_type().is_file() {
                continue;
            }
            if !first {
                writer.write_all(b",")?;
            }
            let file_name = entry.file_name().to_string_lossy();
            serde_json::to_writer(&mut writer, &format!("tidas/{category}/{file_name}"))?;
            first = false;
        }
        writer.write_all(b"]")?;
    }
    writer.write_all(b"},\"unresolved_reference_count\":")?;
    serde_json::to_writer(&mut writer, &unresolved_count)?;
    writer.write_all(b",\"unresolved_references\":[")?;
    stream_jsonl_array(&mut writer, unresolved_path)?;
    writer.write_all(b"]}\n")?;
    writer.flush()?;
    Ok(())
}

fn stream_jsonl_array(writer: &mut impl Write, path: &Path) -> Result<(), ProcessBundleError> {
    let mut first = true;
    for line in BufReader::new(File::open(path)?).lines() {
        let line = line?;
        if line.is_empty() {
            continue;
        }
        if !first {
            writer.write_all(b",")?;
        }
        writer.write_all(line.as_bytes())?;
        first = false;
    }
    Ok(())
}

fn create_category_dirs(root: &Path) -> Result<(), ProcessBundleError> {
    for category in [
        "contacts",
        "sources",
        "unitgroups",
        "flowproperties",
        "flows",
        "processes",
        "lciamethods",
        "lifecyclemodels",
    ] {
        fs::create_dir_all(root.join(category))?;
    }
    Ok(())
}

struct StagedDirectory {
    target: PathBuf,
    staging: TempDir,
}

impl StagedDirectory {
    fn new(target: &Path) -> Result<Self, ProcessBundleError> {
        let parent = target.parent().unwrap_or_else(|| Path::new("."));
        fs::create_dir_all(parent)?;
        Ok(Self {
            target: target.to_path_buf(),
            staging: Builder::new()
                .prefix(".tidas-process-bundles-")
                .tempdir_in(parent)?,
        })
    }

    fn path(&self) -> &Path {
        self.staging.path()
    }

    fn commit(self) -> Result<(), ProcessBundleError> {
        let parent = self.target.parent().unwrap_or_else(|| Path::new("."));
        if !self.target.exists() {
            fs::rename(self.staging.path(), self.target)?;
            return Ok(());
        }
        let backup = Builder::new()
            .prefix(".tidas-process-bundles-backup-")
            .tempdir_in(parent)?;
        let previous = backup.path().join("previous");
        fs::rename(&self.target, &previous)?;
        if let Err(source) = fs::rename(self.staging.path(), &self.target) {
            let restore = fs::rename(&previous, &self.target);
            return match restore {
                Ok(()) => Err(ProcessBundleError::Io(source)),
                Err(restore) => Err(ProcessBundleError::CommitRollback { source, restore }),
            };
        }
        Ok(())
    }
}

#[derive(Debug, Error)]
pub enum ProcessBundleError {
    #[error("TIDAS package is not a directory: {0}")]
    InputNotDirectory(PathBuf),
    #[error("process bundle size overflow")]
    SizeOverflow,
    #[error(
        "failed to commit process bundles and restore previous output: commit={source}; restore={restore}"
    )]
    CommitRollback {
        source: std::io::Error,
        restore: std::io::Error,
    },
    #[error("process bundle I/O failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("process bundle JSON failed: {0}")]
    Json(#[from] serde_json::Error),
    #[error("process bundle traversal failed: {0}")]
    Walk(#[from] walkdir::Error),
    #[error("process bundle runtime failed: {0}")]
    Runtime(#[from] RuntimeError),
    #[error("process bundle canonical store failed: {0}")]
    Store(#[from] StoreError),
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;
    use tidas_runtime::{CancellationToken, MemoryBudget};
    use tidas_validation::{ValidationRequest, validate_tidas_package};

    use super::*;
    use crate::adapters::{AdapterContext, SimaProCsvAdapter, SourceAdapter};
    use crate::report::IssueSpool;
    use crate::writers::{TidasWriteRequest, write_tidas_package};

    #[test]
    fn bundle_materialization_uses_disk_dependencies_and_is_valid() {
        let directory = tempdir().unwrap();
        let source = directory.path().join("source.csv");
        fs::write(
            &source,
            b"{SimaPro 9.5}\n\nProcess\n\nProcess name\nSteel\n\nProducts\nSteel;kg;1\n\nEnd\n",
        )
        .unwrap();
        let cancellation = CancellationToken::default();
        let memory_budget = MemoryBudget::new(16 * 1024 * 1024);
        let mut store = CanonicalStore::create(Some(directory.path())).unwrap();
        let mut issues = IssueSpool::new(Vec::new(), 64 * 1024);
        SimaProCsvAdapter
            .read(
                &AdapterContext {
                    source: &source,
                    cancellation: &cancellation,
                    memory_budget: &memory_budget,
                    max_entry_bytes: 1024 * 1024,
                },
                &mut store,
                &mut issues,
            )
            .unwrap();
        issues.finish().unwrap();
        let tidas = directory.path().join("tidas");
        write_tidas_package(&TidasWriteRequest {
            store: &store,
            output_dir: &tidas,
            cancellation: &cancellation,
            memory_budget: &memory_budget,
        })
        .unwrap();
        let bundles = directory.path().join("bundles");
        let report = write_process_bundles(&ProcessBundleRequest {
            store: &store,
            tidas_dir: &tidas,
            output_dir: &bundles,
            cancellation: &cancellation,
            memory_budget: &memory_budget,
        })
        .unwrap();
        assert_eq!(report.process_count, 1);
        assert_eq!(report.unresolved_reference_count, 0);
        let process_id = store
            .iter_type("processes")
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .internal_id;
        let validation = validate_tidas_package(&ValidationRequest {
            input_dir: bundles.join(process_id).join("tidas"),
            issue_spool: None,
            cancellation,
            memory_budget,
            queue_capacity: 2,
            progress: None,
        })
        .unwrap();
        assert!(validation.summary.ok, "{:?}", validation.summary);
    }
}
