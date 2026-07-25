//! Native, bounded TIDAS and ILCD validation pipelines.

mod contracts;
mod pipeline;
mod schema;

pub use contracts::{
    CategorySummaryV1, SeverityV1, VALIDATION_ISSUE_EVENT_JSON_SCHEMA_V1,
    VALIDATION_ISSUE_EVENT_SCHEMA_V1, VALIDATION_SUMMARY_JSON_SCHEMA_V1,
    VALIDATION_SUMMARY_SCHEMA_V1, ValidationIssueEventV1, ValidationIssueV1, ValidationSummaryV1,
};
pub use pipeline::{ValidationError, ValidationOutput, ValidationRequest, validate_tidas_package};
pub use schema::{SUPPORTED_TIDAS_CATEGORIES, SchemaError, TidasCategory, is_valid_cas_number};
