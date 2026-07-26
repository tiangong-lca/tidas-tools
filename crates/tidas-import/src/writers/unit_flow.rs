use serde_json::{Map, Value, json};

use crate::model::CanonicalEntity;

use super::common::{
    administrative, compliance_declarations, dataset_ref, import_trace, localized, name_parts,
    stable_id,
};

pub fn unit_group(entity: &CanonicalEntity) -> Value {
    let units = entity
        .raw
        .get("units")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_else(|| {
            vec![json!({
                "name": entity.name.as_deref().unwrap_or("unit"),
                "conversionFactor": 1.0
            })]
        });
    let reference_index = units
        .iter()
        .position(|unit| {
            unit.get("referenceUnit")
                .and_then(Value::as_bool)
                .unwrap_or(false)
        })
        .or_else(|| {
            units.iter().position(|unit| {
                unit.get("conversionFactor")
                    .or_else(|| unit.get("meanValue"))
                    .is_some_and(is_one)
            })
        })
        .unwrap_or(0);
    let reference = reference_index.saturating_add(1).to_string();
    let items = units
        .iter()
        .enumerate()
        .map(|(index, unit)| {
            let internal_id = index.saturating_add(1).to_string();
            let factor = unit
                .get("conversionFactor")
                .or_else(|| unit.get("meanValue"))
                .cloned()
                .unwrap_or_else(|| json!(1));
            json!({
                "@dataSetInternalID": internal_id,
                "name": unit.get("name")
                    .or_else(|| unit.get("refUnit"))
                    .and_then(Value::as_str)
                    .unwrap_or("unit"),
                "meanValue": real_string(&factor),
            })
        })
        .collect::<Vec<_>>();
    let name = entity.name.as_deref().unwrap_or("Unit group");
    json!({
        "unitGroupDataSet": {
            "@xmlns": "http://lca.jrc.it/ILCD/UnitGroup",
            "@xmlns:common": "http://lca.jrc.it/ILCD/Common",
            "@xmlns:xsi": "http://www.w3.org/2001/XMLSchema-instance",
            "@version": "1.1",
            "@xsi:schemaLocation": "http://lca.jrc.it/ILCD/UnitGroup ../../schemas/ILCD_UnitGroupDataSet.xsd",
            "unitGroupInformation": {
                "dataSetInformation": {
                    "common:UUID": entity.internal_id,
                    "common:name": localized(name),
                    "classificationInformation": {
                        "common:classification": {
                            "common:class": {
                                "@level": "0",
                                "@classId": "4",
                                "#text": "Other unit groups"
                            }
                        }
                    }
                },
                "quantitativeReference": {"referenceToReferenceUnit": reference}
            },
            "modellingAndValidation": {
                "complianceDeclarations": compliance_declarations(false)
            },
            "administrativeInformation": administrative("unitgroups", &entity.internal_id, false),
            "units": {"unit": items}
        }
    })
}

pub fn flow_property(entity: &CanonicalEntity) -> Value {
    let unit_group_id = entity
        .raw
        .get("unitGroupRefId")
        .and_then(Value::as_str)
        .map_or_else(
            || stable_id("tidas-tools/import/default-unit-group"),
            ToOwned::to_owned,
        );
    let unit_group_name = entity
        .raw
        .get("unitGroupName")
        .and_then(Value::as_str)
        .unwrap_or("Unit group");
    let name = entity.name.as_deref().unwrap_or("Flow property");
    json!({
        "flowPropertyDataSet": {
            "@xmlns": "http://lca.jrc.it/ILCD/FlowProperty",
            "@xmlns:common": "http://lca.jrc.it/ILCD/Common",
            "@xmlns:xsi": "http://www.w3.org/2001/XMLSchema-instance",
            "@version": "1.1",
            "@xsi:schemaLocation": "http://lca.jrc.it/ILCD/FlowProperty ../../schemas/ILCD_FlowPropertyDataSet.xsd",
            "flowPropertiesInformation": {
                "dataSetInformation": {
                    "common:UUID": entity.internal_id,
                    "common:name": localized(name),
                    "classificationInformation": {
                        "common:classification": {
                            "common:class": {
                                "@level": "0",
                                "@classId": "4",
                                "#text": "Other flow properties"
                            }
                        }
                    }
                },
                "quantitativeReference": {
                    "referenceToReferenceUnitGroup": dataset_ref(
                        "unit group data set",
                        &unit_group_id,
                        unit_group_name,
                        "unitgroups"
                    )
                }
            },
            "modellingAndValidation": {
                "complianceDeclarations": compliance_declarations(false)
            },
            "administrativeInformation": administrative(
                "flowproperties",
                &entity.internal_id,
                false
            )
        }
    })
}

pub fn flow(entity: &CanonicalEntity) -> Value {
    let source_type = entity
        .raw
        .get("flowType")
        .and_then(Value::as_str)
        .unwrap_or("PRODUCT_FLOW");
    let dataset_type = if source_type.contains("ELEMENTARY") {
        "Elementary flow"
    } else if source_type.contains("WASTE") {
        "Waste flow"
    } else {
        "Product flow"
    };
    let name = entity.name.as_deref().unwrap_or("Flow");
    let unit_name = entity
        .raw
        .get("unitName")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let flow_property_id = entity
        .raw
        .get("flowPropertyRefId")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
        .or_else(|| {
            unit_name.map(|unit| stable_id(&format!("tidas-tools/import/flowproperty/{unit}")))
        })
        .unwrap_or_else(|| stable_id("tidas-tools/import/default-flow-property"));
    let flow_property_name = entity
        .raw
        .get("flowPropertyName")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
        .or_else(|| unit_name.map(|unit| format!("Amount in {unit}")))
        .unwrap_or_else(|| "Flow property".to_owned());
    let classification = flow_classification(dataset_type, source_type);
    let mut document = json!({
        "flowDataSet": {
            "@xmlns": "http://lca.jrc.it/ILCD/Flow",
            "@xmlns:common": "http://lca.jrc.it/ILCD/Common",
            "@xmlns:ecn": "http://eplca.jrc.ec.europa.eu/ILCD/Extensions/2018/ECNumber",
            "@xmlns:xsi": "http://www.w3.org/2001/XMLSchema-instance",
            "@version": "1.1",
            "@locations": "../ILCDLocations.xml",
            "@xsi:schemaLocation": "http://lca.jrc.it/ILCD/Flow ../../schemas/ILCD_FlowDataSet.xsd",
            "flowInformation": {
                "dataSetInformation": {
                    "common:UUID": entity.internal_id,
                    "name": name_parts(name, "source-described geography"),
                    "classificationInformation": classification
                },
                "quantitativeReference": {"referenceToReferenceFlowProperty": "1"}
            },
            "modellingAndValidation": {
                "LCIMethod": {"typeOfDataSet": dataset_type},
                "complianceDeclarations": compliance_declarations(false)
            },
            "administrativeInformation": administrative("flows", &entity.internal_id, false),
            "flowProperties": {
                "flowProperty": {
                    "@dataSetInternalID": "1",
                    "referenceToFlowPropertyDataSet": dataset_ref(
                        "flow property data set",
                        &flow_property_id,
                        &flow_property_name,
                        "flowproperties"
                    ),
                    "meanValue": "1"
                }
            }
        }
    });
    let dataset_information = document
        .pointer_mut("/flowDataSet/flowInformation/dataSetInformation")
        .and_then(Value::as_object_mut)
        .expect("native flow dataset information is an object");
    if let Some(cas_number) = entity.raw.get("CASNumber").and_then(Value::as_str) {
        dataset_information.insert("CASNumber".to_owned(), Value::String(cas_number.to_owned()));
    }
    if let Some(formula) = entity.raw.get("sumFormula").and_then(Value::as_str) {
        dataset_information.insert("sumFormula".to_owned(), Value::String(formula.to_owned()));
    }
    document
}

fn flow_classification(dataset_type: &str, source_type: &str) -> Value {
    let trace = import_trace(&json!({"sourceFlowType": source_type}));
    if dataset_type == "Elementary flow" {
        json!({
            "common:elementaryFlowCategorization": {
                "common:category": [
                    {"@level": "0", "@catId": "1", "#text": "Emissions"},
                    {"@level": "1", "@catId": "1.3", "#text": "Emissions to air"},
                    {"@level": "2", "@catId": "1.3.4", "#text": "Emissions to air, unspecified"}
                ],
                "common:other": trace
            }
        })
    } else {
        json!({
            "common:classification": {
                "@name": "CPC",
                "common:class": [
                    {"@level": "0", "@classId": "9", "#text": "Community, social and personal services"},
                    {"@level": "1", "@classId": "94", "#text": "Sewage and waste collection, treatment and disposal and other environmental protection services"},
                    {"@level": "2", "@classId": "949", "#text": "Other environmental protection services n.e.c."},
                    {"@level": "3", "@classId": "9490", "#text": "Other environmental protection services n.e.c."},
                    {"@level": "4", "@classId": "94900", "#text": "Other environmental protection services n.e.c."}
                ],
                "common:other": trace
            }
        })
    }
}

fn real_string(value: &Value) -> String {
    match value {
        Value::String(value) => value.clone(),
        Value::Number(value) => value.to_string(),
        _ => "1".to_owned(),
    }
}

fn is_one(value: &Value) -> bool {
    value
        .as_f64()
        .or_else(|| value.as_str().and_then(|text| text.parse::<f64>().ok()))
        == Some(1.0)
}

pub fn exchange_metadata(exchange: &Map<String, Value>) -> Value {
    let mut metadata = Map::new();
    for field in [
        "unitName",
        "providerRefId",
        "flowPropertyRefId",
        "flowPropertyName",
        "location",
    ] {
        if let Some(value) = exchange.get(field).cloned() {
            metadata.insert(field.to_owned(), value);
        }
    }
    import_trace(&json!({"exchangeMetadata": metadata}))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_reference_unit_wins_over_other_factor_one_units() {
        let entity = CanonicalEntity {
            entity_type: "unitgroups".to_owned(),
            internal_id: "11111111-1111-4111-8111-111111111111".to_owned(),
            external_id: None,
            name: Some("Time".to_owned()),
            category_path: Vec::new(),
            raw: Map::from_iter([(
                "units".to_owned(),
                json!([
                    {"name": "hr", "conversionFactor": "1.0", "referenceUnit": false},
                    {"name": "a", "conversionFactor": "8760.0", "referenceUnit": true}
                ]),
            )]),
        };
        let value = unit_group(&entity);
        assert_eq!(
            value["unitGroupDataSet"]["unitGroupInformation"]["quantitativeReference"]["referenceToReferenceUnit"],
            "2"
        );
    }
}
