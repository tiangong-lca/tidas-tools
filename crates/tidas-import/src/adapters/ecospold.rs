use std::collections::{BTreeMap, BTreeSet};

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
    let mut used_process_ids = BTreeSet::new();
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
            for source in source_entities(dataset, version) {
                store.add(&source)?;
            }
            let process = process_entity(
                dataset,
                &root,
                &entry.stable_key,
                index,
                version,
                &mut used_process_ids,
            );
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

fn source_entities(dataset: &XmlNode, version: EcoSpoldVersion) -> Vec<CanonicalEntity> {
    if let EcoSpoldVersion::Two = version {
        return Vec::new();
    }
    dataset
        .descendants_named("source")
        .map(|source| {
            let source_number = source.attr("sourceNumber");
            let title = source.attr("title");
            let author = source.attr("firstAuthor");
            let year = source.attr("year");
            let text = source.attr("text");
            let name = title
                .or(author)
                .or(source_number)
                .unwrap_or("EcoSpold 1 source");
            let seed = [source_number, title, author, year, text]
                .into_iter()
                .flatten()
                .collect::<Vec<_>>()
                .join("\u{1f}");
            let citation = [author, title, year]
                .into_iter()
                .flatten()
                .collect::<Vec<_>>()
                .join(", ");
            let mut raw = Map::from_iter([
                ("shortName".to_owned(), Value::String(name.to_owned())),
                (
                    "sourceCitation".to_owned(),
                    Value::String(if citation.is_empty() {
                        text.unwrap_or(name).to_owned()
                    } else {
                        citation
                    }),
                ),
                (
                    "sourceTrace".to_owned(),
                    json!({
                        "format": "ecospold1",
                        "sourceObject": "source",
                        "source": source.trace(&[])
                    }),
                ),
            ]);
            if let Some(description) = text {
                raw.insert(
                    "description".to_owned(),
                    Value::String(description.to_owned()),
                );
            }
            CanonicalEntity {
                entity_type: "sources".to_owned(),
                internal_id: stable_id(&format!("ecospold1/source/{seed}")),
                external_id: source_number.map(ToOwned::to_owned),
                name: Some(name.to_owned()),
                category_path: Vec::new(),
                raw,
            }
        })
        .collect()
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
    root: &XmlNode,
    label: &str,
    index: usize,
    version: EcoSpoldVersion,
    used_process_ids: &mut BTreeSet<String>,
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
    let preferred_id = declared_id.map_or_else(
        || {
            stable_id(&format!(
                "{}/process/{label}/{index}/{name}",
                version.format()
            ))
        },
        ToOwned::to_owned,
    );
    let internal_id = unique_process_id(&preferred_id, label, index, &name, used_process_ids);
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
    apply_process_semantics(dataset, root, label, version, &mut raw, &internal_id);
    CanonicalEntity {
        entity_type: "processes".to_owned(),
        internal_id,
        external_id: declared_id.map(ToOwned::to_owned),
        name: Some(name),
        category_path: Vec::new(),
        raw,
    }
}

fn unique_process_id(
    preferred: &str,
    label: &str,
    index: usize,
    name: &str,
    used: &mut BTreeSet<String>,
) -> String {
    if used.insert(preferred.to_owned()) {
        return preferred.to_owned();
    }
    let mut suffix = 1_u64;
    loop {
        let candidate = stable_id(&format!(
            "ecospold2/process/{label}/{index}/{name}/{suffix}"
        ));
        if used.insert(candidate.clone()) {
            return candidate;
        }
        suffix = suffix.saturating_add(1);
    }
}

fn apply_process_semantics(
    dataset: &XmlNode,
    root: &XmlNode,
    label: &str,
    version: EcoSpoldVersion,
    raw: &mut Map<String, Value>,
    process_id: &str,
) {
    raw.insert(
        "sourceFormat".to_owned(),
        Value::String(version.format().to_owned()),
    );
    raw.insert("sourceLabel".to_owned(), Value::String(label.to_owned()));
    let time = dataset.first_descendant("timePeriod");
    let geography = dataset.first_descendant("geography");
    let technology = dataset.first_descendant("technology");
    let reference = dataset.first_descendant("referenceFunction");
    let source_classification = process_classification(dataset, version);
    if let Some(year) = time
        .and_then(|node| {
            node.attr("endDate")
                .or_else(|| node.child_text("endDate"))
                .or_else(|| node.attr("endYear"))
                .or_else(|| node.child_text("endYear"))
        })
        .and_then(year)
    {
        raw.insert("dataSetValidUntil".to_owned(), json!(year));
    }
    if let Some(description) = time_description(time) {
        raw.insert(
            "timeDescription".to_owned(),
            Value::String(description.clone()),
        );
        raw.insert(
            "dataCollectionPeriod".to_owned(),
            Value::String(description),
        );
    }
    if let Some(description) = node_description(geography) {
        raw.insert("locationDescription".to_owned(), Value::String(description));
    }
    if let Some(description) = node_description(technology).or_else(|| {
        reference
            .and_then(|node| declared(node.attr("includedProcesses")))
            .map(ToOwned::to_owned)
    }) {
        raw.insert(
            "technologyDescription".to_owned(),
            Value::String(description),
        );
    }
    if let Some(reference) = reference {
        copy_declared_attr(
            reference,
            "includedProcesses",
            raw,
            "dataCutOffAndCompletenessPrinciples",
        );
    }
    if let Some(representativeness) = dataset.first_descendant("representativeness") {
        for (source, target) in [
            ("productionVolume", "productionVolume"),
            ("samplingProcedure", "samplingProcedure"),
            ("uncertaintyAdjustments", "uncertaintyAdjustments"),
            ("extrapolations", "useAdviceForDataSet"),
        ] {
            copy_declared_attr(representativeness, source, raw, target);
        }
    }
    raw.insert(
        "sourceClassification".to_owned(),
        source_classification.clone(),
    );
    raw.insert(
        "sourceTrace".to_owned(),
        json!({
            "format": version.format(),
            "sourceObject": label,
            "sourceIdentifiers": {
                "activityId": dataset.first_descendant("activity").and_then(|node| node.attr("id")),
                "selectedProcessId": process_id
            },
            "sourceClassification": source_classification,
            "rootAttributes": root.trace(&["activityDataset", "childActivityDataset", "dataset", "dataSet"]),
            "dataset": dataset.trace(&["flowData"])
        }),
    );
}

fn process_classification(dataset: &XmlNode, version: EcoSpoldVersion) -> Value {
    match version {
        EcoSpoldVersion::One => {
            let reference = dataset.first_descendant("referenceFunction");
            json!({
                "category": reference.and_then(|node| node.attr("category")),
                "subCategory": reference.and_then(|node| node.attr("subCategory")),
                "localCategory": reference.and_then(|node| node.attr("localCategory")),
                "localSubCategory": reference.and_then(|node| node.attr("localSubCategory"))
            })
        }
        EcoSpoldVersion::Two => Value::Array(
            dataset
                .first_descendant("activityDescription")
                .into_iter()
                .flat_map(|description| &description.children)
                .filter(|child| child.name == "classification")
                .map(classification_trace)
                .collect(),
        ),
    }
}

fn classification_trace(classification: &XmlNode) -> Value {
    json!({
        "classificationId": classification.attr("classificationId"),
        "classificationSystem": classification
            .descendants_named("classificationSystem")
            .filter_map(XmlNode::trimmed_text)
            .collect::<Vec<_>>(),
        "classificationValue": classification
            .descendants_named("classificationValue")
            .filter_map(XmlNode::trimmed_text)
            .collect::<Vec<_>>()
    })
}

fn node_description(node: Option<&XmlNode>) -> Option<String> {
    let node = node?;
    if let Some(value) = declared(node.attr("text")) {
        return Some(value.to_owned());
    }
    node.descendants_named("comment")
        .find_map(first_nested_text)
        .or_else(|| first_nested_text(node))
}

fn first_nested_text(node: &XmlNode) -> Option<String> {
    node.descendants_named("text")
        .find_map(XmlNode::trimmed_text)
        .or_else(|| node.trimmed_text())
        .map(ToOwned::to_owned)
}

fn time_description(node: Option<&XmlNode>) -> Option<String> {
    let node = node?;
    if let Some(text) = node.trimmed_text() {
        return Some(text.to_owned());
    }
    let start = node
        .attr("startDate")
        .or_else(|| node.child_text("startDate"))
        .or_else(|| node.attr("startYear"))
        .or_else(|| node.child_text("startYear"));
    let end = node
        .attr("endDate")
        .or_else(|| node.child_text("endDate"))
        .or_else(|| node.attr("endYear"))
        .or_else(|| node.child_text("endYear"));
    let comment = node
        .descendants_named("comment")
        .find_map(first_nested_text);
    let mut parts = Vec::new();
    match (start, end) {
        (Some(start), Some(end)) => parts.push(format!("{start} - {end}")),
        (Some(start), None) => parts.push(start.to_owned()),
        (None, Some(end)) => parts.push(end.to_owned()),
        (None, None) => {}
    }
    if let Some(comment) = comment {
        parts.push(comment);
    }
    (!parts.is_empty()).then(|| parts.join("; "))
}

fn declared(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| {
        !value.is_empty()
            && !matches!(
                value.to_ascii_lowercase().as_str(),
                "<null>" | "null" | "na" | "n/a"
            )
    })
}

fn copy_declared_attr(node: &XmlNode, source: &str, raw: &mut Map<String, Value>, target: &str) {
    if let Some(value) = declared(node.attr(source)) {
        raw.insert(target.to_owned(), Value::String(value.to_owned()));
    }
}

fn flow_entity(
    element: &XmlNode,
    _label: &str,
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
    if let Some(formula) = element
        .attr("formula")
        .or_else(|| element.child_text("sumFormula"))
    {
        raw.insert("sumFormula".to_owned(), Value::String(formula.to_owned()));
    }
    if let Some(synonyms) = declared(element.attr("localName")) {
        raw.insert("synonyms".to_owned(), Value::String(synonyms.to_owned()));
    }
    let source_classification = exchange_classification(element);
    raw.insert(
        "sourceClassification".to_owned(),
        source_classification.clone(),
    );
    raw.insert(
        "sourceTrace".to_owned(),
        json!({
            "format": version.format(),
            "sourceObject": "exchange",
            "sourceIdentifiers": exchange_identifiers(element),
            "sourceClassification": source_classification,
            "exchange": element.trace(&[])
        }),
    );
    let stable_seed = format!(
        "{}/flow/{flow_type}/{name}/{}/{}/{}/{}",
        version.format(),
        element.attr("CASNumber").unwrap_or_default(),
        element
            .attr("formula")
            .or_else(|| element.child_text("sumFormula"))
            .unwrap_or_default(),
        element.attr("category").unwrap_or_default(),
        element
            .attr("unit")
            .or_else(|| element.child_text("unitName"))
            .unwrap_or_default()
    );
    CanonicalEntity {
        entity_type: "flows".to_owned(),
        internal_id: declared_id.map_or_else(|| stable_id(&stable_seed), ToOwned::to_owned),
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
    apply_exchange_semantics(element, version, &mut value);
    value
}

fn apply_exchange_semantics(
    element: &XmlNode,
    version: EcoSpoldVersion,
    value: &mut Map<String, Value>,
) {
    for (source, target) in [
        ("minValue", "minimumAmount"),
        ("minAmount", "minimumAmount"),
        ("maxValue", "maximumAmount"),
        ("maxAmount", "maximumAmount"),
        ("standardDeviation95", "relativeStandardDeviation95In"),
        ("activityLinkId", "activityLinkId"),
        ("productionVolumeAmount", "productionVolumeAmount"),
        ("unitId", "unitId"),
    ] {
        if !value.contains_key(target)
            && let Some(field) = declared(element.attr(source))
        {
            value.insert(target.to_owned(), Value::String(field.to_owned()));
        }
    }
    let source_id = match version {
        EcoSpoldVersion::One => element.attr("number"),
        EcoSpoldVersion::Two => element
            .attr("id")
            .or_else(|| element.attr("intermediateExchangeId"))
            .or_else(|| element.attr("elementaryExchangeId")),
    };
    if let Some(source_id) = source_id {
        value.insert(
            match version {
                EcoSpoldVersion::One => "sourceExchangeNumber",
                EcoSpoldVersion::Two => "sourceExchangeId",
            }
            .to_owned(),
            Value::String(source_id.to_owned()),
        );
    }
    if element
        .attr("isCalculatedAmount")
        .is_some_and(|value| value.eq_ignore_ascii_case("true"))
    {
        value.insert(
            "dataDerivationTypeStatus".to_owned(),
            Value::String("Calculated".to_owned()),
        );
    }
    if let Some(kind) = uncertainty_type(element, version) {
        value.insert(
            "uncertaintyDistributionType".to_owned(),
            Value::String(kind.to_owned()),
        );
    }
    if let Some(comment) = element
        .attr("generalComment")
        .and_then(|value| declared(Some(value)))
        .map(ToOwned::to_owned)
        .or_else(|| {
            element
                .descendants_named("comment")
                .find_map(first_nested_text)
        })
    {
        value.insert("generalComment".to_owned(), Value::String(comment));
    }
    let source_classification = exchange_classification(element);
    value.insert(
        "sourceTrace".to_owned(),
        json!({
            "format": version.format(),
            "sourceObject": "exchange",
            "sourceIdentifiers": exchange_identifiers(element),
            "sourceClassification": source_classification,
            "exchange": element.trace(&[])
        }),
    );
}

fn uncertainty_type(element: &XmlNode, version: EcoSpoldVersion) -> Option<&'static str> {
    if let EcoSpoldVersion::One = version {
        return element.attr("uncertaintyType").and_then(|value| {
            match value.trim().to_ascii_lowercase().as_str() {
                "0" | "undefined" => Some("undefined"),
                "1" | "lognormal" | "log-normal" => Some("log-normal"),
                "2" | "normal" => Some("normal"),
                "3" | "triangular" => Some("triangular"),
                "4" | "uniform" => Some("uniform"),
                _ => None,
            }
        });
    }
    let uncertainty = element.child("uncertainty")?;
    for child in &uncertainty.children {
        match child.name.to_ascii_lowercase().as_str() {
            "lognormal" => return Some("log-normal"),
            "normal" => return Some("normal"),
            "triangular" => return Some("triangular"),
            "uniform" => return Some("uniform"),
            _ => {}
        }
    }
    None
}

fn exchange_identifiers(element: &XmlNode) -> Value {
    json!({
        "id": element.attr("id"),
        "intermediateExchangeId": element.attr("intermediateExchangeId"),
        "elementaryExchangeId": element.attr("elementaryExchangeId"),
        "activityLinkId": element.attr("activityLinkId"),
        "unitId": element.attr("unitId")
    })
}

fn exchange_classification(element: &XmlNode) -> Value {
    let classifications = element
        .descendants_named("classification")
        .map(classification_trace)
        .collect::<Vec<_>>();
    json!({
        "category": element.attr("category"),
        "subCategory": element.attr("subCategory"),
        "localCategory": element.attr("localCategory"),
        "localSubCategory": element.attr("localSubCategory"),
        "classifications": classifications
    })
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

    const ECOSPOLD1_RICH: &str = r#"<ecoSpold><dataset><metaInformation><processInformation>
      <referenceFunction name="steel" generalComment="note" category="metals" localCategory="local metals" includedProcesses="gate to gate"/>
      <geography location="CH" text="Swiss boundary"/>
      <technology text="electric arc"/>
      <timePeriod startDate="2020-01-01" endDate="2023-12-31">survey period</timePeriod>
      </processInformation><modellingAndValidation><representativeness productionVolume="123 kg" samplingProcedure="survey" uncertaintyAdjustments="pedigree" extrapolations="Swiss use"/>
      </modellingAndValidation><sources><source sourceNumber="10" firstAuthor="A. Author" year="2021" title="Steel source"/></sources></metaInformation>
      <flowData><exchange number="1" name="steel" unit="kg" amount="1" outputGroup="0"/>
      <exchange number="2" name="carbon dioxide" CASNumber="124-38-9" formula="CO2" localName="CO2 local" unit="kg" amount="2.5000000000000001" outputGroup="4" category="air" localCategory="local air" minValue="1.2" maxValue="3.4" uncertaintyType="1" standardDeviation95="12.3456" generalComment="measured"/>
      </flowData></dataset></ecoSpold>"#;

    const ECOSPOLD2_RICH: &str = r#"<ecoSpold><activityDataset><activityDescription>
      <activity id="22222222-2222-4222-8222-222222222222"><activityName>steel</activityName><generalComment><text>activity note</text></generalComment></activity>
      <classification classificationId="activity-class"><classificationSystem>activity classes</classificationSystem><classificationValue>energy systems</classificationValue></classification>
      <timePeriod><startDate>2024-01-01</startDate><endDate>2025-12-31</endDate><comment>temporal note</comment></timePeriod>
      <geography><shortname>CH</shortname><comment>Swiss geography</comment></geography>
      <technology><comment>heat pump route</comment></technology></activityDescription><flowData>
      <intermediateExchange id="11111111-1111-4111-8111-111111111111" intermediateExchangeId="11111111-1111-4111-8111-111111111111" amount="1" productionVolumeAmount="42" isCalculatedAmount="true"><name>steel</name><unitName>kg</unitName><outputGroup>0</outputGroup></intermediateExchange>
      <intermediateExchange id="33333333-3333-4333-8333-333333333333" amount="0.2" activityLinkId="aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa" isCalculatedAmount="true"><name>linked input</name><unitName>kg</unitName><classification classificationId="input-class"><classificationValue>linked class</classificationValue></classification><comment>linked comment</comment><uncertainty><lognormal variance="0.12"/></uncertainty><inputGroup>5</inputGroup></intermediateExchange>
      </flowData></activityDataset></ecoSpold>"#;

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
            let validation_issues = directory.path().join("validation-issues.jsonl");
            let validation = validate_tidas_package(&ValidationRequest {
                input_dir: output,
                issue_spool: Some(validation_issues.clone()),
                cancellation: cancellation.clone(),
                memory_budget: memory_budget.clone(),
                queue_capacity: 2,
                progress: None,
            })
            .unwrap();
            assert!(
                validation.summary.ok,
                "{:?}\n{}",
                validation.summary,
                std::fs::read_to_string(validation_issues).unwrap()
            );
        }
    }

    #[test]
    fn ecospold1_semantics_are_preserved_in_the_disk_store() {
        let directory = tempdir().unwrap();
        let source = directory
            .path()
            .join("process_64e926e8-dd48-3704-b902-daaf546087c4.xml");
        std::fs::write(&source, ECOSPOLD1_RICH).unwrap();
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
            EcoSpoldVersion::One,
        )
        .unwrap();
        let process = store
            .iter_type("processes")
            .unwrap()
            .next()
            .unwrap()
            .unwrap();
        assert_eq!(process.internal_id, "64e926e8-dd48-3704-b902-daaf546087c4");
        assert_eq!(process.raw["dataSetValidUntil"], 2023);
        assert_eq!(process.raw["technologyDescription"], "electric arc");
        assert_eq!(process.raw["productionVolume"], "123 kg");
        assert_eq!(
            process.raw["sourceClassification"]["localCategory"],
            "local metals"
        );
        assert_eq!(store.counts()["sources"], 1);
        let exchanges = store
            .iter_process_exchanges(&process.internal_id)
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(exchanges[1]["minimumAmount"], "1.2");
        assert_eq!(exchanges[1]["uncertaintyDistributionType"], "log-normal");
        assert_eq!(exchanges[1]["sourceExchangeNumber"], "2");
        let flow = store
            .iter_type("flows")
            .unwrap()
            .filter_map(Result::ok)
            .find(|flow| flow.name.as_deref() == Some("carbon dioxide"))
            .unwrap();
        assert_eq!(flow.raw["sumFormula"], "CO2");
        assert_eq!(flow.raw["synonyms"], "CO2 local");
    }

    #[test]
    fn ecospold2_semantics_are_preserved_in_the_disk_store() {
        let directory = tempdir().unwrap();
        let source = directory.path().join("rich.spold");
        std::fs::write(&source, ECOSPOLD2_RICH).unwrap();
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
            EcoSpoldVersion::Two,
        )
        .unwrap();
        let process = store
            .iter_type("processes")
            .unwrap()
            .next()
            .unwrap()
            .unwrap();
        assert_eq!(process.internal_id, "22222222-2222-4222-8222-222222222222");
        assert_eq!(process.raw["referenceYear"], 2024);
        assert_eq!(process.raw["dataSetValidUntil"], 2025);
        assert_eq!(process.raw["locationDescription"], "Swiss geography");
        assert_eq!(process.raw["technologyDescription"], "heat pump route");
        assert_eq!(
            process.raw["sourceClassification"][0]["classificationValue"][0],
            "energy systems"
        );
        let exchanges = store
            .iter_process_exchanges(&process.internal_id)
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(exchanges[1]["dataDerivationTypeStatus"], "Calculated");
        assert_eq!(exchanges[1]["uncertaintyDistributionType"], "log-normal");
        assert_eq!(
            exchanges[1]["sourceTrace"]["sourceIdentifiers"]["activityLinkId"],
            "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa"
        );
    }
}
