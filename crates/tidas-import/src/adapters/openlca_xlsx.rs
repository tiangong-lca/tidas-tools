use std::collections::BTreeMap;
use std::io::{Cursor, Read};

use quick_xml::XmlVersion;
use quick_xml::events::Event;
use quick_xml::reader::Reader;
use serde_json::{Map, Value, json};
use tidas_runtime::MemoryReservation;
use uuid::Uuid;
use zip::ZipArchive;

use super::xml_node::XmlNode;
use super::{AdapterContext, AdapterError, SourceAdapter};
use crate::detect::SourceFormat;
use crate::model::CanonicalEntity;
use crate::report::{ImportIssue, IssueSeverity, IssueSink};
use crate::source::{SourceReadRequest, visit_source_entries};
use crate::store::CanonicalStore;

pub struct OpenLcaProcessXlsxAdapter;
type FlowExchange = (CanonicalEntity, Map<String, Value>);

impl SourceAdapter for OpenLcaProcessXlsxAdapter {
    fn format(&self) -> SourceFormat {
        SourceFormat::OpenlcaProcessXlsx
    }

    fn read(
        &self,
        context: &AdapterContext<'_>,
        store: &mut CanonicalStore,
        issues: &mut dyn IssueSink,
    ) -> Result<(), AdapterError> {
        let request = SourceReadRequest {
            source: context.source,
            allowed_extensions: &["xlsx"],
            max_entry_bytes: context.max_entry_bytes,
            cancellation: context.cancellation,
            memory_budget: context.memory_budget,
        };
        let mut count = 0_u64;
        visit_source_entries(&request, |entry| {
            let mut archive = ZipArchive::new(Cursor::new(entry.bytes))?;
            let layout = workbook_layout(&mut archive, context)?;
            let shared = shared_strings(&mut archive, context)?;
            let general_path = layout.get("General information").ok_or_else(|| {
                AdapterError::Io(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "workbook has no General information sheet",
                ))
            })?;
            let general = general_information(&mut archive, general_path, &shared.values, context)?;
            if general.is_empty() {
                issues.push(&ImportIssue {
                    severity: IssueSeverity::Error,
                    code: "invalid_process_xlsx".to_owned(),
                    message: "Workbook does not contain openLCA General information.".to_owned(),
                    source_object: Some(entry.label.clone()),
                    context: BTreeMap::new(),
                })?;
                return Ok::<(), AdapterError>(());
            }
            let process = process_entity(&general, &entry.stable_key);
            store.begin_process_exchanges(&process.internal_id)?;
            if let Some(path) = layout.get("Flows") {
                visit_table_rows(&mut archive, path, &shared.values, context, |row| {
                    if let Some(flow) = flow_from_row(&row, &entry.stable_key) {
                        super::generated_units::add_for_flow(store, &flow)?;
                        store.add(&flow)?;
                    }
                    Ok(())
                })?;
            }
            let mut exchange_count = 0_usize;
            for (sheet, is_input) in [("Outputs", false), ("Inputs", true)] {
                let Some(path) = layout.get(sheet) else {
                    continue;
                };
                visit_table_rows(&mut archive, path, &shared.values, context, |row| {
                    let Some((flow, exchange)) = exchange_from_row(
                        &row,
                        &entry.stable_key,
                        is_input,
                        exchange_count.saturating_add(1),
                        store,
                    )?
                    else {
                        return Ok(());
                    };
                    super::generated_units::add_for_flow(store, &flow)?;
                    store.add(&flow)?;
                    store.add_process_exchange(&process.internal_id, &exchange)?;
                    exchange_count = exchange_count.saturating_add(1);
                    Ok(())
                })?;
            }
            store.add(&process)?;
            count = count.saturating_add(1);
            issues.push(&ImportIssue {
                severity: IssueSeverity::Warning,
                code: "process_xlsx_mapping".to_owned(),
                message: "Mapped openLCA process workbook with native Rust rules.".to_owned(),
                source_object: Some(entry.label.clone()),
                context: BTreeMap::from([
                    ("process_id".to_owned(), Value::String(process.internal_id)),
                    ("exchange_count".to_owned(), json!(exchange_count)),
                ]),
            })?;
            Ok::<(), AdapterError>(())
        })?;
        if count == 0 {
            issues.push(&ImportIssue {
                severity: IssueSeverity::Error,
                code: "no_process_xlsx_workbooks".to_owned(),
                message: "No openLCA process workbooks were imported.".to_owned(),
                source_object: None,
                context: BTreeMap::new(),
            })?;
        }
        Ok(())
    }
}

struct SharedStrings {
    values: Vec<String>,
    _reservation: Option<MemoryReservation>,
}

fn workbook_layout(
    archive: &mut ZipArchive<Cursor<&[u8]>>,
    context: &AdapterContext<'_>,
) -> Result<BTreeMap<String, String>, AdapterError> {
    let workbook = read_entry(archive, "xl/workbook.xml", context, 4)?;
    let relationships = read_entry(archive, "xl/_rels/workbook.xml.rels", context, 4)?;
    let workbook = XmlNode::parse(&workbook.bytes).map_err(xml_node_io)?;
    let relationships = XmlNode::parse(&relationships.bytes).map_err(xml_node_io)?;
    let targets = relationships
        .descendants_named("Relationship")
        .filter_map(|node| Some((node.attr("Id")?.to_owned(), node.attr("Target")?.to_owned())))
        .collect::<BTreeMap<_, _>>();
    let mut result = BTreeMap::new();
    for sheet in workbook.descendants_named("sheet") {
        let Some(name) = sheet.attr("name") else {
            continue;
        };
        let Some(target) = sheet.attr("id").and_then(|id| targets.get(id)) else {
            continue;
        };
        result.insert(name.to_owned(), resolve_xl_target(target)?);
    }
    Ok(result)
}

fn shared_strings(
    archive: &mut ZipArchive<Cursor<&[u8]>>,
    context: &AdapterContext<'_>,
) -> Result<SharedStrings, AdapterError> {
    if archive.by_name("xl/sharedStrings.xml").is_err() {
        return Ok(SharedStrings {
            values: Vec::new(),
            _reservation: None,
        });
    }
    let entry = read_entry(archive, "xl/sharedStrings.xml", context, 6)?;
    let root = XmlNode::parse(&entry.bytes).map_err(xml_node_io)?;
    let values = root
        .descendants_named("si")
        .map(|item| {
            item.descendants_named("t")
                .filter_map(XmlNode::trimmed_text)
                .collect::<String>()
        })
        .collect();
    Ok(SharedStrings {
        values,
        _reservation: Some(entry.reservation),
    })
}

fn general_information(
    archive: &mut ZipArchive<Cursor<&[u8]>>,
    path: &str,
    shared: &[String],
    context: &AdapterContext<'_>,
) -> Result<BTreeMap<String, String>, AdapterError> {
    let mut found_heading = false;
    let mut result = BTreeMap::new();
    visit_sheet_rows(archive, path, shared, context, |row| {
        if !found_heading {
            found_heading = row
                .first()
                .and_then(Option::as_deref)
                .is_some_and(|value| value.trim().eq_ignore_ascii_case("general information"));
            return Ok(());
        }
        let field = row.first().and_then(Option::as_deref).map_or("", str::trim);
        if field.is_empty() {
            return Ok(());
        }
        if let Some(value) = row.get(1).and_then(Option::as_deref) {
            result.insert(field.to_ascii_lowercase(), value.trim().to_owned());
        }
        Ok(())
    })?;
    Ok(result)
}

fn visit_table_rows(
    archive: &mut ZipArchive<Cursor<&[u8]>>,
    path: &str,
    shared: &[String],
    context: &AdapterContext<'_>,
    mut visitor: impl FnMut(BTreeMap<String, String>) -> Result<(), AdapterError>,
) -> Result<(), AdapterError> {
    let mut headers: Option<Vec<String>> = None;
    visit_sheet_rows(archive, path, shared, context, |row| {
        if headers.is_none() {
            if row.iter().any(Option::is_some) {
                headers = Some(
                    row.iter()
                        .map(|value| value.as_deref().unwrap_or("").trim().to_ascii_lowercase())
                        .collect(),
                );
            }
            return Ok(());
        }
        if !row.iter().any(Option::is_some) {
            return Ok(());
        }
        let item = headers
            .as_ref()
            .expect("headers were initialized")
            .iter()
            .enumerate()
            .filter_map(|(index, header)| {
                (!header.is_empty())
                    .then(|| row.get(index).and_then(Option::as_ref))
                    .flatten()
                    .map(|value| (header.clone(), value.trim().to_owned()))
            })
            .collect();
        visitor(item)
    })
}

fn visit_sheet_rows(
    archive: &mut ZipArchive<Cursor<&[u8]>>,
    path: &str,
    shared: &[String],
    context: &AdapterContext<'_>,
    mut visitor: impl FnMut(Vec<Option<String>>) -> Result<(), AdapterError>,
) -> Result<(), AdapterError> {
    context.cancellation.check()?;
    let mut file = archive.by_name(path)?;
    validate_zip_entry(&file, context)?;
    let _reservation = context.memory_budget.reserve(file.size())?;
    let mut reader = Reader::from_reader(std::io::BufReader::new(&mut file));
    reader.config_mut().check_end_names = true;
    let mut buffer = Vec::new();
    let mut row = None;
    let mut cell_column = 0_usize;
    let mut cell_type = String::new();
    let mut cell_value = String::new();
    let mut in_value = false;
    loop {
        context.cancellation.check()?;
        match reader.read_event_into(&mut buffer)? {
            Event::Start(element) => {
                let name = local_name(element.name().as_ref());
                match name.as_str() {
                    "row" => row = Some(Vec::new()),
                    "c" => {
                        cell_column = 0;
                        cell_type.clear();
                        cell_value.clear();
                        for attribute in element.attributes() {
                            let attribute = attribute?;
                            let key = local_name(attribute.key.as_ref());
                            let value = attribute
                                .decoded_and_normalized_value(
                                    XmlVersion::Implicit1_0,
                                    reader.decoder(),
                                )?
                                .into_owned();
                            if key == "r" {
                                cell_column = column_index(&value);
                            } else if key == "t" {
                                cell_type = value;
                            }
                        }
                    }
                    "v" | "t" => in_value = true,
                    _ => {}
                }
            }
            Event::Text(text) if in_value => {
                cell_value.push_str(&text.xml_content(XmlVersion::Implicit1_0)?);
            }
            Event::End(element) => match local_name(element.name().as_ref()).as_str() {
                "v" | "t" => in_value = false,
                "c" => {
                    if let Some(row) = row.as_mut() {
                        while row.len() <= cell_column {
                            row.push(None);
                        }
                        row[cell_column] = resolve_cell(&cell_type, &cell_value, shared);
                    }
                }
                "row" => {
                    if let Some(row) = row.take() {
                        visitor(row)?;
                    }
                }
                _ => {}
            },
            Event::Eof => break,
            _ => {}
        }
        buffer.clear();
    }
    Ok(())
}

fn process_entity(info: &BTreeMap<String, String>, source: &str) -> CanonicalEntity {
    let name = info
        .get("name")
        .cloned()
        .unwrap_or_else(|| "openLCA process".to_owned());
    let declared_id = info
        .get("uuid")
        .filter(|value| Uuid::parse_str(value).is_ok())
        .cloned();
    CanonicalEntity {
        entity_type: "processes".to_owned(),
        internal_id: declared_id
            .clone()
            .unwrap_or_else(|| stable_id(&format!("openlca-process-xlsx/{source}/{name}"))),
        external_id: declared_id,
        name: Some(name),
        category_path: info
            .get("category")
            .map(|value| split_path(value))
            .unwrap_or_default(),
        raw: Map::from_iter([(
            "description".to_owned(),
            Value::String(
                info.get("description")
                    .cloned()
                    .unwrap_or_else(|| "Imported from openLCA XLSX.".to_owned()),
            ),
        )]),
    }
}

fn flow_from_row(row: &BTreeMap<String, String>, source: &str) -> Option<CanonicalEntity> {
    let name = row.get("name")?.to_owned();
    let category = row.get("category").cloned().unwrap_or_default();
    let declared_id = row
        .get("uuid")
        .filter(|value| Uuid::parse_str(value).is_ok())
        .cloned();
    let mut flow_metadata = Map::from_iter([(
        "flowType".to_owned(),
        Value::String(flow_type(row.get("type").map(String::as_str)).to_owned()),
    )]);
    let mut name_parts = Map::new();
    for (source_field, target_field) in [
        ("treatment standards routes", "treatmentStandardsRoutes"),
        ("mix and location types", "mixAndLocationTypes"),
        ("flow properties", "flowProperties"),
    ] {
        if let Some(value) = row
            .get(source_field)
            .filter(|value| !value.trim().is_empty())
        {
            name_parts.insert(target_field.to_owned(), Value::String(value.clone()));
        }
    }
    if !name_parts.is_empty() {
        flow_metadata.insert("flowName".to_owned(), Value::Object(name_parts));
    }
    flow_metadata.insert(
        "sourceTrace".to_owned(),
        json!({"format": "openlca-process-xlsx", "sourceObject": "Flows row"}),
    );
    Some(CanonicalEntity {
        entity_type: "flows".to_owned(),
        internal_id: declared_id.clone().unwrap_or_else(|| {
            stable_id(&format!(
                "openlca-process-xlsx/flow/{source}/{name}/{category}"
            ))
        }),
        external_id: Some(flow_key(&name, &category)),
        name: Some(name),
        category_path: split_path(&category),
        raw: flow_metadata,
    })
}

fn exchange_from_row(
    row: &BTreeMap<String, String>,
    source: &str,
    is_input: bool,
    index: usize,
    store: &CanonicalStore,
) -> Result<Option<FlowExchange>, AdapterError> {
    let Some(name) = row.get("flow") else {
        return Ok(None);
    };
    let category = row.get("category").cloned().unwrap_or_default();
    let key = flow_key(name, &category);
    let mut flow = store
        .get_by_external_id("flows", &key)?
        .unwrap_or_else(|| CanonicalEntity {
            entity_type: "flows".to_owned(),
            internal_id: stable_id(&format!(
                "openlca-process-xlsx/exchange-flow/{source}/{name}/{category}"
            )),
            external_id: Some(key),
            name: Some(name.clone()),
            category_path: split_path(&category),
            raw: Map::from_iter([(
                "flowType".to_owned(),
                Value::String("PRODUCT_FLOW".to_owned()),
            )]),
        });
    if let Some(unit) = row.get("unit") {
        flow.raw
            .insert("unitName".to_owned(), Value::String(unit.clone()));
    }
    let mut exchange = Map::from_iter([
        ("internalId".to_owned(), json!(index)),
        (
            "flow".to_owned(),
            json!({"@id": flow.internal_id, "name": flow.name}),
        ),
        (
            "flowRefId".to_owned(),
            Value::String(flow.internal_id.clone()),
        ),
        (
            "flowName".to_owned(),
            Value::String(flow.name.clone().unwrap_or_else(|| "Flow".to_owned())),
        ),
        ("isInput".to_owned(), Value::Bool(is_input)),
        (
            "amount".to_owned(),
            Value::String(row.get("amount").cloned().unwrap_or_else(|| "0".to_owned())),
        ),
    ]);
    if row.get("is reference?").is_some_and(|value| truthy(value)) {
        exchange.insert("isQuantitativeReference".to_owned(), Value::Bool(true));
    }
    if let Some(unit) = row.get("unit") {
        exchange.insert("unitName".to_owned(), Value::String(unit.clone()));
    }
    Ok(Some((flow, exchange)))
}

struct BudgetedEntry {
    bytes: Vec<u8>,
    reservation: MemoryReservation,
}

fn read_entry(
    archive: &mut ZipArchive<Cursor<&[u8]>>,
    name: &str,
    context: &AdapterContext<'_>,
    multiplier: u64,
) -> Result<BudgetedEntry, AdapterError> {
    context.cancellation.check()?;
    let mut file = archive.by_name(name)?;
    validate_zip_entry(&file, context)?;
    let accounted =
        file.size()
            .checked_mul(multiplier)
            .ok_or(tidas_runtime::RuntimeError::BudgetExceeded {
                requested: file.size(),
                used: context.memory_budget.used(),
                limit: context.memory_budget.limit(),
            })?;
    let reservation = context.memory_budget.reserve(accounted)?;
    let mut bytes = Vec::with_capacity(usize::try_from(file.size()).map_err(|_| {
        std::io::Error::new(std::io::ErrorKind::InvalidData, "XLSX entry is too large")
    })?);
    file.read_to_end(&mut bytes)?;
    Ok(BudgetedEntry { bytes, reservation })
}

fn validate_zip_entry(
    file: &zip::read::ZipFile<'_, Cursor<&[u8]>>,
    context: &AdapterContext<'_>,
) -> Result<(), AdapterError> {
    if file.enclosed_name().is_none() || file.is_symlink() {
        return Err(AdapterError::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "unsafe XLSX entry",
        )));
    }
    if file.size() > context.max_entry_bytes {
        return Err(AdapterError::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "XLSX entry exceeds configured size limit",
        )));
    }
    Ok(())
}

fn resolve_xl_target(target: &str) -> Result<String, AdapterError> {
    let target = target.trim_start_matches('/');
    let path = if target.starts_with("xl/") {
        target.to_owned()
    } else {
        format!("xl/{target}")
    };
    if path.split('/').any(|part| part == "..") {
        return Err(AdapterError::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "unsafe XLSX relationship target",
        )));
    }
    Ok(path)
}

fn resolve_cell(cell_type: &str, value: &str, shared: &[String]) -> Option<String> {
    let value = value.trim();
    if value.is_empty() {
        return None;
    }
    if cell_type == "s" {
        return value
            .parse::<usize>()
            .ok()
            .and_then(|index| shared.get(index))
            .cloned();
    }
    if cell_type == "b" {
        return Some((value == "1").to_string());
    }
    Some(value.to_owned())
}

fn column_index(reference: &str) -> usize {
    reference
        .chars()
        .take_while(char::is_ascii_alphabetic)
        .fold(0_usize, |value, character| {
            value
                .saturating_mul(26)
                .saturating_add((character.to_ascii_uppercase() as usize).saturating_sub(64))
        })
        .saturating_sub(1)
}

fn local_name(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes)
        .rsplit(':')
        .next()
        .unwrap_or_default()
        .to_owned()
}

fn flow_key(name: &str, category: &str) -> String {
    format!(
        "{}|{}",
        name.trim().to_ascii_lowercase(),
        category.trim().to_ascii_lowercase()
    )
}

fn flow_type(value: Option<&str>) -> &'static str {
    match value
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase()
        .chars()
        .next()
    {
        Some('w') => "WASTE_FLOW",
        Some('p') => "PRODUCT_FLOW",
        _ => "ELEMENTARY_FLOW",
    }
}

fn split_path(value: &str) -> Vec<String> {
    value
        .split('/')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

fn truthy(value: &str) -> bool {
    matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "true" | "yes" | "y" | "1" | "x"
    )
}

fn stable_id(seed: &str) -> String {
    Uuid::new_v5(&Uuid::NAMESPACE_URL, seed.as_bytes()).to_string()
}

fn xml_node_io(error: impl std::fmt::Display) -> AdapterError {
    AdapterError::Io(std::io::Error::new(
        std::io::ErrorKind::InvalidData,
        error.to_string(),
    ))
}

#[cfg(test)]
mod tests {
    use std::fmt::Write as _;
    use std::fs::File;
    use std::io::Write;

    use tempfile::tempdir;
    use tidas_runtime::{CancellationToken, MemoryBudget};
    use tidas_validation::{ValidationRequest, validate_tidas_package};
    use zip::write::SimpleFileOptions;

    use super::*;
    use crate::report::IssueSpool;
    use crate::writers::{TidasWriteRequest, write_tidas_package};

    #[test]
    #[allow(clippy::too_many_lines)]
    fn process_workbook_is_streamed_into_a_valid_package() {
        let directory = tempdir().unwrap();
        let source = directory.path().join("process.xlsx");
        let mut archive = zip::ZipWriter::new(File::create(&source).unwrap());
        let options = SimpleFileOptions::default();
        write_entry(
            &mut archive,
            options,
            "xl/workbook.xml",
            r#"<workbook xmlns:r="relationships"><sheets><sheet name="General information" r:id="rId1"/><sheet name="Flows" r:id="rId2"/><sheet name="Outputs" r:id="rId3"/></sheets></workbook>"#,
        );
        write_entry(
            &mut archive,
            options,
            "xl/_rels/workbook.xml.rels",
            r#"<Relationships><Relationship Id="rId1" Target="worksheets/sheet1.xml"/><Relationship Id="rId2" Target="worksheets/sheet2.xml"/><Relationship Id="rId3" Target="worksheets/sheet3.xml"/></Relationships>"#,
        );
        write_entry(
            &mut archive,
            options,
            "xl/worksheets/sheet1.xml",
            &sheet(&[
                &["General information"],
                &["uuid", "22222222-2222-4222-8222-222222222222"],
                &["name", "Steel production"],
                &["description", "XLSX fixture"],
            ]),
        );
        write_entry(
            &mut archive,
            options,
            "xl/worksheets/sheet2.xml",
            &sheet(&[
                &[
                    "name",
                    "category",
                    "type",
                    "uuid",
                    "treatment standards routes",
                    "mix and location types",
                ],
                &[
                    "Steel",
                    "Products/Metals",
                    "Product flow",
                    "11111111-1111-4111-8111-111111111111",
                    "production route",
                    "GLO",
                ],
            ]),
        );
        write_entry(
            &mut archive,
            options,
            "xl/worksheets/sheet3.xml",
            &sheet(&[
                &["flow", "category", "amount", "unit", "is reference?"],
                &["Steel", "Products/Metals", "1", "kg", "true"],
            ]),
        );
        archive.finish().unwrap();

        let cancellation = CancellationToken::default();
        let memory_budget = MemoryBudget::new(16 * 1024 * 1024);
        let mut store = CanonicalStore::create(Some(directory.path())).unwrap();
        let mut issues = IssueSpool::new(Vec::new(), 64 * 1024);
        OpenLcaProcessXlsxAdapter
            .read(
                &AdapterContext {
                    source: &source,
                    cancellation: &cancellation,
                    memory_budget: &memory_budget,
                    max_entry_bytes: 1024 * 1024,
                },
                &mut store,
                &mut issues,
            )
            .unwrap();
        issues.finish().unwrap();
        assert_eq!(store.counts()["processes"], 1);
        assert_eq!(store.counts()["flows"], 1);
        assert_eq!(
            store
                .iter_process_exchanges("22222222-2222-4222-8222-222222222222")
                .unwrap()
                .count(),
            1
        );
        let output = directory.path().join("tidas");
        write_tidas_package(&TidasWriteRequest {
            store: &store,
            output_dir: &output,
            cancellation: &cancellation,
            memory_budget: &memory_budget,
        })
        .unwrap();
        let validation = validate_tidas_package(&ValidationRequest {
            input_dir: output,
            issue_spool: None,
            cancellation,
            memory_budget,
            queue_capacity: 2,
            progress: None,
        })
        .unwrap();
        assert!(validation.summary.ok, "{:?}", validation.summary);
    }

    fn write_entry(
        archive: &mut zip::ZipWriter<File>,
        options: SimpleFileOptions,
        name: &str,
        content: &str,
    ) {
        archive.start_file(name, options).unwrap();
        archive.write_all(content.as_bytes()).unwrap();
    }

    fn sheet(rows: &[&[&str]]) -> String {
        let mut output = String::from("<worksheet><sheetData>");
        for (row_index, row) in rows.iter().enumerate() {
            output.push_str("<row>");
            for (column, value) in row.iter().enumerate() {
                let reference = format!("{}{}", column_name(column), row_index + 1);
                write!(
                    output,
                    r#"<c r="{reference}" t="inlineStr"><is><t>{value}</t></is></c>"#
                )
                .unwrap();
            }
            output.push_str("</row>");
        }
        output.push_str("</sheetData></worksheet>");
        output
    }

    fn column_name(mut index: usize) -> String {
        index = index.saturating_add(1);
        let mut output = String::new();
        while index > 0 {
            let remainder = (index - 1) % 26;
            output.insert(
                0,
                char::from_u32(u32::try_from(remainder + 65).unwrap()).unwrap(),
            );
            index = (index - 1) / 26;
        }
        output
    }
}
