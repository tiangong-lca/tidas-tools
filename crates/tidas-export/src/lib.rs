//! Native, bounded `PostgreSQL` and S3-compatible TIDAS package export.

mod archive;
mod database;
mod storage;
mod versioning;

use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tempfile::Builder;
use thiserror::Error;
use tidas_conversion::{ConversionError, convert_json_to_xml};
use tidas_runtime::{CancellationToken, MemoryBudget, RuntimeError};

pub use versioning::{VersionNormalizationV1, normalize_package_versions};

pub const EXPORT_REPORT_SCHEMA_V1: &str = "tidas.export-report.v1";
pub const EXPORT_REPORT_JSON_SCHEMA_V1: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/contracts/export-report.v1.schema.json"
));
const DEFAULT_NETWORK_TIMEOUT: Duration = Duration::from_secs(60);

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ExportFormat {
    Tidas,
    Ilcd,
}

impl ExportFormat {
    #[must_use]
    pub const fn extension(self) -> &'static str {
        match self {
            Self::Tidas => "json",
            Self::Ilcd => "xml",
        }
    }
}

#[derive(Clone)]
pub struct SecretString(String);

impl SecretString {
    #[must_use]
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    #[must_use]
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for SecretString {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("SecretString([REDACTED])")
    }
}

#[derive(Clone, Debug)]
pub struct S3Config {
    pub bucket: String,
    pub region: String,
    pub endpoint: Option<String>,
    pub prefix: Option<String>,
    pub access_key_id: SecretString,
    pub secret_access_key: SecretString,
    pub session_token: Option<SecretString>,
}

#[derive(Clone, Debug)]
pub struct ExportRequest {
    pub database_url: SecretString,
    pub output_zip: PathBuf,
    pub format: ExportFormat,
    pub external_documents: Option<S3Config>,
    pub skip_external_documents: bool,
    pub cancellation: CancellationToken,
    pub memory_budget: MemoryBudget,
    pub queue_capacity: usize,
    pub network_timeout: Duration,
}

impl ExportRequest {
    #[must_use]
    pub fn new(
        database_url: SecretString,
        output_zip: PathBuf,
        format: ExportFormat,
        cancellation: CancellationToken,
        memory_budget: MemoryBudget,
        queue_capacity: usize,
    ) -> Self {
        Self {
            database_url,
            output_zip,
            format,
            external_documents: None,
            skip_external_documents: false,
            cancellation,
            memory_budget,
            queue_capacity,
            network_timeout: DEFAULT_NETWORK_TIMEOUT,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ExportReportV1 {
    pub schema_version: String,
    pub format: ExportFormat,
    pub database_record_count: u64,
    pub common_record_count: u64,
    pub category_record_count: u64,
    pub external_document_count: u64,
    pub external_document_bytes: u64,
    pub external_documents_skipped: bool,
    pub version_normalization: Option<VersionNormalizationV1>,
    pub archive_member_count: u64,
    pub archive_bytes: u64,
    pub archive_sha256: String,
    pub peak_accounted_memory_bytes: u64,
    pub warnings: Vec<String>,
}

/// Streams a consistent `PostgreSQL` snapshot into a deterministic package ZIP.
pub fn run_export(request: &ExportRequest) -> Result<ExportReportV1, ExportError> {
    request.cancellation.check()?;
    if request.queue_capacity == 0 {
        return Err(ExportError::ZeroQueueCapacity);
    }
    let parent = request
        .output_zip
        .parent()
        .unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)?;
    if request.output_zip.exists()
        && !fs::symlink_metadata(&request.output_zip)?
            .file_type()
            .is_file()
    {
        return Err(ExportError::OutputNotRegularFile(
            request.output_zip.clone(),
        ));
    }
    let staging = Builder::new().prefix(".tidas-export-").tempdir_in(parent)?;
    let package_dir = staging.path().join("package");
    fs::create_dir(&package_dir)?;

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(ExportError::RuntimeCreate)?;
    let counts = runtime.block_on(database::export_records(request, &package_dir))?;

    let version_normalization = if request.format == ExportFormat::Tidas {
        Some(normalize_package_versions(
            &package_dir,
            &request.cancellation,
        )?)
    } else {
        None
    };

    let mut warnings = counts.warnings;
    let external = if request.skip_external_documents {
        storage::StorageCounts::skipped()
    } else if let Some(config) = &request.external_documents {
        runtime.block_on(storage::download_external_documents(
            config,
            &package_dir.join("external_docs"),
            request,
        ))?
    } else {
        warnings.push(
            "External documents were skipped because no complete S3 configuration was supplied."
                .to_owned(),
        );
        storage::StorageCounts::skipped()
    };

    request.cancellation.check()?;
    let archive = archive::write_deterministic_zip(
        &package_dir,
        &request.output_zip,
        &request.cancellation,
        &request.memory_budget,
    )?;

    Ok(ExportReportV1 {
        schema_version: EXPORT_REPORT_SCHEMA_V1.to_owned(),
        format: request.format,
        database_record_count: counts.common + counts.category,
        common_record_count: counts.common,
        category_record_count: counts.category,
        external_document_count: external.documents,
        external_document_bytes: external.bytes,
        external_documents_skipped: external.skipped,
        version_normalization,
        archive_member_count: archive.members,
        archive_bytes: archive.bytes,
        archive_sha256: archive.sha256,
        peak_accounted_memory_bytes: request.memory_budget.peak(),
        warnings,
    })
}

pub(crate) fn write_record(
    package_dir: &Path,
    relative_stem: &Path,
    json: &[u8],
    format: ExportFormat,
    cancellation: &CancellationToken,
) -> Result<(), ExportError> {
    cancellation.check()?;
    validate_relative_path(relative_stem)?;
    let mut path = relative_stem.as_os_str().to_os_string();
    path.push(format!(".{}", format.extension()));
    let path = PathBuf::from(path);
    let target = package_dir.join(path);
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent)?;
    }
    let bytes = match format {
        ExportFormat::Tidas => json.to_vec(),
        ExportFormat::Ilcd => convert_json_to_xml(json, cancellation)?,
    };
    fs::write(target, bytes)?;
    Ok(())
}

pub(crate) fn validate_relative_path(path: &Path) -> Result<(), ExportError> {
    if path.as_os_str().is_empty() || path.is_absolute() {
        return Err(ExportError::UnsafePath(path.to_path_buf()));
    }
    for component in path.components() {
        match component {
            std::path::Component::Normal(value) if portable_component(value) => {}
            _ => return Err(ExportError::UnsafePath(path.to_path_buf())),
        }
    }
    Ok(())
}

fn portable_component(value: &std::ffi::OsStr) -> bool {
    let Some(text) = value.to_str() else {
        return false;
    };
    if text.is_empty()
        || text == "."
        || text == ".."
        || text.ends_with([' ', '.'])
        || text.chars().any(|character| {
            character.is_control()
                || matches!(character, '<' | '>' | ':' | '"' | '\\' | '|' | '?' | '*')
        })
    {
        return false;
    }
    let base = text
        .split_once('.')
        .map_or(text, |(candidate, _)| candidate)
        .to_ascii_uppercase();
    !matches!(base.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        && !(base.len() == 4
            && (base.starts_with("COM") || base.starts_with("LPT"))
            && base
                .as_bytes()
                .last()
                .is_some_and(|digit| matches!(digit, b'1'..=b'9')))
}

#[derive(Debug, Error)]
pub enum ExportError {
    #[error("export queue capacity must be greater than zero")]
    ZeroQueueCapacity,
    #[error("unsafe database or object-storage path: {0}")]
    UnsafePath(PathBuf),
    #[error("export output exists but is not a regular file: {0}")]
    OutputNotRegularFile(PathBuf),
    #[error("database connection failed")]
    DatabaseConnect(#[source] tokio_postgres::Error),
    #[error("no native TLS root certificates are available for database connections")]
    DatabaseTlsRoots,
    #[error("database snapshot export failed")]
    Database(#[source] tokio_postgres::Error),
    #[error("database producer task failed: {0}")]
    DatabaseTask(#[from] tokio::task::JoinError),
    #[error("S3-compatible object storage configuration is invalid")]
    StorageConfiguration(#[source] object_store::Error),
    #[error("S3-compatible object storage operation failed")]
    Storage(#[source] object_store::Error),
    #[error("S3-compatible object storage operation timed out")]
    StorageTimeout,
    #[error("failed to create the export runtime: {0}")]
    RuntimeCreate(std::io::Error),
    #[error("export package JSON is malformed: {0}")]
    Json(#[from] serde_json::Error),
    #[error("export package XML serialization failed: {0}")]
    Conversion(#[from] ConversionError),
    #[error("export archive failed: {0}")]
    Zip(#[from] zip::result::ZipError),
    #[error("export runtime failed: {0}")]
    Runtime(#[from] RuntimeError),
    #[error("export I/O failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("atomic archive publication failed and rollback also failed: {source}; {restore}")]
    CommitRollback {
        source: std::io::Error,
        restore: std::io::Error,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secrets_are_always_redacted_from_debug() {
        let secret = SecretString::new("postgres://user:password@database/db");
        assert_eq!(format!("{secret:?}"), "SecretString([REDACTED])");
        assert!(!format!("{secret:?}").contains("password"));
    }

    #[test]
    fn checked_in_schema_matches_report_version() {
        let schema: serde_json::Value = serde_json::from_str(EXPORT_REPORT_JSON_SCHEMA_V1).unwrap();
        assert_eq!(
            schema["properties"]["schema_version"]["const"],
            EXPORT_REPORT_SCHEMA_V1
        );
    }

    #[test]
    fn version_suffix_is_preserved_when_record_extension_is_appended() {
        let temporary = tempfile::tempdir().unwrap();
        write_record(
            temporary.path(),
            Path::new("contacts/11111111-1111-1111-1111-111111111111_01.00.006"),
            b"{\"contactDataSet\":{}}\n",
            ExportFormat::Tidas,
            &CancellationToken::default(),
        )
        .unwrap();
        assert!(
            temporary
                .path()
                .join("contacts/11111111-1111-1111-1111-111111111111_01.00.006.json")
                .exists()
        );
    }

    #[test]
    fn path_components_rejected_by_windows_are_rejected_on_every_platform() {
        for path in ["external_docs/CON.txt", "a:b"] {
            assert!(matches!(
                validate_relative_path(Path::new(path)),
                Err(ExportError::UnsafePath(_))
            ));
        }
    }

    #[test]
    fn ilcd_target_uses_the_native_xml_serializer() {
        let temporary = tempfile::tempdir().unwrap();
        write_record(
            temporary.path(),
            Path::new("flows/22222222-2222-2222-2222-222222222222_01.00.000"),
            b"{\"flowDataSet\":{\"common:UUID\":\"22222222-2222-2222-2222-222222222222\"}}\n",
            ExportFormat::Ilcd,
            &CancellationToken::default(),
        )
        .unwrap();
        let xml = fs::read_to_string(
            temporary
                .path()
                .join("flows/22222222-2222-2222-2222-222222222222_01.00.000.xml"),
        )
        .unwrap();
        assert!(xml.starts_with("<?xml version=\"1.0\" encoding=\"utf-8\"?>"));
        assert!(xml.contains("<flowDataSet>"));
        assert!(xml.contains("<common:UUID>22222222-2222-2222-2222-222222222222</common:UUID>"));
    }

    #[test]
    fn existing_output_directory_is_never_replaced() {
        let temporary = tempfile::tempdir().unwrap();
        let output = temporary.path().join("existing");
        fs::create_dir(&output).unwrap();
        let request = ExportRequest::new(
            SecretString::new("postgresql://unused"),
            output.clone(),
            ExportFormat::Tidas,
            CancellationToken::default(),
            MemoryBudget::new(8 * 1024 * 1024),
            1,
        );
        assert!(matches!(
            run_export(&request),
            Err(ExportError::OutputNotRegularFile(path)) if path == output
        ));
        assert!(output.is_dir());
    }
}
