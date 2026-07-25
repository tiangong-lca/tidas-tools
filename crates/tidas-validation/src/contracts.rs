use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use tidas_runtime::SpoolSummaryV1;

pub const VALIDATION_ISSUE_EVENT_SCHEMA_V1: &str = "tidas.validation-issue-event.v1";
pub const VALIDATION_SUMMARY_SCHEMA_V1: &str = "tidas.validation-summary.v1";
pub const VALIDATION_ISSUE_EVENT_JSON_SCHEMA_V1: &str =
    include_str!("../../../contracts/validation-issue-event.v1.schema.json");
pub const VALIDATION_SUMMARY_JSON_SCHEMA_V1: &str =
    include_str!("../../../contracts/validation-summary.v1.schema.json");

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum SeverityV1 {
    Error,
    Warning,
    Info,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ValidationIssueV1 {
    pub issue_code: String,
    pub severity: SeverityV1,
    pub category: String,
    pub file_path: String,
    pub location: String,
    pub message: String,
    pub context: BTreeMap<String, serde_json::Value>,
}

impl ValidationIssueV1 {
    #[must_use]
    pub fn error(
        issue_code: impl Into<String>,
        category: impl Into<String>,
        file_path: impl Into<String>,
        location: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            issue_code: issue_code.into(),
            severity: SeverityV1::Error,
            category: category.into(),
            file_path: file_path.into(),
            location: location.into(),
            message: message.into(),
            context: BTreeMap::new(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ValidationIssueEventV1 {
    #[serde(rename = "type")]
    pub event_type: String,
    pub schema_version: String,
    pub issue_ordinal: u64,
    pub issue: ValidationIssueV1,
}

impl ValidationIssueEventV1 {
    #[must_use]
    pub fn new(issue_ordinal: u64, issue: ValidationIssueV1) -> Self {
        Self {
            event_type: "issue".to_owned(),
            schema_version: VALIDATION_ISSUE_EVENT_SCHEMA_V1.to_owned(),
            issue_ordinal,
            issue,
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CategorySummaryV1 {
    pub category: String,
    pub document_count: u64,
    pub issue_count: u64,
    pub error_count: u64,
    pub warning_count: u64,
    pub info_count: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ValidationSummaryV1 {
    pub schema_version: String,
    pub input_format: String,
    pub ok: bool,
    pub category_count: u64,
    pub document_count: u64,
    pub issue_count: u64,
    pub error_count: u64,
    pub warning_count: u64,
    pub info_count: u64,
    pub categories: Vec<CategorySummaryV1>,
    pub asset_fingerprint: String,
    pub issue_spool: Option<SpoolSummaryV1>,
    pub peak_accounted_memory_bytes: u64,
}

impl ValidationSummaryV1 {
    #[must_use]
    pub fn new(asset_fingerprint: String) -> Self {
        Self {
            schema_version: VALIDATION_SUMMARY_SCHEMA_V1.to_owned(),
            input_format: "tidas-json".to_owned(),
            ok: true,
            category_count: 0,
            document_count: 0,
            issue_count: 0,
            error_count: 0,
            warning_count: 0,
            info_count: 0,
            categories: Vec::new(),
            asset_fingerprint,
            issue_spool: None,
            peak_accounted_memory_bytes: 0,
        }
    }

    pub fn record_issue(&mut self, category: &mut CategorySummaryV1, severity: SeverityV1) {
        self.issue_count += 1;
        category.issue_count += 1;
        match severity {
            SeverityV1::Error => {
                self.error_count += 1;
                category.error_count += 1;
            }
            SeverityV1::Warning => {
                self.warning_count += 1;
                category.warning_count += 1;
            }
            SeverityV1::Info => {
                self.info_count += 1;
                category.info_count += 1;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checked_in_schemas_match_the_rust_contract_versions() {
        let issue_schema: serde_json::Value =
            serde_json::from_str(VALIDATION_ISSUE_EVENT_JSON_SCHEMA_V1).unwrap();
        let summary_schema: serde_json::Value =
            serde_json::from_str(VALIDATION_SUMMARY_JSON_SCHEMA_V1).unwrap();
        assert_eq!(
            issue_schema["properties"]["schema_version"]["const"],
            VALIDATION_ISSUE_EVENT_SCHEMA_V1
        );
        assert_eq!(
            summary_schema["properties"]["schema_version"]["const"],
            VALIDATION_SUMMARY_SCHEMA_V1
        );
        assert_eq!(issue_schema["additionalProperties"], false);
        assert_eq!(summary_schema["additionalProperties"], false);
    }
}
