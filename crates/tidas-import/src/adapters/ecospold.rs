use std::collections::BTreeMap;

use serde_json::{Map, Value, json};
use uuid::Uuid;

use super::xml_node::XmlNode;
use super::{AdapterContext, AdapterError, SourceAdapter};
use crate::detect::SourceFormat;
use crate::model::CanonicalEntity;
use crate::report::{ImportIssue, IssueSeverity, IssueSink};
use crate::source::{SourceReadRequest, visit_source_entries};
use crate::store::CanonicalStore;

pub struct EcoSpold1Adapter;
pub struct EcoSpold2Adapter;

impl SourceAdapter for EcoSpold1Adapter {
    fn format(&self) -> SourceFormat {
        SourceFormat::Ecospold1
    }

    fn read(
        &self,
        context: &AdapterContext<'_>,
        store: &mut CanonicalStore,
        issues: &mut dyn IssueSink,
    ) -> Result<(), AdapterError> {
        read_ecospold(context, store, issues, EcoSpoldVersion::One)
    }
}

impl SourceAdapter for EcoSpold2Adapter {
    fn format(&self) -> SourceFormat {
        SourceFormat::Ecospold2
    }

    fn read(
        &self,
        context: &AdapterContext<'_>,
        store: &mut CanonicalStore,
        issues: &mut dyn IssueSink,
    ) -> Result<(), AdapterError> {
        read_ecospold(context, store, issues, EcoSpoldVersion::Two)
    }
}

#[derive(Clone, Copy)]
enum EcoSpoldVersion {
    One,
    Two,
}

impl EcoSpoldVersion {
    fn format(self) -> &'static str {
        match self {
            Self::One => "ecospold1",
            Self::Two => "ecospold2",
        }
    }
}

fn read_ecospold(
    context: &AdapterContext<'_>,
    store: &mut CanonicalStore,
    issues: &mut dyn IssueSink,
    version: EcoSpoldVersion,
) -> Result<(), AdapterError> {
    let request = SourceReadRequest {
        source: context.source,
        allowed_extensions: &["spold", "xml"],
        max_entry_bytes: context.max_entry_bytes,
        cancellation: context.cancellation,
        memory_budget: context.memory_budget,
    };
    let mut process_count = 0_u64;
    visit_source_entries(&request, |entry| {
        let _structured_reservation = context.reserve_structured_expansion(entry.bytes.len(), 6)?;
        let root = match XmlNode::parse(entry.bytes) {
            Ok(root) => root,
            Err(error) => {
                issues.push(&ImportIssue {
                    severity: IssueSeverity::Error,
                    code: "invalid_ecospold_xml".to_owned(),
                    message: format!("Skipped invalid EcoSpold XML: {error}"),
                    source_object: Some(entry.label.clone()),
                    context: BTreeMap::new(),
                })?;
                return Ok::<(), AdapterError>(());
            }
        };
        let datasets = datasets(&root, version);
        for (index, dataset) in datasets.iter().enumerate() {
            let process = process_entity(dataset, &entry.stable_key, index, version);
            store.begin_process_exchanges(&process.internal_id)?;
            for (exchange_index, element) in exchange_nodes(dataset, version).enumerate() {
                let flow = flow_entity(element, &entry.stable_key, exchange_index, version);
                super::generated_units::add_for_flow(store, &flow)?;
                store.add(&flow)?;
                let exchange = exchange_value(element, &flow, exchange_index, version);
                store.add_process_exchange(&process.internal_id, &exchange)?;
            }
            store.add(&process)?;
            process_count = process_count.saturating_add(1);
            issues.push(&ImportIssue {
                severity: IssueSeverity::Warning,
                code: format!("{}_mapping_with_trace", version.format()),
                message: format!("Mapped native {} activity dataset.", version.format()),
                source_object: Some(entry.label.clone()),
                context: BTreeMap::from([(
                    "process_id".to_owned(),
                    Value::String(process.internal_id),
                )]),
            })?;
        }
        Ok::<(), AdapterError>(())
    })?;
    if process_count == 0 {
        issues.push(&ImportIssue {
            severity: IssueSeverity::Error,
            code: format!("no_{}_datasets", version.format()),
            message: format!("No {} datasets were found.", version.format()),
            source_object: None,
            context: BTreeMap::new(),
        })?;
    }
    Ok(())
}

fn datasets(root: &XmlNode, version: EcoSpoldVersion) -> Vec<&XmlNode> {
    let names: &[&str] = match version {
        EcoSpoldVersion::One => &["dataset", "dataSet"],
        EcoSpoldVersion::Two => &["activityDataset", "childActivityDataset"],
    };
    let mut result = names
        .iter()
        .flat_map(|name| root.descendants_named(name))
        .collect::<Vec<_>>();
    if result.is_empty() && names.contains(&root.name.as_str()) {
        result.push(root);
    }
    result
}

fn exchange_nodes<'a>(
    dataset: &'a XmlNode,
    version: EcoSpoldVersion,
) -> Box<dyn Iterator<Item = &'a XmlNode> + 'a> {
    match version {
        EcoSpoldVersion::One => Box::new(dataset.descendants_named("exchange")),
        EcoSpoldVersion::Two => Box::new(
            dataset
                .descendants_named("intermediateExchange")
                .chain(dataset.descendants_named("elementaryExchange")),
        ),
    }
}

fn process_entity(
    dataset: &XmlNode,
    label: &str,
    index: usize,
    version: EcoSpoldVersion,
) -> CanonicalEntity {
    let activity = dataset.first_descendant("activity");
    let reference = dataset.first_descendant("referenceFunction");
    let name = match version {
        EcoSpoldVersion::One => reference
            .and_then(|node| node.attr("name"))
            .or_else(|| dataset.attr("name")),
        EcoSpoldVersion::Two => activity
            .and_then(|node| node.child_text("activityName"))
            .or_else(|| activity.and_then(|node| node.attr("name"))),
    }
    .map_or_else(
        || format!("{} process {}", version.format(), index.saturating_add(1)),
        ToOwned::to_owned,
    );
    let declared_id = activity
        .and_then(|node| node.attr("id"))
        .filter(|value| Uuid::parse_str(value).is_ok())
        .or_else(|| uuid_in_label(label));
    let internal_id = declared_id.map_or_else(
        || {
            stable_id(&format!(
                "{}/process/{label}/{index}/{name}",
                version.format()
            ))
        },
        ToOwned::to_owned,
    );
    let mut raw = Map::from_iter([(
        "description".to_owned(),
        Value::String(format!(
            "Imported from {} source {label}.",
            version.format()
        )),
    )]);
    if let Some(location) = dataset
        .first_descendant("geography")
        .and_then(|node| node.attr("location"))
        .or_else(|| {
            dataset
                .first_descendant("geography")
                .and_then(|node| node.child_text("shortname"))
        })
    {
        raw.insert("location".to_owned(), Value::String(location.to_owned()));
    }
    if let Some(year) = dataset
        .first_descendant("timePeriod")
        .and_then(|node| {
            node.attr("startDate")
                .or_else(|| node.child_text("startDate"))
        })
        .and_then(year)
    {
        raw.insert("referenceYear".to_owned(), json!(year));
    }
    CanonicalEntity {
        entity_type: "processes".to_owned(),
        internal_id,
        external_id: declared_id.map(ToOwned::to_owned),
        name: Some(name),
        category_path: Vec::new(),
        raw,
    }
}

fn flow_entity(
    element: &XmlNode,
    label: &str,
    index: usize,
    version: EcoSpoldVersion,
) -> CanonicalEntity {
    let name = element
        .attr("name")
        .or_else(|| element.child_text("name"))
        .map_or_else(
            || format!("{} exchange {}", version.format(), index.saturating_add(1)),
            ToOwned::to_owned,
        );
    let declared_id = element
        .attr("id")
        .filter(|value| Uuid::parse_str(value).is_ok());
    let flow_type = if element.name == "elementaryExchange"
        || element.attr("outputGroup") == Some("4")
        || element.child_text("outputGroup") == Some("4")
        || element.attr("category").is_some_and(|category| {
            let category = category.to_ascii_lowercase();
            category.contains("air")
                || category.contains("water")
                || category.contains("soil")
                || category.contains("resource")
        }) {
        "ELEMENTARY_FLOW"
    } else {
        "PRODUCT_FLOW"
    };
    let mut raw = Map::from_iter([("flowType".to_owned(), Value::String(flow_type.to_owned()))]);
    if let Some(unit) = element
        .attr("unit")
        .or_else(|| element.child_text("unitName"))
    {
        raw.insert("unitName".to_owned(), Value::String(unit.to_owned()));
    }
    if let Some(cas) = element
        .attr("CASNumber")
        .or_else(|| element.child_text("CASNumber"))
    {
        raw.insert("CASNumber".to_owned(), Value::String(cas.to_owned()));
    }
    if let Some(formula) = element.child_text("sumFormula") {
        raw.insert("sumFormula".to_owned(), Value::String(formula.to_owned()));
    }
    CanonicalEntity {
        entity_type: "flows".to_owned(),
        internal_id: declared_id.map_or_else(
            || {
                stable_id(&format!(
                    "{}/flow/{label}/{index}/{name}/{flow_type}",
                    version.format()
                ))
            },
            ToOwned::to_owned,
        ),
        external_id: declared_id.map(ToOwned::to_owned),
        name: Some(name),
        category_path: element
            .attr("category")
            .into_iter()
            .chain(element.attr("subCategory"))
            .map(ToOwned::to_owned)
            .collect(),
        raw,
    }
}

fn exchange_value(
    element: &XmlNode,
    flow: &CanonicalEntity,
    index: usize,
    version: EcoSpoldVersion,
) -> Map<String, Value> {
    let is_input = element.attr("inputGroup").is_some()
        || element.child("inputGroup").is_some()
        || element
            .attr("category")
            .is_some_and(|category| category.to_ascii_lowercase().contains("resource"));
    let amount = element
        .attr("amount")
        .or_else(|| element.attr("meanValue"))
        .or_else(|| element.child_text("amount"))
        .unwrap_or("0");
    let unit = element
        .attr("unit")
        .or_else(|| element.child_text("unitName"));
    let mut value = Map::from_iter([
        ("internalId".to_owned(), json!(index.saturating_add(1))),
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
        ("amount".to_owned(), Value::String(amount.to_owned())),
        (
            "sourceFormat".to_owned(),
            Value::String(version.format().to_owned()),
        ),
    ]);
    if let Some(unit) = unit {
        value.insert("unitName".to_owned(), Value::String(unit.to_owned()));
    }
    value
}

fn year(value: &str) -> Option<u32> {
    value
        .split(|character: char| !character.is_ascii_digit())
        .find(|part| part.len() == 4)
        .and_then(|part| part.parse().ok())
}

fn uuid_in_label(label: &str) -> Option<&str> {
    label
        .split(|character: char| !(character.is_ascii_hexdigit() || character == '-'))
        .find(|part| Uuid::parse_str(part).is_ok())
}

fn stable_id(seed: &str) -> String {
    Uuid::new_v5(&Uuid::NAMESPACE_URL, seed.as_bytes()).to_string()
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;
    use tidas_runtime::{CancellationToken, MemoryBudget};
    use tidas_validation::{ValidationRequest, validate_tidas_package};

    use super::*;
    use crate::report::IssueSpool;
    use crate::writers::{TidasWriteRequest, write_tidas_package};

    #[test]
    fn both_ecospold_versions_write_valid_tidas() {
        for (version, file_name, xml) in [
            (
                EcoSpoldVersion::One,
                "one.xml",
                r#"<ecoSpold><dataset><metaInformation><processInformation><referenceFunction name="steel"/></processInformation></metaInformation><flowData><exchange number="1" name="steel" unit="kg" amount="1" outputGroup="0"/><exchange number="2" name="carbon dioxide" unit="kg" amount="2.5" outputGroup="4" category="air"/></flowData></dataset></ecoSpold>"#,
            ),
            (
                EcoSpoldVersion::Two,
                "two.spold",
                r#"<ecoSpold><activityDataset><activityDescription><activity id="22222222-2222-4222-8222-222222222222"><activityName>steel</activityName></activity></activityDescription><flowData><intermediateExchange id="11111111-1111-4111-8111-111111111111" amount="1"><name>steel</name><unitName>kg</unitName><outputGroup>0</outputGroup></intermediateExchange><elementaryExchange id="44444444-4444-4444-8444-444444444444" amount="2.5"><name>carbon dioxide</name><unitName>kg</unitName><outputGroup>4</outputGroup></elementaryExchange></flowData></activityDataset></ecoSpold>"#,
            ),
        ] {
            let directory = tempdir().unwrap();
            let source = directory.path().join(file_name);
            std::fs::write(&source, xml).unwrap();
            let cancellation = CancellationToken::default();
            let memory_budget = MemoryBudget::new(16 * 1024 * 1024);
            let mut store = CanonicalStore::create(Some(directory.path())).unwrap();
            let mut issues = IssueSpool::new(Vec::new(), 64 * 1024);
            read_ecospold(
                &AdapterContext {
                    source: &source,
                    cancellation: &cancellation,
                    memory_budget: &memory_budget,
                    max_entry_bytes: 1024 * 1024,
                },
                &mut store,
                &mut issues,
                version,
            )
            .unwrap();
            issues.finish().unwrap();
            assert_eq!(store.counts()["processes"], 1);
            assert_eq!(store.counts()["flows"], 2);
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
                cancellation: cancellation.clone(),
                memory_budget: memory_budget.clone(),
                queue_capacity: 2,
                progress: None,
            })
            .unwrap();
            assert!(validation.summary.ok, "{:?}", validation.summary);
        }
    }
}
