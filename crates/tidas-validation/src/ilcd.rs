use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

use quick_xml::events::Event;
use quick_xml::name::ResolveResult;
use quick_xml::reader::NsReader;
use tempfile::TempDir;
use tidas_assets::{AssetKind, asset_fingerprint, bundled_assets};
use tidas_xml::{CompiledXsd, XmlDiagnostic, XmlError};
use walkdir::WalkDir;

use crate::contracts::{CategorySummaryV1, ValidationIssueV1, ValidationSummaryV1};
use crate::pipeline::{
    IssueSink, ValidationError, ValidationOutput, ValidationRequest, record_issue, report_progress,
};
use crate::schema::is_valid_cas_number;

const PATH_ACCOUNTING_OVERHEAD: u64 = 128;
const XML_MEMORY_MULTIPLIER: u64 = 6;
const XML_MEMORY_OVERHEAD: u64 = 4096;
const XSD_ASSET_PREFIX: &str = "src/tidas_tools/eilcd/schemas/";

#[derive(Clone, Copy, Debug)]
struct IlcdSchema {
    namespace: &'static str,
    root: &'static str,
    category: &'static str,
    filename: &'static str,
}

const ILCD_SCHEMAS: [IlcdSchema; 12] = [
    IlcdSchema {
        namespace: "http://lca.jrc.it/ILCD/Wrapper",
        root: "ILCD",
        category: "wrapper",
        filename: "ILCD_ILCD.xsd",
    },
    IlcdSchema {
        namespace: "http://lca.jrc.it/ILCD/Contact",
        root: "contactDataSet",
        category: "contacts",
        filename: "ILCD_ContactDataSet.xsd",
    },
    IlcdSchema {
        namespace: "http://lca.jrc.it/ILCD/FlowProperty",
        root: "flowPropertyDataSet",
        category: "flowproperties",
        filename: "ILCD_FlowPropertyDataSet.xsd",
    },
    IlcdSchema {
        namespace: "http://lca.jrc.it/ILCD/Flow",
        root: "flowDataSet",
        category: "flows",
        filename: "ILCD_FlowDataSet.xsd",
    },
    IlcdSchema {
        namespace: "http://lca.jrc.it/ILCD/LCIAMethod",
        root: "LCIAMethodDataSet",
        category: "lciamethods",
        filename: "ILCD_LCIAMethodDataSet.xsd",
    },
    IlcdSchema {
        namespace: "http://eplca.jrc.ec.europa.eu/ILCD/LifeCycleModel/2017",
        root: "lifeCycleModelDataSet",
        category: "lifecyclemodels",
        filename: "ILCD_LifeCycleModelDataSet.xsd",
    },
    IlcdSchema {
        namespace: "http://lca.jrc.it/ILCD/Process",
        root: "processDataSet",
        category: "processes",
        filename: "ILCD_ProcessDataSet.xsd",
    },
    IlcdSchema {
        namespace: "http://lca.jrc.it/ILCD/Source",
        root: "sourceDataSet",
        category: "sources",
        filename: "ILCD_SourceDataSet.xsd",
    },
    IlcdSchema {
        namespace: "http://lca.jrc.it/ILCD/UnitGroup",
        root: "unitGroupDataSet",
        category: "unitgroups",
        filename: "ILCD_UnitGroupDataSet.xsd",
    },
    IlcdSchema {
        namespace: "http://lca.jrc.it/ILCD/Categories",
        root: "CategorySystem",
        category: "categories",
        filename: "ILCD_Categories.xsd",
    },
    IlcdSchema {
        namespace: "http://lca.jrc.it/ILCD/Locations",
        root: "ILCDLocations",
        category: "locations",
        filename: "ILCD_Locations.xsd",
    },
    IlcdSchema {
        namespace: "http://lca.jrc.it/ILCD/LCIAMethodologies",
        root: "ILCDLCIAMethodologies",
        category: "lciamethodologies",
        filename: "ILCD_LCIAMethodologies.xsd",
    },
];

pub fn validate_ilcd_package(
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

    let staged_schemas = StagedXsdCatalog::new()?;
    let mut validators = BTreeMap::<String, CompiledXsd>::new();
    let mut summary = ValidationSummaryV1::new("ilcd-xml", asset_fingerprint()?);
    let mut categories = BTreeMap::<String, CategorySummaryV1>::new();
    let mut sink = IssueSink::new(request.issue_spool.as_deref())?;
    let (files, _path_reservation) =
        sorted_ilcd_xml_files(&request.input_dir, &request.memory_budget)?;
    let document_total = u64::try_from(files.len()).map_err(|_| ValidationError::SizeOverflow)?;
    report_progress(
        request,
        &summary,
        "started",
        None,
        Some(document_total),
        true,
    );

    for file_path in files {
        request.cancellation.check()?;
        validate_ilcd_file(
            &request.input_dir,
            &file_path,
            &staged_schemas,
            &mut validators,
            request,
            &mut summary,
            &mut categories,
            &mut sink,
        )?;
        report_progress(
            request,
            &summary,
            "validating",
            None,
            Some(document_total),
            false,
        );
    }

    summary.categories = categories.into_values().collect();
    summary.category_count =
        u64::try_from(summary.categories.len()).map_err(|_| ValidationError::SizeOverflow)?;
    summary.ok = summary.issue_count == 0;
    let (issue_spool_path, spool_summary) = sink.finish()?;
    summary.issue_spool = spool_summary;
    summary.peak_accounted_memory_bytes = request.memory_budget.peak();
    report_progress(
        request,
        &summary,
        "completed",
        None,
        Some(document_total),
        true,
    );
    Ok(ValidationOutput {
        summary,
        issue_spool_path,
    })
}

#[allow(clippy::too_many_arguments)]
fn validate_ilcd_file(
    package_root: &Path,
    file_path: &Path,
    staged_schemas: &StagedXsdCatalog,
    validators: &mut BTreeMap<String, CompiledXsd>,
    request: &ValidationRequest,
    summary: &mut ValidationSummaryV1,
    categories: &mut BTreeMap<String, CategorySummaryV1>,
    sink: &mut IssueSink,
) -> Result<(), ValidationError> {
    let relative_path = normalized_relative_path(package_root, file_path)?;
    let file_bytes = fs::metadata(file_path)?.len();
    let estimated_bytes = file_bytes
        .checked_mul(XML_MEMORY_MULTIPLIER)
        .and_then(|value| value.checked_add(XML_MEMORY_OVERHEAD))
        .ok_or(ValidationError::SizeOverflow)?;
    let _file_reservation = request.memory_budget.reserve(estimated_bytes)?;
    let bytes = fs::read(file_path)?;

    let root = match inspect_root(&bytes) {
        Ok(root) => root,
        Err(message) => {
            let category = category_summary(categories, "ilcd");
            summary.document_count += 1;
            category.document_count += 1;
            return record_issue(
                summary,
                category,
                sink,
                ValidationIssueV1::error(
                    "invalid_xml",
                    "ilcd",
                    &relative_path,
                    "<root>",
                    format!("Invalid XML: {message}"),
                ),
            );
        }
    };
    let Some(schema) = schema_for_root(&root.namespace, &root.local_name) else {
        let category = category_summary(categories, "ilcd");
        summary.document_count += 1;
        category.document_count += 1;
        let mut issue = ValidationIssueV1::error(
            "unsupported_ilcd_root",
            "ilcd",
            &relative_path,
            format!("/{}", root.local_name),
            format!(
                "Unsupported ILCD XML root element {{{}}}{}",
                root.namespace, root.local_name
            ),
        );
        issue
            .context
            .insert("namespace".to_owned(), root.namespace.into());
        issue
            .context
            .insert("local_name".to_owned(), root.local_name.into());
        return record_issue(summary, category, sink, issue);
    };

    summary.document_count += 1;
    let category = category_summary(categories, schema.category);
    category.document_count += 1;
    let validator = if let Some(validator) = validators.get_mut(schema.filename) {
        validator
    } else {
        let compiled = CompiledXsd::from_path(&staged_schemas.path(schema.filename))?;
        validators.insert(schema.filename.to_owned(), compiled);
        validators
            .get_mut(schema.filename)
            .expect("validator was inserted immediately before lookup")
    };

    match validator.validate(&bytes) {
        Ok(diagnostics) => {
            for diagnostic in diagnostics {
                request.cancellation.check()?;
                record_issue(
                    summary,
                    category,
                    sink,
                    diagnostic_issue(schema.category, &relative_path, diagnostic),
                )?;
            }
        }
        Err(XmlError::NativeParse(error)) => {
            record_issue(
                summary,
                category,
                sink,
                ValidationIssueV1::error(
                    "invalid_xml",
                    schema.category,
                    &relative_path,
                    "<root>",
                    format!("Invalid XML: {error}"),
                ),
            )?;
            return Ok(());
        }
        Err(error) => return Err(error.into()),
    }

    if schema.category == "flows" {
        record_invalid_cas_numbers(&bytes, &relative_path, summary, category, sink)?;
    }
    Ok(())
}

fn diagnostic_issue(
    category: &str,
    file_path: &str,
    diagnostic: XmlDiagnostic,
) -> ValidationIssueV1 {
    let mut location = "<root>".to_owned();
    if let Some(line) = diagnostic.line {
        write!(location, ":line {line}").expect("writing to a String cannot fail");
        if let Some(column) = diagnostic.column {
            write!(location, ":column {column}").expect("writing to a String cannot fail");
        }
    }
    let mut issue = ValidationIssueV1::error(
        "ilcd_schema_error",
        category,
        file_path,
        location,
        diagnostic.message,
    );
    issue
        .context
        .insert("level".to_owned(), diagnostic.level.into());
    issue
        .context
        .insert("domain".to_owned(), diagnostic.domain.into());
    issue
        .context
        .insert("code".to_owned(), diagnostic.code.into());
    if let Some(filename) = diagnostic.filename {
        issue
            .context
            .insert("schema_file".to_owned(), filename.into());
    }
    issue
}

fn category_summary<'a>(
    categories: &'a mut BTreeMap<String, CategorySummaryV1>,
    category: &str,
) -> &'a mut CategorySummaryV1 {
    categories
        .entry(category.to_owned())
        .or_insert_with(|| CategorySummaryV1 {
            category: category.to_owned(),
            ..CategorySummaryV1::default()
        })
}

struct IlcdRoot {
    namespace: String,
    local_name: String,
}

fn inspect_root(bytes: &[u8]) -> Result<IlcdRoot, String> {
    let mut reader = NsReader::from_reader(bytes);
    reader.config_mut().check_comments = true;
    reader.config_mut().check_end_names = true;
    reader.config_mut().allow_unmatched_ends = false;
    let mut buffer = Vec::new();
    let mut root = None;
    let mut open_depth = 0_u64;
    loop {
        let (namespace, event) = reader
            .read_resolved_event_into(&mut buffer)
            .map_err(|error| error.to_string())?;
        match event {
            Event::Start(element) => {
                open_depth = open_depth
                    .checked_add(1)
                    .ok_or_else(|| "XML nesting depth overflowed".to_owned())?;
                if root.is_none() {
                    root = Some(resolve_root(namespace, &element)?);
                }
            }
            Event::Empty(element) if root.is_none() => {
                root = Some(resolve_root(namespace, &element)?);
            }
            Event::End(_) => {
                open_depth = open_depth
                    .checked_sub(1)
                    .ok_or_else(|| "XML contains an unmatched closing element".to_owned())?;
            }
            Event::Eof => {
                if open_depth != 0 {
                    return Err("XML input ends before all elements are closed".to_owned());
                }
                break;
            }
            _ => {}
        }
        buffer.clear();
    }
    root.ok_or_else(|| "XML input has no root element".to_owned())
}

fn resolve_root(
    namespace: ResolveResult<'_>,
    element: &quick_xml::events::BytesStart<'_>,
) -> Result<IlcdRoot, String> {
    let namespace = match namespace {
        ResolveResult::Bound(namespace) => String::from_utf8_lossy(namespace.as_ref()).into_owned(),
        ResolveResult::Unbound => String::new(),
        ResolveResult::Unknown(prefix) => {
            return Err(format!(
                "unknown XML namespace prefix {}",
                String::from_utf8_lossy(&prefix)
            ));
        }
    };
    Ok(IlcdRoot {
        namespace,
        local_name: String::from_utf8_lossy(element.local_name().as_ref()).into_owned(),
    })
}

fn record_invalid_cas_numbers(
    bytes: &[u8],
    file_path: &str,
    summary: &mut ValidationSummaryV1,
    category: &mut CategorySummaryV1,
    sink: &mut IssueSink,
) -> Result<(), ValidationError> {
    let mut reader = NsReader::from_reader(bytes);
    let mut buffer = Vec::new();
    let mut in_cas_number = false;
    loop {
        let (namespace, event) = reader
            .read_resolved_event_into(&mut buffer)
            .map_err(|error| ValidationError::InvalidXml(error.to_string()))?;
        match event {
            Event::Start(element) => {
                in_cas_number = element.local_name().as_ref() == b"CASNumber"
                    && matches!(
                        namespace,
                        ResolveResult::Bound(value)
                            if value.as_ref() == b"http://lca.jrc.it/ILCD/Flow"
                    );
            }
            Event::Text(text) if in_cas_number => {
                let value = text
                    .decode()
                    .map_err(|error| ValidationError::InvalidXml(error.to_string()))?;
                if has_cas_number_shape(&value) && !is_valid_cas_number(&value) {
                    let mut issue = ValidationIssueV1::error(
                        "cas_number_checksum_error",
                        "flows",
                        file_path,
                        "/flowDataSet/flowInformation/dataSetInformation/CASNumber",
                        format!("CASNumber '{value}' has an invalid check digit."),
                    );
                    issue
                        .context
                        .insert("validator".to_owned(), "cas-number".into());
                    record_issue(summary, category, sink, issue)?;
                }
            }
            Event::End(element) if element.local_name().as_ref() == b"CASNumber" => {
                in_cas_number = false;
            }
            Event::Eof => break,
            _ => {}
        }
        buffer.clear();
    }
    Ok(())
}

fn has_cas_number_shape(value: &str) -> bool {
    let mut parts = value.split('-');
    let (Some(first), Some(second), Some(check), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return false;
    };
    (2..=7).contains(&first.len())
        && second.len() == 2
        && check.len() == 1
        && first.bytes().all(|byte| byte.is_ascii_digit())
        && second.bytes().all(|byte| byte.is_ascii_digit())
        && check.bytes().all(|byte| byte.is_ascii_digit())
}

fn schema_for_root(namespace: &str, local_name: &str) -> Option<&'static IlcdSchema> {
    ILCD_SCHEMAS
        .iter()
        .find(|schema| schema.namespace == namespace && schema.root == local_name)
}

struct StagedXsdCatalog {
    directory: TempDir,
}

impl StagedXsdCatalog {
    fn new() -> Result<Self, ValidationError> {
        let directory = tempfile::tempdir()?;
        for asset in bundled_assets()
            .into_iter()
            .filter(|asset| asset.kind == AssetKind::Xsd)
        {
            let filename = asset
                .path
                .strip_prefix(XSD_ASSET_PREFIX)
                .ok_or_else(|| ValidationError::UnexpectedXsdAsset(asset.path.clone()))?;
            fs::write(directory.path().join(filename), asset.bytes)?;
        }
        Ok(Self { directory })
    }

    fn path(&self, filename: &str) -> PathBuf {
        self.directory.path().join(filename)
    }
}

fn sorted_ilcd_xml_files(
    input: &Path,
    budget: &tidas_runtime::MemoryBudget,
) -> Result<(Vec<PathBuf>, tidas_runtime::MemoryReservation), ValidationError> {
    let data = input.join("data");
    let search_root = if data.is_dir() { data.as_path() } else { input };
    let mut estimated_bytes = 0_u64;
    for item in WalkDir::new(search_root).follow_links(false) {
        let item = item.map_err(|error| ValidationError::Walk(error.to_string()))?;
        if is_ilcd_xml_file(item.path(), input, search_root, item.file_type().is_file())? {
            estimated_bytes = estimated_bytes
                .checked_add(path_estimated_bytes(item.path())?)
                .ok_or(ValidationError::SizeOverflow)?;
        }
    }
    let reservation = budget.reserve(estimated_bytes)?;
    let mut files = Vec::new();
    for item in WalkDir::new(search_root).follow_links(false) {
        let item = item.map_err(|error| ValidationError::Walk(error.to_string()))?;
        if is_ilcd_xml_file(item.path(), input, search_root, item.file_type().is_file())? {
            files.push(item.into_path());
        }
    }
    files.sort_by_key(|path| {
        path.strip_prefix(input)
            .unwrap_or(path)
            .to_string_lossy()
            .replace('\\', "/")
    });
    Ok((files, reservation))
}

fn is_ilcd_xml_file(
    path: &Path,
    input: &Path,
    search_root: &Path,
    is_file: bool,
) -> Result<bool, ValidationError> {
    if !is_file
        || !path
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("xml"))
        || path
            .file_name()
            .is_some_and(|name| name == "reference_types.xml")
    {
        return Ok(false);
    }
    if search_root == input {
        let relative = path
            .strip_prefix(input)
            .map_err(|_| ValidationError::PathOutsideInput(path.to_path_buf()))?;
        if relative.parent().is_some_and(|parent| {
            parent
                .components()
                .any(|part| part.as_os_str() == "schemas")
        }) || relative.parent().is_some_and(|parent| {
            parent
                .components()
                .any(|part| part.as_os_str() == "stylesheets")
        }) {
            return Ok(false);
        }
    }
    Ok(true)
}

fn path_estimated_bytes(path: &Path) -> Result<u64, ValidationError> {
    let path_bytes =
        u64::try_from(path.as_os_str().len()).map_err(|_| ValidationError::SizeOverflow)?;
    path_bytes
        .checked_add(PATH_ACCOUNTING_OVERHEAD)
        .ok_or(ValidationError::SizeOverflow)
}

fn normalized_relative_path(root: &Path, path: &Path) -> Result<String, ValidationError> {
    let relative = path
        .strip_prefix(root)
        .map_err(|_| ValidationError::PathOutsideInput(path.to_path_buf()))?;
    Ok(relative.to_string_lossy().replace('\\', "/"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tidas_runtime::{CancellationToken, MemoryBudget};

    fn request(input_dir: &Path, issue_spool: Option<PathBuf>) -> ValidationRequest {
        ValidationRequest {
            input_dir: input_dir.to_path_buf(),
            issue_spool,
            cancellation: CancellationToken::default(),
            memory_budget: MemoryBudget::new(32 * 1024 * 1024),
            queue_capacity: 8,
            progress: None,
        }
    }

    #[test]
    fn valid_locations_and_invalid_documents_are_deterministic() {
        let directory = tempfile::tempdir().unwrap();
        let valid = bundled_assets()
            .into_iter()
            .find(|asset| asset.path.ends_with("ILCDLocations_Reference.xml"))
            .unwrap();
        fs::write(directory.path().join("a-valid.xml"), valid.bytes).unwrap();
        fs::write(
            directory.path().join("b-invalid.xml"),
            br#"<ILCDLocations xmlns="http://lca.jrc.it/ILCD/Locations"/>"#,
        )
        .unwrap();
        fs::write(directory.path().join("c-unknown.xml"), b"<custom/>").unwrap();
        let spool = directory.path().join("issues.jsonl");

        let first = validate_ilcd_package(&request(directory.path(), Some(spool.clone()))).unwrap();
        let first_bytes = fs::read(&spool).unwrap();
        let second =
            validate_ilcd_package(&request(directory.path(), Some(spool.clone()))).unwrap();
        let second_bytes = fs::read(&spool).unwrap();

        assert_eq!(first.summary, second.summary);
        assert_eq!(first_bytes, second_bytes);
        assert_eq!(first.summary.document_count, 3);
        assert_eq!(first.summary.issue_count, 2);
        assert_eq!(first.summary.input_format, "ilcd-xml");
        let events: Vec<crate::ValidationIssueEventV1> = first_bytes
            .split(|byte| *byte == b'\n')
            .filter(|line| !line.is_empty())
            .map(|line| serde_json::from_slice(line).unwrap())
            .collect();
        assert_eq!(events[0].issue.issue_code, "ilcd_schema_error");
        assert_eq!(events[1].issue.issue_code, "unsupported_ilcd_root");
    }

    #[test]
    fn invalid_flow_cas_checksum_is_reported_after_xsd_validation() {
        let directory = tempfile::tempdir().unwrap();
        fs::write(
            directory.path().join("flow.xml"),
            br#"<flowDataSet xmlns="http://lca.jrc.it/ILCD/Flow" version="1.1"><flowInformation><dataSetInformation><UUID>00000000-0000-0000-0000-000000000000</UUID><name><baseName xml:lang="en">Flow</baseName></name><CASNumber>64-17-6</CASNumber><sumFormula>H</sumFormula></dataSetInformation><quantitativeReference type="Reference flow(s)"><referenceToReferenceFlowProperty>0</referenceToReferenceFlowProperty></quantitativeReference></flowInformation><modellingAndValidation><LCIMethodAndAllocation><typeOfDataSet>Unit process, single operation</typeOfDataSet><LCIMethodPrinciple>Attributional</LCIMethodPrinciple><deviationsFromLCIMethodPrinciple xml:lang="en">none</deviationsFromLCIMethodPrinciple></LCIMethodAndAllocation></modellingAndValidation><administrativeInformation><publicationAndOwnership><dataSetVersion>01.00.000</dataSetVersion></publicationAndOwnership></administrativeInformation><flowProperties><flowProperty dataSetInternalID="0"><referenceToFlowPropertyDataSet refObjectId="00000000-0000-0000-0000-000000000000" version="01.00.000" uri="../flowproperties/x.xml"/><meanValue>1</meanValue><minimumValue>1</minimumValue><maximumValue>1</maximumValue><uncertaintyDistributionType>undefined</uncertaintyDistributionType><relativeStandardDeviation95In>0</relativeStandardDeviation95In><dataDerivationTypeStatus>Measured</dataDerivationTypeStatus></flowProperty></flowProperties></flowDataSet>"#,
        )
        .unwrap();
        let output = validate_ilcd_package(&request(directory.path(), None)).unwrap();
        assert!(!output.summary.ok);
        assert!(output.summary.issue_count >= 1);
    }

    #[test]
    fn malformed_xml_is_a_data_issue_and_cancellation_stops_before_traversal() {
        let directory = tempfile::tempdir().unwrap();
        fs::write(directory.path().join("broken.xml"), b"<root><child></root>").unwrap();
        let output = validate_ilcd_package(&request(directory.path(), None)).unwrap();
        assert_eq!(output.summary.document_count, 1);
        assert_eq!(output.summary.issue_count, 1);
        assert!(!output.summary.ok);

        let cancellation = CancellationToken::default();
        cancellation.cancel();
        let cancelled = ValidationRequest {
            input_dir: directory.path().to_path_buf(),
            issue_spool: None,
            cancellation,
            memory_budget: MemoryBudget::new(1024),
            queue_capacity: 1,
            progress: None,
        };
        assert!(matches!(
            validate_ilcd_package(&cancelled),
            Err(ValidationError::Runtime(
                tidas_runtime::RuntimeError::Cancelled
            ))
        ));
    }
}
