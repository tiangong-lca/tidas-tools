use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tempfile::{Builder, TempDir};
use thiserror::Error;
use tidas_conversion::{
    ConversionDirection, ConversionError, ConversionReportV1, ConversionRequest, convert_directory,
};
use tidas_runtime::{CancellationToken, MemoryBudget, RuntimeError};
use tidas_validation::{
    ValidationError, ValidationRequest, validate_ilcd_package, validate_tidas_package,
};

use crate::bundles::{
    ProcessBundleError, ProcessBundleReportV1, ProcessBundleRequest, write_process_bundles,
};
use crate::detect::SourceFormat;
use crate::mapping::{
    MappingCsvError, MappingCsvReportV1, MappingCsvRequest, write_mapping_csv_gz,
};
use crate::pipeline::{ImportCoreError, ImportCoreRequest, parse_external_source};
use crate::writers::{
    PackageWriteError, PackageWriteReportV1, TidasWriteRequest, write_tidas_package,
};

pub const IMPORT_EXECUTION_REPORT_SCHEMA_V1: &str = "tidas.import-execution-report.v1";
pub const IMPORT_EXECUTION_REPORT_JSON_SCHEMA_V1: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/contracts/import-execution-report.v1.schema.json"
));

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ImportTarget {
    Tidas,
    Ilcd,
    Both,
}

impl ImportTarget {
    fn includes_ilcd(self) -> bool {
        matches!(self, Self::Ilcd | Self::Both)
    }

    fn keeps_tidas(self) -> bool {
        matches!(self, Self::Tidas | Self::Both)
    }
}

pub struct ImportRequest {
    pub source: PathBuf,
    pub requested_format: Option<SourceFormat>,
    pub output_dir: PathBuf,
    pub target: ImportTarget,
    pub write_mapping: bool,
    pub write_process_bundles: bool,
    pub cancellation: CancellationToken,
    pub memory_budget: MemoryBudget,
    pub queue_capacity: usize,
    pub max_entry_bytes: u64,
    pub max_issue_bytes: usize,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ImportExecutionReportV1 {
    pub schema_version: String,
    pub source_path: String,
    pub detected_format: SourceFormat,
    pub detection_evidence: Vec<String>,
    pub target: ImportTarget,
    pub object_counts: BTreeMap<String, u64>,
    pub warning_count: u64,
    pub error_count: u64,
    pub issues_spooled: u64,
    pub issues_file: String,
    pub tidas_package: PackageWriteReportV1,
    pub ilcd_conversion: Option<ConversionReportV1>,
    pub mapping: Option<MappingCsvReportV1>,
    pub process_bundles: Option<ProcessBundleReportV1>,
    pub tidas_validation_issue_count: u64,
    pub ilcd_validation_issue_count: Option<u64>,
    pub peak_accounted_memory_bytes: u64,
}

pub fn run_import(
    request: &ImportRequest,
) -> Result<ImportExecutionReportV1, ImportExecutionError> {
    request.cancellation.check()?;
    if request.queue_capacity == 0 {
        return Err(ImportExecutionError::ZeroQueueCapacity);
    }
    reject_nested_output(&request.source, &request.output_dir)?;
    let staging = StagedDirectory::new(&request.output_dir)?;
    let issue_writer = BufWriter::new(File::create(staging.path().join("issues.jsonl"))?);
    let core = parse_external_source(
        &ImportCoreRequest {
            source: &request.source,
            requested_format: request.requested_format,
            spool_parent: staging.path().parent(),
            cancellation: &request.cancellation,
            memory_budget: &request.memory_budget,
            max_entry_bytes: request.max_entry_bytes,
            max_issue_bytes: request.max_issue_bytes,
        },
        issue_writer,
    )?;
    let crate::pipeline::ImportCoreOutput {
        store,
        report: core_report,
        issue_writer,
        issue_spool,
    } = core;
    drop(issue_writer);
    if issue_spool.error_count > 0 {
        return Err(ImportExecutionError::SourceIssues {
            count: issue_spool.error_count,
        });
    }

    let tidas_dir = staging.path().join("tidas");
    let tidas_package = write_tidas_package(&TidasWriteRequest {
        store: &store,
        output_dir: &tidas_dir,
        cancellation: &request.cancellation,
        memory_budget: &request.memory_budget,
    })?;
    let tidas_validation = validate_tidas_package(&validation_request(
        &tidas_dir,
        staging.path().join("tidas-validation-issues.jsonl"),
        request,
    ))?;
    ensure_valid("tidas", &tidas_validation.summary)?;

    let ilcd = write_and_validate_ilcd(request, staging.path(), &tidas_dir)?;
    let mapping = request
        .write_mapping
        .then(|| {
            write_mapping_csv_gz(&MappingCsvRequest {
                store: &store,
                output_path: &staging.path().join("mapping.csv.gz"),
                source_format: core_report.detected_format.as_str(),
                cancellation: &request.cancellation,
                memory_budget: &request.memory_budget,
            })
        })
        .transpose()?;
    let process_bundles = request
        .write_process_bundles
        .then(|| {
            write_process_bundles(&ProcessBundleRequest {
                store: &store,
                tidas_dir: &tidas_dir,
                output_dir: &staging.path().join("process-bundles"),
                cancellation: &request.cancellation,
                memory_budget: &request.memory_budget,
            })
        })
        .transpose()?;
    let report = ImportExecutionReportV1 {
        schema_version: IMPORT_EXECUTION_REPORT_SCHEMA_V1.to_owned(),
        source_path: request.source.to_string_lossy().into_owned(),
        detected_format: core_report.detected_format,
        detection_evidence: core_report.detection_evidence,
        target: request.target,
        object_counts: core_report.object_counts,
        warning_count: issue_spool.warning_count,
        error_count: issue_spool.error_count,
        issues_spooled: issue_spool.issue_count,
        issues_file: "issues.jsonl".to_owned(),
        tidas_package,
        ilcd_conversion: ilcd.as_ref().map(|value| value.0.clone()),
        mapping,
        process_bundles,
        tidas_validation_issue_count: tidas_validation.summary.issue_count,
        ilcd_validation_issue_count: ilcd.as_ref().map(|value| value.1),
        peak_accounted_memory_bytes: request.memory_budget.peak(),
    };
    write_report(staging.path(), &report)?;
    if !request.target.keeps_tidas() {
        fs::remove_dir_all(tidas_dir)?;
    }
    request.cancellation.check()?;
    staging.commit()?;
    Ok(report)
}

fn write_and_validate_ilcd(
    request: &ImportRequest,
    root: &Path,
    tidas_dir: &Path,
) -> Result<Option<(ConversionReportV1, u64)>, ImportExecutionError> {
    if !request.target.includes_ilcd() {
        return Ok(None);
    }
    let ilcd_dir = root.join("ilcd");
    let conversion = convert_directory(&ConversionRequest {
        input_dir: tidas_dir.to_path_buf(),
        output_dir: ilcd_dir.clone(),
        direction: ConversionDirection::TidasToIlcd,
        cancellation: request.cancellation.clone(),
        memory_budget: request.memory_budget.clone(),
        queue_capacity: request.queue_capacity,
        progress: None,
    })?;
    let validation = validate_ilcd_package(&validation_request(
        &ilcd_dir,
        root.join("ilcd-validation-issues.jsonl"),
        request,
    ))?;
    ensure_valid("ilcd", &validation.summary)?;
    Ok(Some((conversion, validation.summary.issue_count)))
}

fn validation_request(
    input_dir: &Path,
    issue_spool: PathBuf,
    request: &ImportRequest,
) -> ValidationRequest {
    ValidationRequest {
        input_dir: input_dir.to_path_buf(),
        issue_spool: Some(issue_spool),
        cancellation: request.cancellation.clone(),
        memory_budget: request.memory_budget.clone(),
        queue_capacity: request.queue_capacity,
        progress: None,
    }
}

fn ensure_valid(
    format: &'static str,
    summary: &tidas_validation::ValidationSummaryV1,
) -> Result<(), ImportExecutionError> {
    if summary.ok {
        Ok(())
    } else {
        Err(ImportExecutionError::GeneratedPackageInvalid {
            format,
            issues: summary.issue_count,
        })
    }
}

fn write_report(root: &Path, report: &ImportExecutionReportV1) -> Result<(), ImportExecutionError> {
    let mut writer = BufWriter::new(File::create(root.join("import-report.json"))?);
    serde_json::to_writer_pretty(&mut writer, report)?;
    writer.write_all(b"\n")?;
    writer.flush()?;
    Ok(())
}

fn reject_nested_output(source: &Path, output: &Path) -> Result<(), ImportExecutionError> {
    if source.is_dir() && output.starts_with(source) {
        return Err(ImportExecutionError::OutputNestedInSource(
            output.to_path_buf(),
        ));
    }
    Ok(())
}

struct StagedDirectory {
    target: PathBuf,
    staging: TempDir,
}

impl StagedDirectory {
    fn new(target: &Path) -> Result<Self, ImportExecutionError> {
        let parent = target.parent().unwrap_or_else(|| Path::new("."));
        fs::create_dir_all(parent)?;
        Ok(Self {
            target: target.to_path_buf(),
            staging: Builder::new()
                .prefix(".tidas-import-execution-")
                .tempdir_in(parent)?,
        })
    }

    fn path(&self) -> &Path {
        self.staging.path()
    }

    fn commit(self) -> Result<(), ImportExecutionError> {
        let parent = self.target.parent().unwrap_or_else(|| Path::new("."));
        if !self.target.exists() {
            fs::rename(self.staging.path(), self.target)?;
            return Ok(());
        }
        let backup = Builder::new()
            .prefix(".tidas-import-execution-backup-")
            .tempdir_in(parent)?;
        let previous = backup.path().join("previous");
        fs::rename(&self.target, &previous)?;
        if let Err(source) = fs::rename(self.staging.path(), &self.target) {
            let restore = fs::rename(&previous, &self.target);
            return match restore {
                Ok(()) => Err(ImportExecutionError::Io(source)),
                Err(restore) => Err(ImportExecutionError::CommitRollback { source, restore }),
            };
        }
        Ok(())
    }
}

#[derive(Debug, Error)]
pub enum ImportExecutionError {
    #[error("queue capacity must be greater than zero")]
    ZeroQueueCapacity,
    #[error("import output cannot be nested in the source directory: {0}")]
    OutputNestedInSource(PathBuf),
    #[error("generated {format} package failed validation with {issues} issues")]
    GeneratedPackageInvalid { format: &'static str, issues: u64 },
    #[error("import source produced {count} error issue(s) before package generation")]
    SourceIssues { count: u64 },
    #[error(
        "failed to commit import output and restore previous output: commit={source}; restore={restore}"
    )]
    CommitRollback {
        source: std::io::Error,
        restore: std::io::Error,
    },
    #[error("import execution I/O failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("import execution JSON failed: {0}")]
    Json(#[from] serde_json::Error),
    #[error("import execution runtime failed: {0}")]
    Runtime(#[from] RuntimeError),
    #[error("import core failed: {0}")]
    Core(#[from] ImportCoreError),
    #[error("TIDAS package write failed: {0}")]
    Package(#[from] PackageWriteError),
    #[error("ILCD conversion failed: {0}")]
    Conversion(#[from] ConversionError),
    #[error("package validation failed: {0}")]
    Validation(#[from] ValidationError),
    #[error("mapping CSV failed: {0}")]
    Mapping(#[from] MappingCsvError),
    #[error("process bundles failed: {0}")]
    Bundles(#[from] ProcessBundleError),
}

#[cfg(test)]
mod tests {
    use jsonschema::validator_for;
    use tempfile::tempdir;

    use super::*;

    #[test]
    fn complete_import_is_atomic_valid_and_contract_conformant() {
        let directory = tempdir().unwrap();
        let source = directory.path().join("source.csv");
        fs::write(
            &source,
            b"{SimaPro 9.5}\n\nProcess\n\nProcess name\nSteel\n\nProducts\nSteel | production route | GLO;kg;1\n\nEnd\n",
        )
        .unwrap();
        let output = directory.path().join("output");
        let report = run_import(&ImportRequest {
            source,
            requested_format: None,
            output_dir: output.clone(),
            target: ImportTarget::Both,
            write_mapping: true,
            write_process_bundles: true,
            cancellation: CancellationToken::default(),
            memory_budget: MemoryBudget::new(32 * 1024 * 1024),
            queue_capacity: 2,
            max_entry_bytes: 1024 * 1024,
            max_issue_bytes: 64 * 1024,
        })
        .unwrap();
        assert_eq!(report.detected_format, SourceFormat::SimaproCsv);
        assert!(output.join("tidas/processes").is_dir());
        assert!(output.join("ilcd/data/processes").is_dir());
        assert!(output.join("mapping.csv.gz").is_file());
        assert!(output.join("process-bundles/index.json").is_file());
        let report_value: serde_json::Value =
            serde_json::from_slice(&fs::read(output.join("import-report.json")).unwrap()).unwrap();
        let schema: serde_json::Value =
            serde_json::from_str(IMPORT_EXECUTION_REPORT_JSON_SCHEMA_V1).unwrap();
        assert!(validator_for(&schema).unwrap().is_valid(&report_value));
    }
}
