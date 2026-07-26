use std::io::Write;
use std::path::Path;

use thiserror::Error;
use tidas_runtime::{CancellationToken, MemoryBudget, RuntimeError};

use crate::adapters::{AdapterContext, AdapterError, adapter_for};
use crate::detect::{DetectionError, DetectionRequest, SourceFormat, detect_format};
use crate::report::{ImportReportV1, IssueSinkError, IssueSpool, IssueSpoolSummary};
use crate::store::{CanonicalStore, StoreError};

pub struct ImportCoreRequest<'a> {
    pub source: &'a Path,
    pub requested_format: Option<SourceFormat>,
    pub spool_parent: Option<&'a Path>,
    pub cancellation: &'a CancellationToken,
    pub memory_budget: &'a MemoryBudget,
    pub max_entry_bytes: u64,
    pub max_issue_bytes: usize,
}

pub struct ImportCoreOutput<W> {
    pub store: CanonicalStore,
    pub report: ImportReportV1,
    pub issue_writer: W,
    pub issue_spool: IssueSpoolSummary,
}

pub fn parse_external_source<W: Write>(
    request: &ImportCoreRequest<'_>,
    issue_writer: W,
) -> Result<ImportCoreOutput<W>, ImportCoreError> {
    request.cancellation.check()?;
    if request.max_issue_bytes == 0 {
        return Err(ImportCoreError::ZeroIssueLimit);
    }
    let detected = detect_format(&DetectionRequest {
        source: request.source.to_path_buf(),
        requested_format: request.requested_format,
    })?;
    match detected.format {
        SourceFormat::UnsupportedZolca => {
            return Err(ImportCoreError::ZolcaUnsupported);
        }
        SourceFormat::Unknown => {
            return Err(ImportCoreError::UnknownFormat);
        }
        _ => {}
    }
    let adapter =
        adapter_for(detected.format).ok_or(ImportCoreError::AdapterUnavailable(detected.format))?;
    let mut store = CanonicalStore::create(request.spool_parent)?;
    let mut issues = IssueSpool::new(issue_writer, request.max_issue_bytes);
    adapter.read(
        &AdapterContext {
            source: request.source,
            cancellation: request.cancellation,
            memory_budget: request.memory_budget,
            max_entry_bytes: request.max_entry_bytes,
        },
        &mut store,
        &mut issues,
    )?;
    let (issue_writer, issue_spool) = issues.finish()?;
    let mut report = ImportReportV1::new(
        request.source.to_string_lossy(),
        detected.format,
        detected.evidence,
    );
    report.object_counts.clone_from(store.counts());
    report.warning_count = issue_spool.warning_count;
    report.error_count = issue_spool.error_count;
    report.issues_spooled = issue_spool.issue_count;
    report.peak_accounted_memory_bytes = request.memory_budget.peak();
    Ok(ImportCoreOutput {
        store,
        report,
        issue_writer,
        issue_spool,
    })
}

#[derive(Debug, Error)]
pub enum ImportCoreError {
    #[error("openLCA .zolca databases are intentionally unsupported")]
    ZolcaUnsupported,
    #[error("source format could not be detected")]
    UnknownFormat,
    #[error("Rust adapter for {0:?} is not available")]
    AdapterUnavailable(SourceFormat),
    #[error("maximum issue event size must be greater than zero")]
    ZeroIssueLimit,
    #[error("import runtime failed: {0}")]
    Runtime(#[from] RuntimeError),
    #[error("import format detection failed: {0}")]
    Detection(#[from] DetectionError),
    #[error("import canonical store failed: {0}")]
    Store(#[from] StoreError),
    #[error("import source adapter failed: {0}")]
    Adapter(#[from] AdapterError),
    #[error("import issue spool failed: {0}")]
    Issue(#[from] IssueSinkError),
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use super::*;

    fn controls() -> (CancellationToken, MemoryBudget) {
        (
            CancellationToken::default(),
            MemoryBudget::new(4 * 1024 * 1024),
        )
    }

    #[test]
    fn core_pipeline_detects_parses_spools_and_reports_without_python() {
        let directory = tempdir().unwrap();
        let source = directory.path().join("source.csv");
        std::fs::write(
            &source,
            b"{SimaPro 9.5}\n\nProcess\n\nProcess name\nSteel\n\nProducts\nSteel;kg;1\n\nEnd\n",
        )
        .unwrap();
        let (cancellation, memory_budget) = controls();
        let output = parse_external_source(
            &ImportCoreRequest {
                source: &source,
                requested_format: None,
                spool_parent: Some(directory.path()),
                cancellation: &cancellation,
                memory_budget: &memory_budget,
                max_entry_bytes: 1024 * 1024,
                max_issue_bytes: 64 * 1024,
            },
            Vec::new(),
        )
        .unwrap();
        assert_eq!(output.report.detected_format, SourceFormat::SimaproCsv);
        assert_eq!(output.report.object_counts["processes"], 1);
        assert_eq!(output.report.object_counts["flows"], 1);
        assert_eq!(output.report.warning_count, 1);
        assert_eq!(output.report.error_count, 0);
        assert_eq!(output.issue_spool.issue_count, 1);
        assert!(!output.issue_writer.is_empty());
        assert_eq!(memory_budget.used(), 0);
    }

    #[test]
    fn unsupported_zolca_fails_and_openlca_uses_the_native_adapter() {
        let directory = tempdir().unwrap();
        let zolca = directory.path().join("database.zolca");
        std::fs::write(&zolca, b"SQLite format 3").unwrap();
        let (cancellation, memory_budget) = controls();
        let request = |source| ImportCoreRequest {
            source,
            requested_format: None,
            spool_parent: Some(directory.path()),
            cancellation: &cancellation,
            memory_budget: &memory_budget,
            max_entry_bytes: 1024,
            max_issue_bytes: 1024,
        };
        assert!(matches!(
            parse_external_source(&request(&zolca), Vec::new()),
            Err(ImportCoreError::ZolcaUnsupported)
        ));

        let json = directory.path().join("process.json");
        std::fs::write(&json, br#"{"@type":"Process","@id":"p"}"#).unwrap();
        let output = parse_external_source(&request(&json), Vec::new()).unwrap();
        assert_eq!(output.report.detected_format, SourceFormat::OpenlcaJsonld);
        assert_eq!(output.report.object_counts["processes"], 1);
    }

    #[test]
    fn cancellation_prevents_detection_and_adapter_work() {
        let directory = tempdir().unwrap();
        let source = directory.path().join("source.csv");
        std::fs::write(&source, b"{SimaPro 9.5}\n").unwrap();
        let (cancellation, memory_budget) = controls();
        cancellation.cancel();
        assert!(matches!(
            parse_external_source(
                &ImportCoreRequest {
                    source: &source,
                    requested_format: None,
                    spool_parent: None,
                    cancellation: &cancellation,
                    memory_budget: &memory_budget,
                    max_entry_bytes: 1024,
                    max_issue_bytes: 1024,
                },
                Vec::new(),
            ),
            Err(ImportCoreError::Runtime(RuntimeError::Cancelled))
        ));
    }
}
