use std::collections::BTreeMap;
use std::io::Write;

use serde::{Deserialize, Serialize};
use thiserror::Error;
use tidas_runtime::{JsonlSpool, RuntimeError, SpoolSummaryV1};

use crate::detect::SourceFormat;

pub const IMPORT_REPORT_SCHEMA_V1: &str = "tidas.import-report.v1";

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum IssueSeverity {
    Note,
    Warning,
    Error,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ImportIssue {
    pub severity: IssueSeverity,
    pub code: String,
    pub message: String,
    pub source_object: Option<String>,
    #[serde(default)]
    pub context: BTreeMap<String, serde_json::Value>,
}

pub trait IssueSink {
    fn push(&mut self, issue: &ImportIssue) -> Result<(), IssueSinkError>;
}

pub struct IssueSpool<W: Write> {
    spool: JsonlSpool<W>,
    issue_count: u64,
    warning_count: u64,
    error_count: u64,
}

impl<W: Write> IssueSpool<W> {
    #[must_use]
    pub fn new(writer: W, max_event_bytes: usize) -> Self {
        Self {
            spool: JsonlSpool::new(writer, max_event_bytes),
            issue_count: 0,
            warning_count: 0,
            error_count: 0,
        }
    }

    pub fn finish(self) -> Result<(W, IssueSpoolSummary), IssueSinkError> {
        let (writer, spool) = self.spool.finish()?;
        Ok((
            writer,
            IssueSpoolSummary {
                issue_count: self.issue_count,
                warning_count: self.warning_count,
                error_count: self.error_count,
                spool,
            },
        ))
    }
}

impl<W: Write> IssueSink for IssueSpool<W> {
    fn push(&mut self, issue: &ImportIssue) -> Result<(), IssueSinkError> {
        self.spool.push(issue)?;
        self.issue_count = self.issue_count.saturating_add(1);
        match issue.severity {
            IssueSeverity::Note => {}
            IssueSeverity::Warning => {
                self.warning_count = self.warning_count.saturating_add(1);
            }
            IssueSeverity::Error => {
                self.error_count = self.error_count.saturating_add(1);
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct IssueSpoolSummary {
    pub issue_count: u64,
    pub warning_count: u64,
    pub error_count: u64,
    pub spool: SpoolSummaryV1,
}

#[derive(Debug, Error)]
pub enum IssueSinkError {
    #[error("issue spool failed: {0}")]
    Runtime(#[from] RuntimeError),
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ImportReportV1 {
    pub schema_version: String,
    pub source_path: String,
    pub detected_format: SourceFormat,
    pub detection_evidence: Vec<String>,
    pub target: String,
    pub object_counts: BTreeMap<String, u64>,
    pub warning_count: u64,
    pub error_count: u64,
    pub issues_spooled: u64,
    pub output_tree_sha256: Option<String>,
    pub output_bytes: Option<u64>,
    pub peak_accounted_memory_bytes: u64,
}

impl ImportReportV1 {
    #[must_use]
    pub fn new(
        source_path: impl Into<String>,
        detected_format: SourceFormat,
        detection_evidence: Vec<String>,
    ) -> Self {
        Self {
            schema_version: IMPORT_REPORT_SCHEMA_V1.to_owned(),
            source_path: source_path.into(),
            detected_format,
            detection_evidence,
            target: "tidas".to_owned(),
            object_counts: BTreeMap::new(),
            warning_count: 0,
            error_count: 0,
            issues_spooled: 0,
            output_tree_sha256: None,
            output_bytes: None,
            peak_accounted_memory_bytes: 0,
        }
    }
}
