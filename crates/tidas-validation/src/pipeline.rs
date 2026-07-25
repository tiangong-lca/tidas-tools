use std::fs;
use std::path::{Path, PathBuf};

use tempfile::NamedTempFile;
use thiserror::Error;
use tidas_assets::{AssetError, asset_fingerprint};
use tidas_runtime::{CancellationToken, JsonlSpool, MemoryBudget, RuntimeError, SpoolSummaryV1};

use crate::contracts::{
    CategorySummaryV1, ValidationIssueEventV1, ValidationIssueV1, ValidationSummaryV1,
};
use crate::schema::{SUPPORTED_TIDAS_CATEGORIES, SchemaCatalog, SchemaError, TidasCategory};

const PATH_ACCOUNTING_OVERHEAD: u64 = 128;
const JSON_MEMORY_MULTIPLIER: u64 = 8;
const JSON_MEMORY_OVERHEAD: u64 = 4096;
const MAX_ISSUE_EVENT_BYTES: usize = 1024 * 1024;

#[derive(Clone, Debug)]
pub struct ValidationRequest {
    pub input_dir: PathBuf,
    pub issue_spool: Option<PathBuf>,
    pub cancellation: CancellationToken,
    pub memory_budget: MemoryBudget,
    pub queue_capacity: usize,
}

#[derive(Clone, Debug)]
pub struct ValidationOutput {
    pub summary: ValidationSummaryV1,
    pub issue_spool_path: Option<PathBuf>,
}

pub fn validate_tidas_package(
    request: &ValidationRequest,
) -> Result<ValidationOutput, ValidationError> {
    request.cancellation.check()?;
    if request.queue_capacity == 0 {
        return Err(ValidationError::ZeroQueueCapacity);
    }
    if !request.input_dir.is_dir() {
        return Err(ValidationError::InputNotDirectory(
            request.input_dir.clone(),
        ));
    }

    let catalog = SchemaCatalog::load()?;
    let mut summary = ValidationSummaryV1::new(asset_fingerprint()?);
    let mut sink = IssueSink::new(request.issue_spool.as_deref())?;

    for category in SUPPORTED_TIDAS_CATEGORIES {
        request.cancellation.check()?;
        let category_dir = request.input_dir.join(category.as_str());
        if !category_dir.is_dir() {
            continue;
        }
        let validator = catalog.validator(category)?;
        let (files, _path_reservation) = sorted_json_files(&category_dir, &request.memory_budget)?;
        let mut category_summary = CategorySummaryV1 {
            category: category.as_str().to_owned(),
            ..CategorySummaryV1::default()
        };
        for file_path in files {
            request.cancellation.check()?;
            category_summary.document_count += 1;
            summary.document_count += 1;
            validate_file(
                &request.input_dir,
                &file_path,
                category,
                &validator,
                request,
                &mut summary,
                &mut category_summary,
                &mut sink,
            )?;
        }
        summary.categories.push(category_summary);
    }

    summary.category_count =
        u64::try_from(summary.categories.len()).map_err(|_| ValidationError::SizeOverflow)?;
    summary.ok = summary.issue_count == 0;
    let (issue_spool_path, spool_summary) = sink.finish()?;
    summary.issue_spool = spool_summary;
    summary.peak_accounted_memory_bytes = request.memory_budget.peak();
    Ok(ValidationOutput {
        summary,
        issue_spool_path,
    })
}

#[allow(clippy::too_many_arguments)]
fn validate_file(
    package_root: &Path,
    file_path: &Path,
    category: TidasCategory,
    validator: &crate::schema::TidasValidator,
    request: &ValidationRequest,
    summary: &mut ValidationSummaryV1,
    category_summary: &mut CategorySummaryV1,
    sink: &mut IssueSink,
) -> Result<(), ValidationError> {
    let relative_path = normalized_relative_path(package_root, file_path)?;
    let file_bytes = fs::metadata(file_path)?.len();
    let estimated_bytes = file_bytes
        .checked_mul(JSON_MEMORY_MULTIPLIER)
        .and_then(|value| value.checked_add(JSON_MEMORY_OVERHEAD))
        .ok_or(ValidationError::SizeOverflow)?;
    let _file_reservation = request.memory_budget.reserve(estimated_bytes)?;
    let bytes = fs::read(file_path)?;
    let instance = match serde_json::from_slice(&bytes) {
        Ok(instance) => instance,
        Err(error) => {
            let issue = ValidationIssueV1::error(
                "invalid_json",
                category.as_str(),
                &relative_path,
                "<root>",
                format!("Invalid JSON: {error}"),
            );
            record_issue(summary, category_summary, sink, issue)?;
            return Ok(());
        }
    };
    for issue in validator.issues(&instance, &relative_path) {
        request.cancellation.check()?;
        record_issue(summary, category_summary, sink, issue)?;
    }
    Ok(())
}

fn record_issue(
    summary: &mut ValidationSummaryV1,
    category: &mut CategorySummaryV1,
    sink: &mut IssueSink,
    issue: ValidationIssueV1,
) -> Result<(), ValidationError> {
    summary.record_issue(category, issue.severity);
    sink.push(issue)
}

fn sorted_json_files(
    directory: &Path,
    budget: &MemoryBudget,
) -> Result<(Vec<PathBuf>, tidas_runtime::MemoryReservation), ValidationError> {
    let mut estimated_bytes = 0_u64;
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        if is_regular_json_file(&entry)? {
            estimated_bytes = estimated_bytes
                .checked_add(path_estimated_bytes(&entry.path())?)
                .ok_or(ValidationError::SizeOverflow)?;
        }
    }
    let reservation = budget.reserve(estimated_bytes)?;
    let mut files = Vec::new();
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        if is_regular_json_file(&entry)? {
            files.push(entry.path());
        }
    }
    files.sort_by_key(|path| normalized_sort_key(path));
    Ok((files, reservation))
}

fn is_regular_json_file(entry: &fs::DirEntry) -> Result<bool, std::io::Error> {
    Ok(entry.file_type()?.is_file()
        && entry
            .path()
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("json")))
}

fn path_estimated_bytes(path: &Path) -> Result<u64, ValidationError> {
    let path_bytes =
        u64::try_from(path.as_os_str().len()).map_err(|_| ValidationError::SizeOverflow)?;
    path_bytes
        .checked_add(PATH_ACCOUNTING_OVERHEAD)
        .ok_or(ValidationError::SizeOverflow)
}

fn normalized_sort_key(path: &Path) -> String {
    path.file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .replace('\\', "/")
}

fn normalized_relative_path(root: &Path, path: &Path) -> Result<String, ValidationError> {
    let relative = path
        .strip_prefix(root)
        .map_err(|_| ValidationError::PathOutsideInput(path.to_path_buf()))?;
    Ok(relative.to_string_lossy().replace('\\', "/"))
}

struct IssueSink {
    target: Option<PathBuf>,
    spool: Option<JsonlSpool<NamedTempFile>>,
    next_ordinal: u64,
}

impl IssueSink {
    fn new(target: Option<&Path>) -> Result<Self, ValidationError> {
        let spool = match target {
            Some(target) => {
                let parent = target.parent().unwrap_or_else(|| Path::new("."));
                if !parent.is_dir() {
                    return Err(ValidationError::SpoolParentMissing(parent.to_path_buf()));
                }
                Some(JsonlSpool::new(
                    NamedTempFile::new_in(parent)?,
                    MAX_ISSUE_EVENT_BYTES,
                ))
            }
            None => None,
        };
        Ok(Self {
            target: target.map(Path::to_path_buf),
            spool,
            next_ordinal: 0,
        })
    }

    fn push(&mut self, issue: ValidationIssueV1) -> Result<(), ValidationError> {
        if let Some(spool) = &mut self.spool {
            spool.push(&ValidationIssueEventV1::new(self.next_ordinal, issue))?;
        }
        self.next_ordinal = self
            .next_ordinal
            .checked_add(1)
            .ok_or(ValidationError::SizeOverflow)?;
        Ok(())
    }

    fn finish(self) -> Result<(Option<PathBuf>, Option<SpoolSummaryV1>), ValidationError> {
        let Some(spool) = self.spool else {
            return Ok((None, None));
        };
        let target = self
            .target
            .expect("a configured spool always has a target path");
        let (temporary, summary) = spool.finish()?;
        temporary
            .persist(&target)
            .map_err(|error| ValidationError::PersistSpool {
                path: target.clone(),
                source: error.error,
            })?;
        Ok((Some(target), Some(summary)))
    }
}

#[derive(Debug, Error)]
pub enum ValidationError {
    #[error("validation input is not a directory: {0}")]
    InputNotDirectory(PathBuf),
    #[error("validation queue capacity must be greater than zero")]
    ZeroQueueCapacity,
    #[error("validation path is outside the input directory: {0}")]
    PathOutsideInput(PathBuf),
    #[error("issue spool parent directory does not exist: {0}")]
    SpoolParentMissing(PathBuf),
    #[error("failed to persist issue spool at {path}: {source}")]
    PersistSpool {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("validation size cannot be represented safely")]
    SizeOverflow,
    #[error(transparent)]
    Asset(#[from] AssetError),
    #[error(transparent)]
    Runtime(#[from] RuntimeError),
    #[error(transparent)]
    Schema(#[from] SchemaError),
    #[error("validation I/O failed: {0}")]
    Io(#[from] std::io::Error),
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    fn request(input_dir: &Path, issue_spool: Option<PathBuf>) -> ValidationRequest {
        ValidationRequest {
            input_dir: input_dir.to_path_buf(),
            issue_spool,
            cancellation: CancellationToken::default(),
            memory_budget: MemoryBudget::new(16 * 1024 * 1024),
            queue_capacity: 16,
        }
    }

    #[test]
    fn empty_package_is_a_complete_success() {
        let directory = tempfile::tempdir().unwrap();
        let output = validate_tidas_package(&request(directory.path(), None)).unwrap();
        assert!(output.summary.ok);
        assert_eq!(output.summary.document_count, 0);
        assert_eq!(output.summary.issue_count, 0);
        assert_eq!(output.summary.categories, []);
    }

    #[test]
    fn schema_and_parse_issues_stream_in_deterministic_file_order() {
        let directory = tempfile::tempdir().unwrap();
        let sources = directory.path().join("sources");
        fs::create_dir(&sources).unwrap();
        fs::write(sources.join("b.json"), b"{").unwrap();
        fs::write(sources.join("a.json"), b"{}").unwrap();
        let spool_path = directory.path().join("issues.jsonl");

        let first =
            validate_tidas_package(&request(directory.path(), Some(spool_path.clone()))).unwrap();
        let first_bytes = fs::read(&spool_path).unwrap();
        let second =
            validate_tidas_package(&request(directory.path(), Some(spool_path.clone()))).unwrap();
        let second_bytes = fs::read(&spool_path).unwrap();

        assert!(!first.summary.ok);
        assert_eq!(first.summary, second.summary);
        assert_eq!(first_bytes, second_bytes);
        assert_eq!(first.summary.document_count, 2);
        assert!(first.summary.issue_count >= 2);
        let events: Vec<ValidationIssueEventV1> = first_bytes
            .split(|byte| *byte == b'\n')
            .filter(|line| !line.is_empty())
            .map(|line| serde_json::from_slice(line).unwrap())
            .collect();
        assert!(events[0].issue.file_path.ends_with("a.json"));
        assert!(events.last().unwrap().issue.file_path.ends_with("b.json"));
        assert_eq!(
            first.summary.issue_spool.as_ref().unwrap().event_count,
            first.summary.issue_count
        );
    }

    #[test]
    fn cancellation_stops_before_reading_documents() {
        let directory = tempfile::tempdir().unwrap();
        let cancellation = CancellationToken::default();
        cancellation.cancel();
        let request = ValidationRequest {
            input_dir: directory.path().to_path_buf(),
            issue_spool: None,
            cancellation,
            memory_budget: MemoryBudget::new(1024),
            queue_capacity: 1,
        };
        assert!(matches!(
            validate_tidas_package(&request),
            Err(ValidationError::Runtime(RuntimeError::Cancelled))
        ));
    }

    #[test]
    fn oversized_document_is_rejected_by_the_explicit_budget() {
        let directory = tempfile::tempdir().unwrap();
        let sources = directory.path().join("sources");
        fs::create_dir(&sources).unwrap();
        fs::write(sources.join("large.json"), vec![b' '; 1024]).unwrap();
        let request = ValidationRequest {
            input_dir: directory.path().to_path_buf(),
            issue_spool: None,
            cancellation: CancellationToken::default(),
            memory_budget: MemoryBudget::new(1024),
            queue_capacity: 1,
        };
        assert!(matches!(
            validate_tidas_package(&request),
            Err(ValidationError::Runtime(
                RuntimeError::BudgetExceeded { .. }
            ))
        ));
    }
}
