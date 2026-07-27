//! Native, bounded external-LCA import pipeline.
//!
//! Source adapters write canonical entities to a disk-backed store. Writers
//! consume that store in deterministic order, so source records and mapping
//! rows never need to be retained as one in-memory package.

pub mod adapters;
pub mod bundles;
pub mod detect;
pub mod mapping;
pub mod model;
pub mod normalization;
pub mod orchestrator;
pub mod pipeline;
pub mod report;
pub mod source;
pub mod store;
pub mod writers;

pub use adapters::{
    AdapterContext, AdapterError, EcoSpold1Adapter, EcoSpold2Adapter, IlcdAdapter,
    OpenLcaJsonLdAdapter, OpenLcaProcessXlsxAdapter, SourceAdapter, adapter_for,
};
pub use bundles::{
    ProcessBundleError, ProcessBundleReportV1, ProcessBundleRequest, write_process_bundles,
};
pub use detect::{
    DetectedFormat, DetectionConfidence, DetectionError, DetectionRequest, SourceFormat,
    detect_format,
};
pub use mapping::{
    MAPPING_CSV_COLUMNS, MappingCsvError, MappingCsvReportV1, MappingCsvRequest,
    write_mapping_csv_gz,
};
pub use model::{CanonicalEntity, CanonicalExchange, EntityRef};
pub use normalization::{
    CanonicalClassification, CanonicalFlow, CanonicalFlowName, CanonicalFlowPropertyAssignment,
    FlowDatasetType, FlowNormalizationError, normalize_flow,
};
pub use orchestrator::{
    IMPORT_EXECUTION_REPORT_JSON_SCHEMA_V1, IMPORT_EXECUTION_REPORT_SCHEMA_V1,
    ImportExecutionError, ImportExecutionReportV1, ImportRequest, ImportTarget, run_import,
};
pub use pipeline::{ImportCoreError, ImportCoreOutput, ImportCoreRequest, parse_external_source};
pub use report::{IMPORT_REPORT_SCHEMA_V1, ImportIssue, ImportReportV1, IssueSeverity};
pub use source::{
    SourceEntry, SourceReadError, SourceReadRequest, SourceReadSummary, visit_source_entries,
};
pub use store::{CanonicalStore, StoreError};
pub use writers::{
    IMPORT_PACKAGE_REPORT_JSON_SCHEMA_V1, IMPORT_PACKAGE_REPORT_SCHEMA_V1, IlcdWriteReportV1,
    IlcdWriteRequest, PackageWriteError, PackageWriteReportV1, TidasWriteRequest,
    write_ilcd_package, write_tidas_package,
};
