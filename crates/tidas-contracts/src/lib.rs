//! Stable, versioned contracts shared by the `tidas` CLI and embedders.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use thiserror::Error;

pub const OPERATION_REPORT_SCHEMA_V1: &str = "tidas.operation-report.v1";
pub const DIAGNOSTIC_SCHEMA_V1: &str = "tidas.diagnostic.v1";
pub const INVOCATION_CONTEXT_SCHEMA_V1: &str = "tidas.invocation-context.v1";
pub const OPERATION_REPORT_JSON_SCHEMA_V1: &str =
    include_str!("../../../contracts/operation-report.v1.schema.json");

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum CommandNameV1 {
    Convert,
    Import,
    Export,
    Validate,
    Release,
    Ruleset,
    Version,
}

impl CommandNameV1 {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Convert => "convert",
            Self::Import => "import",
            Self::Export => "export",
            Self::Validate => "validate",
            Self::Release => "release",
            Self::Ruleset => "ruleset",
            Self::Version => "version",
        }
    }
}

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

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ConfigSourceV1 {
    None,
    Environment,
    Cli,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum LogLevelV1 {
    Error,
    Warn,
    Info,
    Debug,
    Trace,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ProgressModeV1 {
    Auto,
    Never,
    Always,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ReportDestinationV1 {
    Stdout,
    File,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum InputPolicyV1 {
    ExplicitPathOrDash,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum DiagnosticDestinationV1 {
    Stderr,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct InvocationContextV1 {
    pub schema_version: String,
    pub config_source: ConfigSourceV1,
    pub config_path: Option<String>,
    pub log_level: LogLevelV1,
    pub progress_mode: ProgressModeV1,
    pub progress_enabled: bool,
    pub memory_budget_bytes: u64,
    pub queue_capacity: usize,
    pub input_policy: InputPolicyV1,
    pub report_destination: ReportDestinationV1,
    pub diagnostic_destination: DiagnosticDestinationV1,
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
    pub command: CommandNameV1,
    pub status: OperationStatus,
    pub exit_class: ExitClass,
    pub completeness: Completeness,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub invocation: Option<InvocationContextV1>,
    pub summary: BTreeMap<String, serde_json::Value>,
    pub diagnostics: Vec<DiagnosticV1>,
    pub artifacts: Vec<ArtifactRefV1>,
    pub next_actions: Vec<String>,
}

impl OperationReportV1 {
    #[must_use]
    pub fn succeeded(command: CommandNameV1) -> Self {
        Self {
            schema_version: OPERATION_REPORT_SCHEMA_V1.to_owned(),
            command,
            status: OperationStatus::Succeeded,
            exit_class: ExitClass::Success,
            completeness: Completeness::Complete,
            invocation: None,
            summary: BTreeMap::new(),
            diagnostics: Vec::new(),
            artifacts: Vec::new(),
            next_actions: Vec::new(),
        }
    }

    #[must_use]
    pub fn unavailable(command: CommandNameV1, next_action: impl Into<String>) -> Self {
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

    #[must_use]
    pub fn completed_with_issues(command: CommandNameV1, diagnostic: DiagnosticV1) -> Self {
        let mut report = Self::succeeded(command);
        report.status = OperationStatus::CompletedWithIssues;
        report.exit_class = ExitClass::DataIssues;
        report.diagnostics.push(diagnostic);
        report
    }

    #[must_use]
    pub fn failed(
        command: CommandNameV1,
        exit_class: ExitClass,
        code: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        let mut report = Self::succeeded(command);
        report.status = OperationStatus::Failed;
        report.exit_class = exit_class;
        report.completeness = Completeness::NotStarted;
        report.diagnostics.push(DiagnosticV1::new(code, message));
        report
    }

    #[must_use]
    pub fn cancelled(command: CommandNameV1) -> Self {
        let mut report = Self::failed(
            command,
            ExitClass::Cancelled,
            "operation_cancelled",
            "The operation was cancelled before completion.",
        );
        report.status = OperationStatus::Cancelled;
        report.completeness = Completeness::Partial;
        report
    }

    #[must_use]
    pub fn with_invocation(mut self, invocation: InvocationContextV1) -> Self {
        self.invocation = Some(invocation);
        self
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
        let mut report = OperationReportV1::succeeded(CommandNameV1::Version);
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
            "invocation": null,
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
        assert_eq!(
            schema["$defs"]["invocation"]["properties"]["schema_version"]["const"],
            INVOCATION_CONTEXT_SCHEMA_V1
        );
        assert_eq!(
            schema["properties"]["command"]["enum"]
                .as_array()
                .map(Vec::len),
            Some(7)
        );
        assert!(
            !schema["required"]
                .as_array()
                .unwrap()
                .contains(&serde_json::json!("invocation")),
            "invocation remains additive for pre-#119 v1 embedders"
        );
        assert_eq!(schema["additionalProperties"], false);
    }

    #[test]
    fn command_names_are_the_seven_product_commands() {
        let names = [
            CommandNameV1::Convert,
            CommandNameV1::Import,
            CommandNameV1::Export,
            CommandNameV1::Validate,
            CommandNameV1::Release,
            CommandNameV1::Ruleset,
            CommandNameV1::Version,
        ]
        .map(CommandNameV1::as_str);
        assert_eq!(
            names,
            [
                "convert", "import", "export", "validate", "release", "ruleset", "version"
            ]
        );
    }

    #[test]
    fn completed_with_issues_is_complete_but_nonzero() {
        let report = OperationReportV1::completed_with_issues(
            CommandNameV1::Validate,
            DiagnosticV1::new("fixture_warning", "The oracle reported a data issue."),
        );
        assert_eq!(report.status, OperationStatus::CompletedWithIssues);
        assert_eq!(report.exit_class, ExitClass::DataIssues);
        assert_eq!(report.completeness, Completeness::Complete);
        assert_eq!(report.exit_class.code(), 2);
    }
}
