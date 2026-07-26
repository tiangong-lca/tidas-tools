use std::collections::BTreeMap;

use serde_json::{Map, Value, json};

use crate::model::CanonicalEntity;

use super::common::{
    CONTACT_NAME, DEFAULT_VERSION, administrative, compliance_declarations, contact_id,
    dataset_ref, import_trace, localized, name_parts,
};

pub fn lifecycle_model(entity: &CanonicalEntity) -> Value {
    let process_refs = entity
        .raw
        .get("processRefs")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let instance_by_process = process_refs
        .iter()
        .enumerate()
        .filter_map(|(index, process)| {
            process
                .get("id")
                .and_then(Value::as_str)
                .map(|id| (id.to_owned(), index.saturating_add(1).to_string()))
        })
        .collect::<BTreeMap<_, _>>();
    let connections = connections_by_provider(entity, &instance_by_process);
    let instances = process_refs
        .iter()
        .filter_map(|process| process_instance(process, &instance_by_process, &connections))
        .collect::<Vec<_>>();
    let reference = entity
        .raw
        .get("referenceProcessId")
        .and_then(Value::as_str)
        .and_then(|id| instance_by_process.get(id))
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(1);
    let name = entity.name.as_deref().unwrap_or("Life cycle model");
    let description = entity
        .raw
        .get("description")
        .and_then(Value::as_str)
        .unwrap_or("Converted from external LCA package");
    let owner = dataset_ref("contact data set", &contact_id(), CONTACT_NAME, "contacts");
    let mut dataset_information = Map::from_iter([
        (
            "common:UUID".to_owned(),
            Value::String(entity.internal_id.clone()),
        ),
        ("name".to_owned(), name_parts(name, "GLO")),
        (
            "classificationInformation".to_owned(),
            default_classification(),
        ),
        ("common:generalComment".to_owned(), localized(description)),
    ]);
    if let Some(trace) = entity.raw.get("sourceTrace") {
        dataset_information.insert("common:other".to_owned(), import_trace(trace));
    }
    let administrative = administrative("lifecyclemodels", &entity.internal_id, true);
    json!({
        "lifeCycleModelDataSet": {
            "@xmlns": "http://eplca.jrc.ec.europa.eu/ILCD/LifeCycleModel/2017",
            "@xmlns:acme": "http://acme.com/custom",
            "@xmlns:common": "http://lca.jrc.it/ILCD/Common",
            "@xmlns:xsi": "http://www.w3.org/2001/XMLSchema-instance",
            "@locations": "../ILCDLocations.xml",
            "@version": "1.1",
            "@xsi:schemaLocation": "http://eplca.jrc.ec.europa.eu/ILCD/LifeCycleModel/2017 ../../schemas/ILCD_LifeCycleModelDataSet.xsd",
            "lifeCycleModelInformation": {
                "dataSetInformation": dataset_information,
                "quantitativeReference": {
                    "referenceToReferenceProcess": reference
                },
                "technology": {
                    "processes": {"processInstance": instances}
                }
            },
            "modellingAndValidation": {
                "validation": {
                    "review": {
                        "common:referenceToNameOfReviewerAndInstitution": owner,
                        "common:otherReviewDetails": localized("Source review state not declared")
                    }
                },
                "complianceDeclarations": compliance_declarations(true)
            },
            "administrativeInformation": {
                "common:commissionerAndGoal": {
                    "common:referenceToCommissioner": dataset_ref(
                        "contact data set",
                        &contact_id(),
                        CONTACT_NAME,
                        "contacts"
                    ),
                    "common:intendedApplications": localized(
                        "Converted from external LCA package"
                    )
                },
                "dataEntryBy": administrative["dataEntryBy"].clone(),
                "publicationAndOwnership": administrative["publicationAndOwnership"].clone()
            }
        }
    })
}

type ProviderConnections = BTreeMap<String, BTreeMap<String, Vec<Value>>>;

fn connections_by_provider(
    entity: &CanonicalEntity,
    instances: &BTreeMap<String, String>,
) -> ProviderConnections {
    let mut result = ProviderConnections::new();
    for connection in entity
        .raw
        .get("connections")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let Some(provider) = connection.get("providerProcessId").and_then(Value::as_str) else {
            continue;
        };
        let Some(consumer) = connection.get("consumerProcessId").and_then(Value::as_str) else {
            continue;
        };
        let Some(flow) = connection.get("flowRefId").and_then(Value::as_str) else {
            continue;
        };
        let (Some(_), Some(consumer_instance)) = (instances.get(provider), instances.get(consumer))
        else {
            continue;
        };
        result
            .entry(provider.to_owned())
            .or_default()
            .entry(flow.to_owned())
            .or_default()
            .push(json!({
                "@id": consumer_instance,
                "@flowUUID": flow,
                "@version": DEFAULT_VERSION,
            }));
    }
    result
}

fn process_instance(
    process: &Value,
    instances: &BTreeMap<String, String>,
    connections: &ProviderConnections,
) -> Option<Value> {
    let process_id = process.get("id")?.as_str()?;
    let instance_id = instances.get(process_id)?;
    let process_name = process
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or("Process");
    let mut instance = Map::from_iter([
        (
            "@dataSetInternalID".to_owned(),
            Value::String(instance_id.clone()),
        ),
        (
            "@multiplicationFactor".to_owned(),
            Value::String("1".to_owned()),
        ),
        (
            "referenceToProcess".to_owned(),
            dataset_ref("process data set", process_id, process_name, "processes"),
        ),
    ]);
    let outputs = connections
        .get(process_id)
        .into_iter()
        .flat_map(|by_flow| by_flow.iter())
        .map(|(flow, downstream)| {
            let downstream = if downstream.len() == 1 {
                downstream[0].clone()
            } else {
                Value::Array(downstream.clone())
            };
            json!({
                "@flowUUID": flow,
                "@version": DEFAULT_VERSION,
                "@dominant": "true",
                "downstreamProcess": downstream,
            })
        })
        .collect::<Vec<_>>();
    if !outputs.is_empty() {
        instance.insert(
            "connections".to_owned(),
            json!({
                "outputExchange": if outputs.len() == 1 {
                    outputs[0].clone()
                } else {
                    Value::Array(outputs)
                }
            }),
        );
    }
    instance.insert(
        "common:other".to_owned(),
        import_trace(&json!({
            "sourceProcessType": process.get("processType").cloned().unwrap_or(Value::Null),
            "sourceProcessId": process_id,
        })),
    );
    Some(Value::Object(instance))
}

fn default_classification() -> Value {
    json!({
        "common:classification": {
            "@name": "ISIC rev.4",
            "common:class": [
                {"@level": "0", "@classId": "T", "#text": "Other service activities"},
                {"@level": "1", "@classId": "94", "#text": "Activities of membership organizations"},
                {"@level": "2", "@classId": "949", "#text": "Activities of other membership organizations"},
                {"@level": "3", "@classId": "9499", "#text": "Activities of other membership organizations n.e.c."}
            ]
        }
    })
}
