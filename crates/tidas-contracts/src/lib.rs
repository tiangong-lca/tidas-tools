//! Stable, versioned contracts shared by the `tidas` CLI and embedders.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use thiserror::Error;

pub const OPERATION_REPORT_SCHEMA_V1: &str = "tidas.operation-report.v1";
pub const DIAGNOSTIC_SCHEMA_V1: &str = "tidas.diagnostic.v1";
pub const OPERATION_REPORT_JSON_SCHEMA_V1: &str =
    include_str!("../../../contracts/operation-report.v1.schema.json");

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ExitClass {
    Success,
    DataIssues,
    Usage,
    Unavailable,
    Internal,
    Io,
    Cancelled,
}

impl ExitClass {
    #[must_use]
    pub const fn code(self) -> u8 {
        match self {
            Self::Success => 0,
            Self::DataIssues => 2,
            Self::Usage => 64,
            Self::Unavailable => 69,
            Self::Internal => 70,
            Self::Io => 74,
            Self::Cancelled => 130,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum OperationStatus {
    Succeeded,
    CompletedWithIssues,
    Failed,
    Cancelled,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Completeness {
    Complete,
    Partial,
    NotStarted,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ArtifactRefV1 {
    pub path: String,
    pub media_type: String,
    pub sha256: Option<String>,
    pub bytes: Option<u64>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DiagnosticV1 {
    pub schema_version: String,
    pub code: String,
    pub message: String,
    pub path: Option<String>,
    pub details: BTreeMap<String, String>,
}

impl DiagnosticV1 {
    #[must_use]
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            schema_version: DIAGNOSTIC_SCHEMA_V1.to_owned(),
            code: code.into(),
            message: message.into(),
            path: None,
            details: BTreeMap::new(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct OperationReportV1 {
    pub schema_version: String,
    pub command: String,
    pub status: OperationStatus,
    pub exit_class: ExitClass,
    pub completeness: Completeness,
    pub summary: BTreeMap<String, serde_json::Value>,
    pub diagnostics: Vec<DiagnosticV1>,
    pub artifacts: Vec<ArtifactRefV1>,
    pub next_actions: Vec<String>,
}

impl OperationReportV1 {
    #[must_use]
    pub fn succeeded(command: impl Into<String>) -> Self {
        Self {
            schema_version: OPERATION_REPORT_SCHEMA_V1.to_owned(),
            command: command.into(),
            status: OperationStatus::Succeeded,
            exit_class: ExitClass::Success,
            completeness: Completeness::Complete,
            summary: BTreeMap::new(),
            diagnostics: Vec::new(),
            artifacts: Vec::new(),
            next_actions: Vec::new(),
        }
    }

    #[must_use]
    pub fn unavailable(command: impl Into<String>, next_action: impl Into<String>) -> Self {
        let mut report = Self::succeeded(command);
        report.status = OperationStatus::Failed;
        report.exit_class = ExitClass::Unavailable;
        report.completeness = Completeness::NotStarted;
        report.diagnostics.push(DiagnosticV1::new(
            "feature_not_migrated",
            "This Rust command is not implemented yet.",
        ));
        report.next_actions.push(next_action.into());
        report
    }

    pub fn to_canonical_json_line(&self) -> Result<Vec<u8>, ContractError> {
        let mut bytes = serde_json::to_vec(self)?;
        bytes.push(b'\n');
        Ok(bytes)
    }
}

#[derive(Debug, Error)]
pub enum ContractError {
    #[error("failed to serialize a versioned contract: {0}")]
    Serialize(#[from] serde_json::Error),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exit_codes_are_stable_and_distinct() {
        let classes = [
            ExitClass::Success,
            ExitClass::DataIssues,
            ExitClass::Usage,
            ExitClass::Unavailable,
            ExitClass::Internal,
            ExitClass::Io,
            ExitClass::Cancelled,
        ];
        let mut codes = classes.map(ExitClass::code).to_vec();
        codes.sort_unstable();
        codes.dedup();
        assert_eq!(codes.len(), classes.len());
        assert_eq!(ExitClass::Success.code(), 0);
        assert_eq!(ExitClass::DataIssues.code(), 2);
        assert_eq!(ExitClass::Cancelled.code(), 130);
    }

    #[test]
    fn canonical_json_is_repeatable_and_lf_terminated() {
        let mut report = OperationReportV1::succeeded("version");
        report.summary.insert("b".to_owned(), serde_json::json!(2));
        report.summary.insert("a".to_owned(), serde_json::json!(1));

        let first = report.to_canonical_json_line().unwrap();
        let second = report.to_canonical_json_line().unwrap();

        assert_eq!(first, second);
        assert!(first.ends_with(b"\n"));
        let text = String::from_utf8(first).unwrap();
        assert!(text.find("\"a\"").unwrap() < text.find("\"b\"").unwrap());
    }

    #[test]
    fn unknown_fields_are_rejected() {
        let value = serde_json::json!({
            "schema_version": OPERATION_REPORT_SCHEMA_V1,
            "command": "version",
            "status": "succeeded",
            "exit_class": "success",
            "completeness": "complete",
            "summary": {},
            "diagnostics": [],
            "artifacts": [],
            "next_actions": [],
            "unexpected": true
        });
        assert!(serde_json::from_value::<OperationReportV1>(value).is_err());
    }

    #[test]
    fn checked_in_json_schema_matches_the_rust_contract_version() {
        let schema: serde_json::Value =
            serde_json::from_str(OPERATION_REPORT_JSON_SCHEMA_V1).unwrap();
        assert_eq!(
            schema["properties"]["schema_version"]["const"],
            OPERATION_REPORT_SCHEMA_V1
        );
        assert_eq!(
            schema["$defs"]["diagnostic"]["properties"]["schema_version"]["const"],
            DIAGNOSTIC_SCHEMA_V1
        );
        assert_eq!(schema["additionalProperties"], false);
    }
}
