use std::collections::BTreeMap;

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
                store.add(&entity)?;
                supported = supported.saturating_add(1);
            }
            Ok::<(), AdapterError>(())
        })?;
        resolve_process_locations(store)?;
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
    raw
}

fn process_raw(object: &Map<String, Value>) -> Map<String, Value> {
    let exchanges = object
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
    ]);
    if let Some(process_type) = text(object.get("processType")) {
        raw.insert(
            "sourceProcessType".to_owned(),
            Value::String(process_type.to_owned()),
        );
    }
    if let Some(location) = object.get("location").and_then(Value::as_object) {
        if let Some(code) = text(location.get("code")) {
            raw.insert("location".to_owned(), Value::String(code.to_owned()));
        }
        if let Some(id) = text(location.get("@id")) {
            raw.insert("locationRefId".to_owned(), Value::String(id.to_owned()));
        }
    }
    raw
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
        (
            "flow".to_owned(),
            json!({"@id": flow_id, "name": flow_name}),
        ),
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
    if let Some(unit) = object
        .get("unit")
        .and_then(Value::as_object)
        .and_then(|unit| text(unit.get("name")))
        .or_else(|| text(flow.get("refUnit")))
    {
        exchange.insert("unitName".to_owned(), Value::String(unit.to_owned()));
    }
    if let Some(provider) = object.get("defaultProvider").and_then(Value::as_object)
        && let Some(provider_id) = text(provider.get("@id"))
    {
        exchange.insert(
            "providerRefId".to_owned(),
            Value::String(provider_id.to_owned()),
        );
    }
    Some(Value::Object(exchange))
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
    use crate::report::IssueSpool;
    use crate::writers::{
        IlcdWriteRequest, TidasWriteRequest, write_ilcd_package, write_tidas_package,
    };

    #[test]
    fn openlca_entities_and_decimal_lexemes_are_preserved() {
        let directory = tempdir().unwrap();
        std::fs::write(
            directory.path().join("flow.json"),
            br#"{"@type":"Flow","@id":"11111111-1111-4111-8111-111111111111","name":"Steel","flowType":"PRODUCT_FLOW","refUnit":"kg"}"#,
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
                r#"{{"@type":"Flow","@id":"{PROVIDER_FLOW_ID}","name":"Steel","flowType":"PRODUCT_FLOW","refUnit":"kg"}}"#
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
