use std::collections::BTreeMap;
use std::str::FromStr;

use bigdecimal::{BigDecimal, RoundingMode};
use serde_json::{Map, Value, json};
use uuid::Uuid;

use super::{AdapterContext, AdapterError, SourceAdapter};
use crate::detect::SourceFormat;
use crate::model::CanonicalEntity;
use crate::report::{ImportIssue, IssueSeverity, IssueSink};
use crate::source::{SourceReadRequest, visit_source_entries};
use crate::store::CanonicalStore;

pub struct OpenLcaJsonLdAdapter;

impl SourceAdapter for OpenLcaJsonLdAdapter {
    fn format(&self) -> SourceFormat {
        SourceFormat::OpenlcaJsonld
    }

    fn read(
        &self,
        context: &AdapterContext<'_>,
        store: &mut CanonicalStore,
        issues: &mut dyn IssueSink,
    ) -> Result<(), AdapterError> {
        let request = SourceReadRequest {
            source: context.source,
            allowed_extensions: &["json", "jsonld"],
            max_entry_bytes: context.max_entry_bytes,
            cancellation: context.cancellation,
            memory_budget: context.memory_budget,
        };
        let mut supported = 0_u64;
        visit_source_entries(&request, |entry| {
            let _structured_reservation =
                context.reserve_structured_expansion(entry.bytes.len(), 4)?;
            let value: Value = match serde_json::from_slice(entry.bytes) {
                Ok(value) => value,
                Err(error) => {
                    issues.push(&ImportIssue {
                        severity: IssueSeverity::Error,
                        code: "invalid_openlca_jsonld".to_owned(),
                        message: format!("Skipped invalid openLCA JSON-LD: {error}"),
                        source_object: Some(entry.label.clone()),
                        context: BTreeMap::new(),
                    })?;
                    return Ok::<(), AdapterError>(());
                }
            };
            for object in objects(&value) {
                let Some(mut entity) = to_entity(object, &entry.stable_key) else {
                    continue;
                };
                if entity.entity_type == "flows" {
                    super::generated_units::add_for_flow(store, &entity)?;
                }
                if entity.entity_type == "processes" {
                    store.begin_process_exchanges(&entity.internal_id)?;
                    if let Some(exchanges) = entity
                        .raw
                        .remove("exchanges")
                        .and_then(|value| value.as_array().cloned())
                    {
                        for exchange in exchanges
                            .into_iter()
                            .filter_map(|value| value.as_object().cloned())
                        {
                            store.add_process_exchange(&entity.internal_id, &exchange)?;
                        }
                    }
                }
                let auxiliary = entity.entity_type == "dqsystems";
                store.add(&entity)?;
                if !auxiliary {
                    supported = supported.saturating_add(1);
                }
            }
            Ok::<(), AdapterError>(())
        })?;
        resolve_process_locations(store)?;
        resolve_process_data_quality(store)?;
        super::openlca_normalize::normalize_exchange_amounts(context, store, issues)?;
        if let Some(model) = provider_graph_lifecycle_model(store)? {
            store.add(&model)?;
            supported = supported.saturating_add(1);
        }
        if supported == 0 {
            issues.push(&ImportIssue {
                severity: IssueSeverity::Error,
                code: "no_openlca_jsonld_entities".to_owned(),
                message: "No supported openLCA JSON-LD entities were found.".to_owned(),
                source_object: None,
                context: BTreeMap::new(),
            })?;
        } else {
            issues.push(&ImportIssue {
                severity: IssueSeverity::Warning,
                code: "openlca_jsonld_mapping".to_owned(),
                message: "Mapped openLCA JSON-LD entities with native Rust rules.".to_owned(),
                source_object: None,
                context: BTreeMap::from([("entity_count".to_owned(), json!(supported))]),
            })?;
        }
        Ok(())
    }
}

fn objects(value: &Value) -> Box<dyn Iterator<Item = &Map<String, Value>> + '_> {
    match value {
        Value::Object(object) => Box::new(std::iter::once(object)),
        Value::Array(items) => Box::new(items.iter().filter_map(Value::as_object)),
        _ => Box::new(std::iter::empty()),
    }
}

fn to_entity(object: &Map<String, Value>, source: &str) -> Option<CanonicalEntity> {
    let object_type = text(object.get("@type"))?;
    let id = text(object.get("@id"))
        .filter(|value| !value.is_empty())
        .map_or_else(
            || stable_id(&format!("openlca/{object_type}/{source}/{}", name(object))),
            ToOwned::to_owned,
        );
    let external_id = text(object.get("@id")).map(ToOwned::to_owned);
    let (entity_type, raw) = match object_type {
        "UnitGroup" => ("unitgroups", unit_group_raw(object)),
        "FlowProperty" => ("flowproperties", flow_property_raw(object)),
        "Flow" => ("flows", flow_raw(object)),
        "Process" => ("processes", process_raw(object)),
        "Actor" => ("contacts", copy_fields(object, &["email", "description"])),
        "Source" => (
            "sources",
            copy_fields(object, &["textReference", "description", "url"]),
        ),
        "Location" => (
            "locations",
            copy_fields(object, &["code", "category", "description"]),
        ),
        "DQSystem" => ("dqsystems", copy_fields(object, &["indicators", "source"])),
        _ => return None,
    };
    Some(CanonicalEntity {
        entity_type: entity_type.to_owned(),
        internal_id: id,
        external_id,
        name: Some(name(object)),
        category_path: text(object.get("category"))
            .map(split_path)
            .unwrap_or_default(),
        raw,
    })
}

fn unit_group_raw(object: &Map<String, Value>) -> Map<String, Value> {
    Map::from_iter([(
        "units".to_owned(),
        object
            .get("units")
            .cloned()
            .unwrap_or_else(|| Value::Array(Vec::new())),
    )])
}

fn flow_property_raw(object: &Map<String, Value>) -> Map<String, Value> {
    let mut raw = Map::new();
    if let Some(unit_group) = object.get("unitGroup").and_then(Value::as_object) {
        copy_ref(unit_group, &mut raw, "unitGroupRefId", "unitGroupName");
    }
    raw
}

fn flow_raw(object: &Map<String, Value>) -> Map<String, Value> {
    let mut raw = Map::new();
    raw.insert(
        "flowType".to_owned(),
        Value::String(
            text(object.get("flowType"))
                .unwrap_or("PRODUCT_FLOW")
                .to_owned(),
        ),
    );
    for (source, target) in [("cas", "CASNumber"), ("formula", "sumFormula")] {
        if let Some(value) = object.get(source).cloned() {
            raw.insert(target.to_owned(), value);
        }
    }
    if let Some(properties) = object.get("flowProperties").cloned() {
        raw.insert("flowProperties".to_owned(), properties);
    }
    if let Some(property) = object
        .get("flowProperties")
        .and_then(Value::as_array)
        .and_then(|items| {
            items
                .iter()
                .find(|item| {
                    item.get("isRefFlowProperty")
                        .and_then(Value::as_bool)
                        .unwrap_or(false)
                })
                .or_else(|| items.first())
        })
        .and_then(|item| item.get("flowProperty"))
        .and_then(Value::as_object)
    {
        copy_ref(property, &mut raw, "flowPropertyRefId", "flowPropertyName");
    }
    if let Some(unit) = text(object.get("refUnit")) {
        raw.insert("unitName".to_owned(), Value::String(unit.to_owned()));
    }
    let mut name_parts = Map::new();
    for field in [
        "treatmentStandardsRoutes",
        "mixAndLocationTypes",
        "flowProperties",
    ] {
        if let Some(value) = object.get(field).and_then(Value::as_str) {
            name_parts.insert(field.to_owned(), Value::String(value.to_owned()));
        }
    }
    if !name_parts.is_empty() {
        raw.insert("flowName".to_owned(), Value::Object(name_parts));
    }
    raw.insert(
        "sourceTrace".to_owned(),
        json!({
            "format": "openlca-jsonld",
            "sourceObject": "Flow",
            "sourceId": object.get("@id")
        }),
    );
    raw
}

fn process_raw(object: &Map<String, Value>) -> Map<String, Value> {
    let mut exchanges = object
        .get("exchanges")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .enumerate()
                .filter_map(|(index, item)| exchange(item.as_object()?, index))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    apply_allocations(object, &mut exchanges);
    let mut raw = Map::from_iter([
        (
            "description".to_owned(),
            Value::String(
                text(object.get("description"))
                    .unwrap_or("Imported from openLCA JSON-LD.")
                    .to_owned(),
            ),
        ),
        ("exchanges".to_owned(), Value::Array(exchanges)),
        (
            "sourceTrace".to_owned(),
            json!({"format": "openlca-jsonld", "process": object}),
        ),
    ]);
    if let Some(process_type) = text(object.get("processType")) {
        raw.insert(
            "sourceProcessType".to_owned(),
            Value::String(process_type.to_owned()),
        );
    }
    for (source, target) in [
        ("defaultAllocationMethod", "sourceDefaultAllocationMethod"),
        ("version", "version"),
        ("lastChange", "lastChange"),
        ("allocationFactors", "allocationFactors"),
        ("dqEntry", "dqEntry"),
        ("dqSystem", "dqSystem"),
    ] {
        if let Some(value) = object.get(source).cloned() {
            raw.insert(target.to_owned(), value);
        }
    }
    apply_process_documentation(object, &mut raw);
    apply_process_location(object, &mut raw);
    raw
}

fn apply_process_documentation(object: &Map<String, Value>, raw: &mut Map<String, Value>) {
    let Some(documentation) = object
        .get("processDocumentation")
        .and_then(Value::as_object)
    else {
        return;
    };
    for (source, target) in [
        ("validUntil", "dataSetValidUntil"),
        ("timeDescription", "timeDescription"),
        ("geographyDescription", "locationDescription"),
        ("technologyDescription", "technologyDescription"),
        ("samplingDescription", "samplingProcedure"),
        ("dataCollectionDescription", "dataCollectionPeriod"),
        (
            "dataSelectionDescription",
            "dataSelectionAndCombinationPrinciples",
        ),
        (
            "dataTreatmentDescription",
            "dataTreatmentAndExtrapolationsPrinciples",
        ),
        (
            "completenessDescription",
            "dataCutOffAndCompletenessPrinciples",
        ),
        ("uncertaintyAdjustments", "uncertaintyAdjustments"),
        ("useAdvice", "useAdviceForDataSet"),
        ("intendedApplication", "intendedApplications"),
        ("projectDescription", "project"),
        ("modelingConstantsDescription", "modellingConstants"),
        (
            "inventoryMethodDescription",
            "deviationsFromLCIMethodPrinciple",
        ),
        ("restrictionsDescription", "accessRestrictions"),
        ("creationDate", "creationDate"),
        ("sources", "sourceRefs"),
        ("reviews", "sourceReviews"),
    ] {
        if let Some(value) = documentation.get(source).cloned() {
            raw.insert(target.to_owned(), value);
        }
    }
    let year_source = documentation
        .get("validFrom")
        .or_else(|| documentation.get("creationDate"));
    if let Some(year) = year_source.and_then(year) {
        raw.insert("referenceYear".to_owned(), json!(year));
    }
}

fn apply_process_location(object: &Map<String, Value>, raw: &mut Map<String, Value>) {
    let Some(location) = object.get("location").and_then(Value::as_object) else {
        return;
    };
    if let Some(code) = text(location.get("code")) {
        raw.insert("location".to_owned(), Value::String(code.to_owned()));
    }
    if let Some(id) = text(location.get("@id")) {
        raw.insert("locationRefId".to_owned(), Value::String(id.to_owned()));
    }
}

fn exchange(object: &Map<String, Value>, index: usize) -> Option<Value> {
    let flow = object.get("flow")?.as_object()?;
    let flow_id = text(flow.get("@id"))?;
    let flow_name = text(flow.get("name")).unwrap_or("Flow");
    let amount = object
        .get("amount")
        .map_or_else(|| "0".to_owned(), number_text);
    let mut exchange = Map::from_iter([
        (
            "internalId".to_owned(),
            object
                .get("internalId")
                .cloned()
                .unwrap_or_else(|| json!(index.saturating_add(1))),
        ),
        ("flowRefId".to_owned(), Value::String(flow_id.to_owned())),
        ("flowName".to_owned(), Value::String(flow_name.to_owned())),
        ("flow".to_owned(), Value::Object(flow.clone())),
        (
            "isInput".to_owned(),
            Value::Bool(
                object
                    .get("isInput")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
            ),
        ),
        ("amount".to_owned(), Value::String(amount)),
    ]);
    if let Some(reference) = object.get("isQuantitativeReference").cloned() {
        exchange.insert("isQuantitativeReference".to_owned(), reference);
    }
    for field in [
        "amountFormula",
        "isAvoidedProduct",
        "dqEntry",
        "description",
    ] {
        if let Some(value) = object.get(field).cloned() {
            let target = if field == "description" {
                "generalComment"
            } else {
                field
            };
            exchange.insert(target.to_owned(), value);
        }
    }
    if let Some(unit) = object.get("unit").and_then(Value::as_object) {
        copy_ref(unit, &mut exchange, "unitId", "unitName");
    } else if let Some(unit) = text(flow.get("refUnit")) {
        exchange.insert("unitName".to_owned(), Value::String(unit.to_owned()));
    }
    if let Some(property) = object.get("flowProperty").and_then(Value::as_object) {
        copy_ref(
            property,
            &mut exchange,
            "flowPropertyRefId",
            "flowPropertyName",
        );
    }
    if let Some(provider) = object.get("defaultProvider").and_then(Value::as_object)
        && let Some(provider_id) = text(provider.get("@id"))
    {
        exchange.insert(
            "providerRefId".to_owned(),
            Value::String(provider_id.to_owned()),
        );
    }
    if let Some(location) = object.get("location").and_then(Value::as_object) {
        if let Some(code) = text(location.get("code")) {
            exchange.insert("location".to_owned(), Value::String(code.to_owned()));
        }
        exchange.insert("sourceLocation".to_owned(), Value::Object(location.clone()));
    }
    add_uncertainty(&mut exchange, object.get("uncertainty"));
    exchange.insert(
        "sourceTrace".to_owned(),
        json!({"format": "openlca-jsonld", "exchange": object}),
    );
    Some(Value::Object(exchange))
}

fn add_uncertainty(exchange: &mut Map<String, Value>, value: Option<&Value>) {
    let Some(uncertainty) = value.and_then(Value::as_object) else {
        return;
    };
    if let Some(kind) = uncertainty
        .get("distributionType")
        .and_then(Value::as_str)
        .and_then(uncertainty_type)
    {
        exchange.insert(
            "uncertaintyDistributionType".to_owned(),
            Value::String(kind.to_owned()),
        );
    }
    for (source, target) in [("minimum", "minimumAmount"), ("maximum", "maximumAmount")] {
        if let Some(value) = uncertainty.get(source) {
            exchange.insert(target.to_owned(), Value::String(number_text(value)));
        }
    }
    if let Some(dispersion) = uncertainty_dispersion(uncertainty) {
        exchange.insert(
            "relativeStandardDeviation95In".to_owned(),
            Value::String(dispersion),
        );
    }
}

fn uncertainty_type(value: &str) -> Option<&'static str> {
    match value {
        "LOG_NORMAL_DISTRIBUTION" => Some("log-normal"),
        "NORMAL_DISTRIBUTION" => Some("normal"),
        "TRIANGLE_DISTRIBUTION" | "TRIANGULAR_DISTRIBUTION" => Some("triangular"),
        "UNIFORM_DISTRIBUTION" => Some("uniform"),
        _ => None,
    }
}

fn uncertainty_dispersion(uncertainty: &Map<String, Value>) -> Option<String> {
    let distribution = text(uncertainty.get("distributionType"))?;
    let value = match distribution {
        "LOG_NORMAL_DISTRIBUTION" => {
            let value = decimal(uncertainty.get("geomSd"))?;
            &value * &value
        }
        "NORMAL_DISTRIBUTION" => decimal(uncertainty.get("sd"))? * BigDecimal::from(2),
        _ => return None,
    };
    let rounded = value.with_scale_round(3, RoundingMode::HalfEven);
    (BigDecimal::from(0)..=BigDecimal::from(100))
        .contains(&rounded)
        .then(|| rounded.to_string())
}

fn apply_allocations(process: &Map<String, Value>, exchanges: &mut [Value]) {
    let Some(factors) = process.get("allocationFactors").and_then(Value::as_array) else {
        return;
    };
    let mut exchange_indexes = BTreeMap::new();
    let mut output_flow_indexes = BTreeMap::new();
    for (index, exchange) in exchanges.iter().filter_map(Value::as_object).enumerate() {
        let emitted_id = index.saturating_add(1).to_string();
        if let Some(internal_id) = exchange.get("internalId") {
            exchange_indexes.insert(value_key(internal_id), index);
        }
        if !exchange
            .get("isInput")
            .and_then(Value::as_bool)
            .unwrap_or(false)
            && let Some(flow_id) = text(exchange.get("flowRefId"))
        {
            output_flow_indexes
                .entry(flow_id.to_owned())
                .or_insert(emitted_id);
        }
    }
    let mut allocations: BTreeMap<usize, Vec<Value>> = BTreeMap::new();
    for factor in factors.iter().filter_map(Value::as_object) {
        let Some(exchange_id) = factor
            .get("exchange")
            .and_then(Value::as_object)
            .and_then(|exchange| exchange.get("internalId"))
            .map(value_key)
        else {
            continue;
        };
        let Some(target_index) = exchange_indexes.get(&exchange_id).copied() else {
            continue;
        };
        let Some(product_id) = factor
            .get("product")
            .and_then(Value::as_object)
            .and_then(|product| text(product.get("@id")))
        else {
            continue;
        };
        let Some(coproduct) = output_flow_indexes.get(product_id) else {
            continue;
        };
        let Some(fraction) = allocation_percentage(factor.get("value")) else {
            continue;
        };
        if fraction == "0" {
            continue;
        }
        allocations.entry(target_index).or_default().push(json!({
            "internalReferenceToCoProduct": coproduct,
            "allocatedFraction": fraction,
        }));
    }
    for (index, entries) in allocations {
        if let Some(exchange) = exchanges.get_mut(index).and_then(Value::as_object_mut) {
            exchange.insert("allocations".to_owned(), Value::Array(entries));
        }
    }
}

fn allocation_percentage(value: Option<&Value>) -> Option<String> {
    let percentage = decimal(value)? * BigDecimal::from(100);
    Some(
        percentage
            .with_scale_round(3, RoundingMode::HalfEven)
            .normalized()
            .to_string(),
    )
}

fn decimal(value: Option<&Value>) -> Option<BigDecimal> {
    match value? {
        Value::String(value) => BigDecimal::from_str(value.trim()).ok(),
        Value::Number(value) => BigDecimal::from_str(&value.to_string()).ok(),
        _ => None,
    }
}

fn value_key(value: &Value) -> String {
    match value {
        Value::String(value) => value.clone(),
        other => other.to_string(),
    }
}

fn year(value: &Value) -> Option<u64> {
    let text = match value {
        Value::String(value) => value.as_str(),
        Value::Number(value) => return value.as_u64(),
        _ => return None,
    };
    text.split(|character: char| !character.is_ascii_digit())
        .find(|token| token.len() == 4)
        .and_then(|token| token.parse().ok())
}

fn copy_fields(object: &Map<String, Value>, fields: &[&str]) -> Map<String, Value> {
    fields
        .iter()
        .filter_map(|field| {
            object
                .get(*field)
                .cloned()
                .map(|value| ((*field).to_owned(), value))
        })
        .collect()
}

fn copy_ref(
    source: &Map<String, Value>,
    target: &mut Map<String, Value>,
    id_field: &str,
    name_field: &str,
) {
    if let Some(id) = source.get("@id").cloned() {
        target.insert(id_field.to_owned(), id);
    }
    if let Some(name) = source.get("name").cloned() {
        target.insert(name_field.to_owned(), name);
    }
}

fn name(object: &Map<String, Value>) -> String {
    text(object.get("name"))
        .or_else(|| text(object.get("@id")))
        .unwrap_or("Unnamed openLCA entity")
        .to_owned()
}

fn text(value: Option<&Value>) -> Option<&str> {
    value.and_then(Value::as_str)
}

fn number_text(value: &Value) -> String {
    match value {
        Value::String(value) => value.clone(),
        Value::Number(value) => value.to_string(),
        _ => "0".to_owned(),
    }
}

fn split_path(value: &str) -> Vec<String> {
    value
        .split('/')
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

fn stable_id(seed: &str) -> String {
    Uuid::new_v5(&Uuid::NAMESPACE_URL, seed.as_bytes()).to_string()
}

fn resolve_process_locations(store: &mut CanonicalStore) -> Result<(), AdapterError> {
    let processes = store
        .iter_type("processes")?
        .collect::<Result<Vec<_>, _>>()?;
    for mut process in processes {
        if process.raw.contains_key("location") {
            continue;
        }
        let Some(reference) = process.raw.get("locationRefId").and_then(Value::as_str) else {
            continue;
        };
        let Some(location) = store.get_by_external_id("locations", reference)? else {
            continue;
        };
        if let Some(code) = location.raw.get("code").and_then(Value::as_str) {
            process
                .raw
                .insert("location".to_owned(), Value::String(code.to_owned()));
            store.add(&process)?;
        }
    }
    Ok(())
}

fn resolve_process_data_quality(store: &mut CanonicalStore) -> Result<(), AdapterError> {
    let processes = store
        .iter_type("processes")?
        .collect::<Result<Vec<_>, _>>()?;
    for mut process in processes {
        let Some(entry) = process.raw.get("dqEntry").and_then(Value::as_str) else {
            continue;
        };
        let Some(system_id) = process
            .raw
            .get("dqSystem")
            .and_then(Value::as_object)
            .and_then(|system| text(system.get("@id")))
        else {
            continue;
        };
        let Some(system) = store.get_by_external_id("dqsystems", system_id)? else {
            continue;
        };
        let indicators = data_quality_indicators(entry, &system.raw);
        if !indicators.is_empty() {
            process
                .raw
                .insert("dataQualityIndicators".to_owned(), Value::Array(indicators));
            store.add(&process)?;
        }
    }
    store.remove_type("dqsystems")?;
    Ok(())
}

fn data_quality_indicators(entry: &str, system: &Map<String, Value>) -> Vec<Value> {
    let scores = entry
        .trim()
        .trim_matches(|character| matches!(character, '(' | ')'))
        .split(';')
        .map(str::trim)
        .filter_map(|score| score.parse::<u8>().ok())
        .collect::<Vec<_>>();
    let mut names = BTreeMap::new();
    if let Some(indicators) = system.get("indicators").and_then(Value::as_array) {
        for indicator in indicators.iter().filter_map(Value::as_object) {
            if let (Some(position), Some(name)) = (
                indicator.get("position").and_then(Value::as_u64),
                text(indicator.get("name")),
            ) {
                names.insert(position, name);
            }
        }
    }
    let mut used = std::collections::BTreeSet::new();
    scores
        .into_iter()
        .enumerate()
        .filter_map(|(index, score)| {
            let name = names.get(&u64::try_from(index).ok()?.saturating_add(1))?;
            let mapped_name = quality_indicator_name(name)?;
            let level = quality_level(score)?;
            used.insert(mapped_name).then(|| {
                json!({
                    "@name": mapped_name,
                    "@value": level,
                })
            })
        })
        .collect()
}

fn quality_indicator_name(name: &str) -> Option<&'static str> {
    let name = name.to_ascii_lowercase();
    [
        ("complete", "Completeness"),
        ("temporal", "Time representativeness"),
        ("time", "Time representativeness"),
        ("geograph", "Geographical representativeness"),
        ("technolog", "Technological representativeness"),
        ("reliab", "Precision"),
        ("review", "Methodological appropriateness and consistency"),
        (
            "data collection",
            "Methodological appropriateness and consistency",
        ),
        ("method", "Methodological appropriateness and consistency"),
    ]
    .into_iter()
    .find_map(|(keyword, mapped)| name.contains(keyword).then_some(mapped))
}

fn quality_level(score: u8) -> Option<&'static str> {
    match score {
        1 => Some("Very good"),
        2 => Some("Good"),
        3 => Some("Fair"),
        4 => Some("Poor"),
        5 => Some("Very poor"),
        _ => None,
    }
}

fn provider_graph_lifecycle_model(
    store: &CanonicalStore,
) -> Result<Option<CanonicalEntity>, AdapterError> {
    let processes = store
        .iter_type("processes")?
        .collect::<Result<Vec<_>, _>>()?;
    if processes.len() < 2 {
        return Ok(None);
    }
    let process_ids = processes
        .iter()
        .map(|process| process.internal_id.as_str())
        .collect::<std::collections::BTreeSet<_>>();
    let mut connections = Vec::new();
    for consumer in &processes {
        for exchange in store.iter_process_exchanges(&consumer.internal_id)? {
            let exchange = exchange?;
            let Some(provider_id) = exchange.get("providerRefId").and_then(Value::as_str) else {
                continue;
            };
            if !exchange
                .get("isInput")
                .and_then(Value::as_bool)
                .unwrap_or(false)
                || !process_ids.contains(provider_id)
            {
                continue;
            }
            let Some(flow_id) = exchange.get("flowRefId").and_then(Value::as_str) else {
                continue;
            };
            if Uuid::parse_str(flow_id).is_err() {
                continue;
            }
            connections.push(json!({
                "providerProcessId": provider_id,
                "consumerProcessId": consumer.internal_id,
                "flowRefId": flow_id,
                "flowName": exchange.get("flowName").cloned().unwrap_or(Value::Null),
                "consumerExchangeInternalId": exchange.get("internalId").cloned().unwrap_or(Value::Null),
                "amount": exchange.get("amount").cloned().unwrap_or(Value::String("0".to_owned())),
                "location": exchange.get("location").cloned().unwrap_or(Value::Null),
            }));
        }
    }
    if connections.is_empty() {
        return Ok(None);
    }
    let provider_connection_count = connections.len();
    let reference_process = processes
        .iter()
        .find(|process| {
            process.raw.get("sourceProcessType").and_then(Value::as_str) == Some("LCI_RESULT")
        })
        .unwrap_or(&processes[0]);
    let process_refs = processes
        .iter()
        .map(|process| {
            json!({
                "id": process.internal_id,
                "name": process.name.as_deref().unwrap_or("Process"),
                "processType": process.raw.get("sourceProcessType").cloned().unwrap_or(Value::Null),
            })
        })
        .collect::<Vec<_>>();
    let model_seed = processes
        .iter()
        .map(|process| process.internal_id.as_str())
        .collect::<Vec<_>>()
        .join("/");
    Ok(Some(CanonicalEntity {
        entity_type: "lifecyclemodels".to_owned(),
        internal_id: stable_id(&format!("openlca-jsonld/lifecyclemodel/{model_seed}")),
        external_id: None,
        name: Some("openLCA JSON-LD provider graph".to_owned()),
        category_path: Vec::new(),
        raw: Map::from_iter([
            (
                "description".to_owned(),
                Value::String(
                    "Candidate lifecycle model derived from openLCA JSON-LD exchange defaultProvider hints."
                        .to_owned(),
                ),
            ),
            (
                "referenceProcessId".to_owned(),
                Value::String(reference_process.internal_id.clone()),
            ),
            ("processRefs".to_owned(), Value::Array(process_refs)),
            ("connections".to_owned(), Value::Array(connections)),
            (
                "sourceTrace".to_owned(),
                json!({
                    "format": "openlca-jsonld",
                    "sourceObject": "provider-graph",
                    "derivedEntity": "default-provider-candidate-lifecyclemodel",
                    "providerConnectionCount": provider_connection_count,
                }),
            ),
        ]),
    }))
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;
    use tidas_runtime::{CancellationToken, MemoryBudget};
    use tidas_validation::{ValidationRequest, validate_ilcd_package, validate_tidas_package};

    use super::*;
    use crate::normalization::normalize_flow;
    use crate::report::IssueSpool;
    use crate::writers::{
        IlcdWriteRequest, TidasWriteRequest, write_ilcd_package, write_tidas_package,
    };

    #[test]
    fn openlca_entities_and_decimal_lexemes_are_preserved() {
        let directory = tempdir().unwrap();
        std::fs::write(
            directory.path().join("flow.json"),
            br#"{"@type":"Flow","@id":"11111111-1111-4111-8111-111111111111","name":"Steel","flowType":"PRODUCT_FLOW","refUnit":"kg","treatmentStandardsRoutes":"production route","mixAndLocationTypes":"GLO"}"#,
        )
        .unwrap();
        std::fs::write(
            directory.path().join("process.json"),
            br#"{"@type":"Process","@id":"22222222-2222-4222-8222-222222222222","name":"Steel process","exchanges":[{"internalId":1,"flow":{"@id":"11111111-1111-4111-8111-111111111111","name":"Steel"},"amount":0.123456789012345678,"isInput":false}]}"#,
        )
        .unwrap();
        let cancellation = CancellationToken::default();
        let memory_budget = MemoryBudget::new(4 * 1024 * 1024);
        let mut store = CanonicalStore::create(Some(directory.path())).unwrap();
        let mut issues = IssueSpool::new(Vec::new(), 64 * 1024);
        OpenLcaJsonLdAdapter
            .read(
                &AdapterContext {
                    source: directory.path(),
                    cancellation: &cancellation,
                    memory_budget: &memory_budget,
                    max_entry_bytes: 1024 * 1024,
                },
                &mut store,
                &mut issues,
            )
            .unwrap();
        issues.finish().unwrap();
        let process = store
            .get("processes", "22222222-2222-4222-8222-222222222222")
            .unwrap()
            .unwrap();
        let exchange = store
            .iter_process_exchanges(&process.internal_id)
            .unwrap()
            .next()
            .unwrap()
            .unwrap();
        assert_eq!(exchange["amount"], "0.123456789012345678");
        assert_eq!(store.counts()["flows"], 1);
        assert_eq!(store.counts()["unitgroups"], 1);
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

    #[test]
    fn openlca_all_flow_properties_reach_typed_normalization() {
        let object = json!({
            "@type": "Flow",
            "@id": "11111111-1111-4111-8111-111111111111",
            "name": "Fuel",
            "flowType": "PRODUCT_FLOW",
            "treatmentStandardsRoutes": "production route",
            "mixAndLocationTypes": "GLO",
            "flowProperties": [
                {
                    "flowProperty": {
                        "@id": "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb",
                        "name": "Mass"
                    },
                    "conversionFactor": "1",
                    "isRefFlowProperty": true
                },
                {
                    "flowProperty": {
                        "@id": "cccccccc-cccc-4ccc-8ccc-cccccccccccc",
                        "name": "Net calorific value"
                    },
                    "conversionFactor": "42.500",
                    "isRefFlowProperty": false
                }
            ]
        });
        let entity = to_entity(object.as_object().unwrap(), "fixture.json").unwrap();
        let normalized = normalize_flow(&entity).unwrap();
        assert_eq!(normalized.flow_properties.len(), 2);
        assert_eq!(
            normalized.flow_properties[0].flow_property_uuid,
            "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb"
        );
        assert!(normalized.flow_properties[0].is_reference);
        assert_eq!(
            normalized.flow_properties[1].flow_property_uuid,
            "cccccccc-cccc-4ccc-8ccc-cccccccccccc"
        );
        assert_eq!(normalized.flow_properties[1].conversion_factor, "42.500");
    }

    #[test]
    fn uncertainty_and_allocation_match_the_frozen_python_rules() {
        assert_eq!(
            uncertainty_dispersion(
                json!({
                    "distributionType": "LOG_NORMAL_DISTRIBUTION",
                    "geomSd": "1.05"
                })
                .as_object()
                .unwrap()
            ),
            Some("1.102".to_owned())
        );
        assert_eq!(
            uncertainty_dispersion(
                json!({
                    "distributionType": "NORMAL_DISTRIBUTION",
                    "sd": 3
                })
                .as_object()
                .unwrap()
            ),
            Some("6.000".to_owned())
        );
        assert_eq!(
            uncertainty_dispersion(
                json!({
                    "distributionType": "LOG_NORMAL_DISTRIBUTION",
                    "geomSd": 11
                })
                .as_object()
                .unwrap()
            ),
            None
        );

        let raw = process_raw(
            json!({
                "@type": "Process",
                "@id": "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb",
                "name": "allocation fixture",
                "exchanges": [
                    {"internalId": 7, "isInput": true, "flow": {"@id": "in-x", "name": "input"}, "amount": 5},
                    {"internalId": 1, "isInput": false, "flow": {"@id": "prod-a", "name": "A"}, "amount": 1},
                    {"internalId": 2, "isInput": false, "flow": {"@id": "prod-b", "name": "B"}, "amount": 1}
                ],
                "allocationFactors": [
                    {"exchange": {"internalId": 7}, "product": {"@id": "prod-a"}, "value": 0.6},
                    {"exchange": {"internalId": 7}, "product": {"@id": "prod-b"}, "value": 0.4},
                    {"exchange": {"internalId": 7}, "product": {"@id": "prod-a"}, "value": 0}
                ]
            })
            .as_object()
            .unwrap(),
        );
        let allocations = &raw["exchanges"][0]["allocations"];
        assert_eq!(
            allocations,
            &json!([
                {"internalReferenceToCoProduct": "2", "allocatedFraction": "60"},
                {"internalReferenceToCoProduct": "3", "allocatedFraction": "40"}
            ])
        );
        assert!(raw["exchanges"][1].get("allocations").is_none());

        let indicators = data_quality_indicators(
            "(2;1)",
            json!({
                "indicators": [
                    {"position": 1, "name": "Process Review"},
                    {"position": 2, "name": "Process Completeness"}
                ]
            })
            .as_object()
            .unwrap(),
        );
        assert_eq!(
            indicators,
            vec![
                json!({"@name": "Methodological appropriateness and consistency", "@value": "Good"}),
                json!({"@name": "Completeness", "@value": "Very good"})
            ]
        );
    }

    #[test]
    fn provider_links_create_a_schema_valid_lifecycle_model() {
        let directory = tempdir().unwrap();
        write_provider_fixture(directory.path());
        let cancellation = CancellationToken::default();
        let memory_budget = MemoryBudget::new(16 * 1024 * 1024);
        let mut store = CanonicalStore::create(Some(directory.path())).unwrap();
        let mut issues = IssueSpool::new(Vec::new(), 64 * 1024);
        OpenLcaJsonLdAdapter
            .read(
                &AdapterContext {
                    source: directory.path(),
                    cancellation: &cancellation,
                    memory_budget: &memory_budget,
                    max_entry_bytes: 1024 * 1024,
                },
                &mut store,
                &mut issues,
            )
            .unwrap();
        issues.finish().unwrap();
        assert_eq!(store.counts()["lifecyclemodels"], 1);
        let output = directory.path().join("tidas");
        write_tidas_package(&TidasWriteRequest {
            store: &store,
            output_dir: &output,
            cancellation: &cancellation,
            memory_budget: &memory_budget,
        })
        .unwrap();
        let issue_path = directory.path().join("validation.jsonl");
        let validation = validate_tidas_package(&ValidationRequest {
            input_dir: output.clone(),
            issue_spool: Some(issue_path.clone()),
            cancellation,
            memory_budget,
            queue_capacity: 2,
            progress: None,
        })
        .unwrap();
        assert!(
            validation.summary.ok,
            "lifecycle model validation failed: {}",
            std::fs::read_to_string(issue_path).unwrap()
        );
        let model_path = std::fs::read_dir(output.join("lifecyclemodels"))
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        let model: Value = serde_json::from_slice(&std::fs::read(model_path).unwrap()).unwrap();
        let instances =
            model["lifeCycleModelDataSet"]["lifeCycleModelInformation"]["technology"]["processes"]
                ["processInstance"]
                .as_array()
                .unwrap();
        let provider = instances
            .iter()
            .find(|instance| instance["referenceToProcess"]["@refObjectId"] == PROVIDER_PROCESS_ID)
            .unwrap();
        let downstream = &provider["connections"]["outputExchange"]["downstreamProcess"];
        assert_eq!(downstream["@flowUUID"], PROVIDER_FLOW_ID);
        assert_eq!(
            downstream["@id"],
            instances
                .iter()
                .find(|instance| {
                    instance["referenceToProcess"]["@refObjectId"] == CONSUMER_PROCESS_ID
                })
                .unwrap()["@dataSetInternalID"]
        );
        let ilcd = directory.path().join("ilcd");
        write_ilcd_package(&IlcdWriteRequest {
            store: &store,
            output_dir: &ilcd,
            cancellation: &CancellationToken::default(),
            memory_budget: &MemoryBudget::new(32 * 1024 * 1024),
            queue_capacity: 2,
        })
        .unwrap();
        let ilcd_issues = directory.path().join("ilcd-validation.jsonl");
        let ilcd_validation = validate_ilcd_package(&ValidationRequest {
            input_dir: ilcd,
            issue_spool: Some(ilcd_issues.clone()),
            cancellation: CancellationToken::default(),
            memory_budget: MemoryBudget::new(32 * 1024 * 1024),
            queue_capacity: 2,
            progress: None,
        })
        .unwrap();
        assert!(
            ilcd_validation.summary.ok,
            "lifecycle model ILCD validation failed: {}",
            std::fs::read_to_string(ilcd_issues).unwrap()
        );
    }

    const PROVIDER_FLOW_ID: &str = "11111111-1111-4111-8111-111111111111";
    const PROVIDER_PROCESS_ID: &str = "22222222-2222-4222-8222-222222222222";
    const CONSUMER_PROCESS_ID: &str = "33333333-3333-4333-8333-333333333333";

    fn write_provider_fixture(directory: &std::path::Path) {
        std::fs::write(
            directory.join("flow.json"),
            format!(
                r#"{{"@type":"Flow","@id":"{PROVIDER_FLOW_ID}","name":"Steel","flowType":"PRODUCT_FLOW","refUnit":"kg","treatmentStandardsRoutes":"production route","mixAndLocationTypes":"GLO"}}"#
            ),
        )
        .unwrap();
        std::fs::write(
            directory.join("provider.json"),
            format!(
                r#"{{"@type":"Process","@id":"{PROVIDER_PROCESS_ID}","name":"Steel provider","exchanges":[{{"internalId":1,"flow":{{"@id":"{PROVIDER_FLOW_ID}","name":"Steel"}},"amount":1,"isInput":false,"isQuantitativeReference":true}}]}}"#
            ),
        )
        .unwrap();
        std::fs::write(
            directory.join("consumer.json"),
            format!(
                r#"{{"@type":"Process","@id":"{CONSUMER_PROCESS_ID}","name":"Steel consumer","exchanges":[{{"internalId":1,"flow":{{"@id":"{PROVIDER_FLOW_ID}","name":"Steel"}},"amount":2.5,"isInput":true,"defaultProvider":{{"@type":"Process","@id":"{PROVIDER_PROCESS_ID}","name":"Steel provider"}}}},{{"internalId":2,"flow":{{"@id":"{PROVIDER_FLOW_ID}","name":"Steel"}},"amount":1,"isInput":false,"isQuantitativeReference":true}}]}}"#
            ),
        )
        .unwrap();
    }
}
