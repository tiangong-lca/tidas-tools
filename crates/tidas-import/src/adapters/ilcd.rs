use std::collections::BTreeMap;

use serde_json::{Map, Value, json};
use uuid::Uuid;

use super::{AdapterContext, AdapterError, SourceAdapter};
use crate::detect::SourceFormat;
use crate::model::CanonicalEntity;
use crate::report::{ImportIssue, IssueSeverity, IssueSink};
use crate::source::{SourceReadRequest, visit_source_entries};
use crate::store::CanonicalStore;

pub struct IlcdAdapter;

impl SourceAdapter for IlcdAdapter {
    fn format(&self) -> SourceFormat {
        SourceFormat::Ilcd
    }

    fn read(
        &self,
        context: &AdapterContext<'_>,
        store: &mut CanonicalStore,
        issues: &mut dyn IssueSink,
    ) -> Result<(), AdapterError> {
        let request = SourceReadRequest {
            source: context.source,
            allowed_extensions: &["xml"],
            max_entry_bytes: context.max_entry_bytes,
            cancellation: context.cancellation,
            memory_budget: context.memory_budget,
        };
        let mut count = 0_u64;
        visit_source_entries(&request, |entry| {
            let _structured_reservation =
                context.reserve_structured_expansion(entry.bytes.len(), 6)?;
            let json_bytes =
                match tidas_conversion::convert_xml_to_json(entry.bytes, context.cancellation) {
                    Ok(bytes) => bytes,
                    Err(error) => {
                        issues.push(&ImportIssue {
                            severity: IssueSeverity::Error,
                            code: "invalid_ilcd_xml".to_owned(),
                            message: format!("Skipped invalid ILCD XML: {error}"),
                            source_object: Some(entry.label.clone()),
                            context: BTreeMap::new(),
                        })?;
                        return Ok::<(), AdapterError>(());
                    }
                };
            let document: Value = serde_json::from_slice(&json_bytes)?;
            let Some(mut entity) = entity_from_document(&document, &entry.stable_key) else {
                return Ok::<(), AdapterError>(());
            };
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
            count = count.saturating_add(1);
            Ok::<(), AdapterError>(())
        })?;
        if count == 0 {
            issues.push(&ImportIssue {
                severity: IssueSeverity::Error,
                code: "no_ilcd_datasets".to_owned(),
                message: "No supported ILCD datasets were found.".to_owned(),
                source_object: None,
                context: BTreeMap::new(),
            })?;
        } else {
            issues.push(&ImportIssue {
                severity: IssueSeverity::Warning,
                code: "ilcd_native_mapping".to_owned(),
                message: "Mapped ILCD datasets with native Rust rules.".to_owned(),
                source_object: None,
                context: BTreeMap::from([("entity_count".to_owned(), json!(count))]),
            })?;
        }
        Ok(())
    }
}

fn entity_from_document(document: &Value, label: &str) -> Option<CanonicalEntity> {
    let (root_name, dataset) = document.as_object()?.iter().next()?;
    let dataset = dataset.as_object()?;
    let (entity_type, information_path): (&str, &[&str]) = match root_name.as_str() {
        "contactDataSet" => ("contacts", &["contactInformation", "dataSetInformation"]),
        "sourceDataSet" => ("sources", &["sourceInformation", "dataSetInformation"]),
        "unitGroupDataSet" => (
            "unitgroups",
            &["unitGroupInformation", "dataSetInformation"],
        ),
        "flowPropertyDataSet" => (
            "flowproperties",
            &["flowPropertiesInformation", "dataSetInformation"],
        ),
        "flowDataSet" => ("flows", &["flowInformation", "dataSetInformation"]),
        "processDataSet" => ("processes", &["processInformation", "dataSetInformation"]),
        _ => return None,
    };
    let information = value_at(dataset, information_path)?;
    let id = field_text_value(information, "common:UUID")
        .filter(|value| Uuid::parse_str(value).is_ok())
        .map_or_else(
            || stable_id(&format!("ilcd/{entity_type}/{label}")),
            ToOwned::to_owned,
        );
    let raw = match entity_type {
        "unitgroups" => unit_group_raw(dataset),
        "flowproperties" => flow_property_raw(dataset),
        "flows" => flow_raw(dataset),
        "processes" => process_raw(dataset),
        "sources" => source_raw(information),
        _ => Map::new(),
    };
    Some(CanonicalEntity {
        entity_type: entity_type.to_owned(),
        internal_id: id.clone(),
        external_id: Some(id),
        name: dataset_name(information, entity_type),
        category_path: classification_path(information),
        raw,
    })
}

fn unit_group_raw(dataset: &Map<String, Value>) -> Map<String, Value> {
    let reference = value_at(
        dataset,
        &[
            "unitGroupInformation",
            "quantitativeReference",
            "referenceToReferenceUnit",
        ],
    )
    .and_then(scalar_text);
    let units = value_at(dataset, &["units", "unit"])
        .map(items)
        .unwrap_or_default()
        .into_iter()
        .filter_map(Value::as_object)
        .map(|unit| {
            let internal_id = field_text(unit, "@dataSetInternalID").unwrap_or("1");
            json!({
                "name": field_text(unit, "name").unwrap_or("unit"),
                "conversionFactor": field_text(unit, "meanValue").unwrap_or("1"),
                "referenceUnit": reference == Some(internal_id),
            })
        })
        .collect::<Vec<_>>();
    Map::from_iter([("units".to_owned(), Value::Array(units))])
}

fn flow_property_raw(dataset: &Map<String, Value>) -> Map<String, Value> {
    let mut raw = Map::new();
    let reference = value_at(
        dataset,
        &[
            "flowPropertiesInformation",
            "quantitativeReference",
            "referenceToReferenceUnitGroup",
        ],
    )
    .and_then(Value::as_object);
    if let Some(reference) = reference {
        if let Some(id) = field_text(reference, "@refObjectId") {
            raw.insert("unitGroupRefId".to_owned(), Value::String(id.to_owned()));
        }
        if let Some(name) = reference
            .get("common:shortDescription")
            .and_then(localized_text)
        {
            raw.insert("unitGroupName".to_owned(), Value::String(name.to_owned()));
        }
    }
    raw
}

fn flow_raw(dataset: &Map<String, Value>) -> Map<String, Value> {
    let mut raw = Map::new();
    let flow_type = value_at(
        dataset,
        &["modellingAndValidation", "LCIMethod", "typeOfDataSet"],
    )
    .and_then(scalar_text)
    .unwrap_or("Product flow");
    raw.insert(
        "flowType".to_owned(),
        Value::String(flow_type.to_ascii_uppercase().replace(' ', "_")),
    );
    let property = value_at(dataset, &["flowProperties", "flowProperty"])
        .map(items)
        .unwrap_or_default()
        .into_iter()
        .find_map(Value::as_object)
        .and_then(|item| item.get("referenceToFlowPropertyDataSet"))
        .and_then(Value::as_object);
    if let Some(property) = property {
        if let Some(id) = field_text(property, "@refObjectId") {
            raw.insert("flowPropertyRefId".to_owned(), Value::String(id.to_owned()));
        }
        if let Some(name) = property
            .get("common:shortDescription")
            .and_then(localized_text)
        {
            raw.insert(
                "flowPropertyName".to_owned(),
                Value::String(name.to_owned()),
            );
        }
    }
    raw
}

fn process_raw(dataset: &Map<String, Value>) -> Map<String, Value> {
    let description = value_at(
        dataset,
        &[
            "processInformation",
            "dataSetInformation",
            "common:generalComment",
        ],
    )
    .and_then(localized_text)
    .unwrap_or("Imported from ILCD.");
    let exchanges = value_at(dataset, &["exchanges", "exchange"])
        .map(items)
        .unwrap_or_default()
        .into_iter()
        .filter_map(Value::as_object)
        .enumerate()
        .filter_map(|(index, exchange)| process_exchange(exchange, index))
        .collect();
    let mut raw = Map::from_iter([
        (
            "description".to_owned(),
            Value::String(description.to_owned()),
        ),
        ("exchanges".to_owned(), Value::Array(exchanges)),
    ]);
    if let Some(location) = value_at(
        dataset,
        &[
            "processInformation",
            "geography",
            "locationOfOperationSupplyOrProduction",
            "@location",
        ],
    )
    .and_then(scalar_text)
    {
        raw.insert("location".to_owned(), Value::String(location.to_owned()));
    }
    if let Some(year) = value_at(
        dataset,
        &["processInformation", "time", "common:referenceYear"],
    )
    .and_then(scalar_text)
    .and_then(|value| value.parse::<u32>().ok())
    {
        raw.insert("referenceYear".to_owned(), json!(year));
    }
    raw
}

fn process_exchange(exchange: &Map<String, Value>, index: usize) -> Option<Value> {
    let reference = exchange.get("referenceToFlowDataSet")?.as_object()?;
    let flow_id = field_text(reference, "@refObjectId")?;
    let flow_name = reference
        .get("common:shortDescription")
        .and_then(localized_text)
        .unwrap_or("Flow");
    let amount = field_text(exchange, "meanAmount")
        .or_else(|| field_text(exchange, "resultingAmount"))
        .unwrap_or("0");
    let direction = field_text(exchange, "exchangeDirection").unwrap_or("Output");
    Some(json!({
        "internalId": field_text(exchange, "@dataSetInternalID")
            .map_or_else(|| index.saturating_add(1).to_string(), ToOwned::to_owned),
        "flow": {"@id": flow_id, "name": flow_name},
        "flowRefId": flow_id,
        "flowName": flow_name,
        "isInput": direction.eq_ignore_ascii_case("input"),
        "amount": amount,
    }))
}

fn source_raw(information: &Value) -> Map<String, Value> {
    let mut raw = Map::new();
    if let Some(citation) = information.get("sourceCitation").and_then(scalar_text) {
        raw.insert(
            "textReference".to_owned(),
            Value::String(citation.to_owned()),
        );
    }
    raw
}

fn dataset_name(information: &Value, entity_type: &str) -> Option<String> {
    let name = if matches!(entity_type, "flows" | "processes") {
        value_at_value(information, &["name", "baseName"])
    } else {
        information.get("common:name")
    };
    name.and_then(localized_text)
        .or_else(|| information.get("common:shortName").and_then(localized_text))
        .map(ToOwned::to_owned)
}

fn classification_path(information: &Value) -> Vec<String> {
    value_at_value(
        information,
        &[
            "classificationInformation",
            "common:classification",
            "common:class",
        ],
    )
    .map(items)
    .unwrap_or_default()
    .into_iter()
    .filter_map(localized_text)
    .map(ToOwned::to_owned)
    .collect()
}

fn value_at<'a>(object: &'a Map<String, Value>, path: &[&str]) -> Option<&'a Value> {
    let (first, rest) = path.split_first()?;
    let mut current = object.get(*first)?;
    for field in rest {
        current = current.get(*field)?;
    }
    Some(current)
}

fn value_at_value<'a>(value: &'a Value, path: &[&str]) -> Option<&'a Value> {
    let mut current = value;
    for field in path {
        current = current.get(*field)?;
    }
    Some(current)
}

fn field_text<'a>(object: &'a Map<String, Value>, field: &str) -> Option<&'a str> {
    object.get(field).and_then(scalar_text)
}

fn field_text_value<'a>(value: &'a Value, field: &str) -> Option<&'a str> {
    value.get(field).and_then(scalar_text)
}

fn scalar_text(value: &Value) -> Option<&str> {
    value.as_str()
}

fn localized_text(value: &Value) -> Option<&str> {
    value
        .as_str()
        .or_else(|| value.get("#text").and_then(Value::as_str))
}

fn items(value: &Value) -> Vec<&Value> {
    value
        .as_array()
        .map_or_else(|| vec![value], |items| items.iter().collect())
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
    fn ilcd_identifiers_relations_and_values_survive_native_import() {
        let directory = tempdir().unwrap();
        let root = directory.path().join("ilcd");
        for category in ["unitgroups", "flowproperties", "flows", "processes"] {
            std::fs::create_dir_all(root.join(category)).unwrap();
        }
        let unit_id = "22222222-2222-4222-8222-222222222222";
        let property_id = "33333333-3333-4333-8333-333333333333";
        let flow_id = "44444444-4444-4444-8444-444444444444";
        let process_id = "55555555-5555-4555-8555-555555555555";
        std::fs::write(
            root.join("unitgroups/u.xml"),
            format!(
                r#"<unitGroupDataSet xmlns:common="http://lca.jrc.it/ILCD/Common"><unitGroupInformation><dataSetInformation><common:UUID>{unit_id}</common:UUID><common:name xml:lang="en">Units of mass</common:name></dataSetInformation><quantitativeReference><referenceToReferenceUnit>1</referenceToReferenceUnit></quantitativeReference></unitGroupInformation><units><unit dataSetInternalID="1"><name>kg</name><meanValue>1</meanValue></unit></units></unitGroupDataSet>"#
            ),
        )
        .unwrap();
        std::fs::write(
            root.join("flowproperties/p.xml"),
            format!(
                r#"<flowPropertyDataSet xmlns:common="http://lca.jrc.it/ILCD/Common"><flowPropertiesInformation><dataSetInformation><common:UUID>{property_id}</common:UUID><common:name xml:lang="en">Mass</common:name></dataSetInformation><quantitativeReference><referenceToReferenceUnitGroup refObjectId="{unit_id}"><common:shortDescription xml:lang="en">Units of mass</common:shortDescription></referenceToReferenceUnitGroup></quantitativeReference></flowPropertiesInformation></flowPropertyDataSet>"#
            ),
        )
        .unwrap();
        std::fs::write(
            root.join("flows/f.xml"),
            format!(
                r#"<flowDataSet xmlns:common="http://lca.jrc.it/ILCD/Common"><flowInformation><dataSetInformation><common:UUID>{flow_id}</common:UUID><name><baseName xml:lang="en">Steel</baseName></name></dataSetInformation></flowInformation><modellingAndValidation><LCIMethod><typeOfDataSet>Product flow</typeOfDataSet></LCIMethod></modellingAndValidation><flowProperties><flowProperty><referenceToFlowPropertyDataSet refObjectId="{property_id}"><common:shortDescription xml:lang="en">Mass</common:shortDescription></referenceToFlowPropertyDataSet></flowProperty></flowProperties></flowDataSet>"#
            ),
        )
        .unwrap();
        std::fs::write(
            root.join("processes/x.xml"),
            format!(
                r#"<processDataSet xmlns:common="http://lca.jrc.it/ILCD/Common"><processInformation><dataSetInformation><common:UUID>{process_id}</common:UUID><name><baseName xml:lang="en">Steel process</baseName></name></dataSetInformation><time><common:referenceYear>2022</common:referenceYear></time><geography><locationOfOperationSupplyOrProduction location="GLO"/></geography></processInformation><exchanges><exchange dataSetInternalID="1"><referenceToFlowDataSet refObjectId="{flow_id}"><common:shortDescription xml:lang="en">Steel</common:shortDescription></referenceToFlowDataSet><exchangeDirection>Output</exchangeDirection><meanAmount>1</meanAmount></exchange></exchanges></processDataSet>"#
            ),
        )
        .unwrap();
        let cancellation = CancellationToken::default();
        let memory_budget = MemoryBudget::new(16 * 1024 * 1024);
        let mut store = CanonicalStore::create(Some(directory.path())).unwrap();
        let mut issues = IssueSpool::new(Vec::new(), 64 * 1024);
        IlcdAdapter
            .read(
                &AdapterContext {
                    source: &root,
                    cancellation: &cancellation,
                    memory_budget: &memory_budget,
                    max_entry_bytes: 1024 * 1024,
                },
                &mut store,
                &mut issues,
            )
            .unwrap();
        issues.finish().unwrap();
        assert_eq!(
            store.get("flows", flow_id).unwrap().unwrap().raw["flowPropertyRefId"],
            property_id
        );
        assert_eq!(
            store.get("processes", process_id).unwrap().unwrap().raw["referenceYear"],
            2022
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
}
