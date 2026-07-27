use serde_json::{Map, Value, json};

use crate::model::CanonicalEntity;
use crate::normalization::{CanonicalClassification, CanonicalFlow, FlowDatasetType};

use super::common::{
    DEFAULT_VERSION, administrative_for_entity, administrative_version, compliance_declarations,
    dataset_ref, dataset_ref_version, import_trace, localized, stable_id,
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
            "administrativeInformation": administrative_for_entity("unitgroups", entity, false),
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
            "administrativeInformation": administrative_for_entity(
                "flowproperties", entity, false
            )
        }
    })
}

pub fn flow(flow: &CanonicalFlow) -> Value {
    let dataset_type = flow.dataset_type.as_tidas();
    let classification = flow_classification(&flow.classification, flow.dataset_type);
    let name = flow_name(flow);
    let properties = flow
        .flow_properties
        .iter()
        .enumerate()
        .map(|(index, property)| {
            json!({
                "@dataSetInternalID": index.saturating_add(1).to_string(),
                "referenceToFlowPropertyDataSet": dataset_ref_version(
                    "flow property data set",
                    &property.flow_property_uuid,
                    &property.flow_property_name,
                    "flowproperties",
                    property.version.as_deref(),
                ),
                "meanValue": property.conversion_factor,
                "common:other": import_trace(&property.source_evidence),
            })
        })
        .collect::<Vec<_>>();
    let properties = if properties.len() == 1 {
        properties.into_iter().next().expect("one flow property")
    } else {
        Value::Array(properties)
    };
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
                    "common:UUID": flow.id,
                    "name": name,
                    "classificationInformation": classification
                },
                "quantitativeReference": {
                    "referenceToReferenceFlowProperty": flow.reference_property_internal_id
                }
            },
            "modellingAndValidation": {
                "LCIMethod": {"typeOfDataSet": dataset_type},
                "complianceDeclarations": compliance_declarations(false)
            },
            "administrativeInformation": administrative_version(
                "flows",
                &flow.id,
                false,
                flow.version.as_deref().unwrap_or(DEFAULT_VERSION),
            ),
            "flowProperties": {
                "flowProperty": properties
            }
        }
    });
    let dataset_information = document
        .pointer_mut("/flowDataSet/flowInformation/dataSetInformation")
        .and_then(Value::as_object_mut)
        .expect("native flow dataset information is an object");
    if let Some(synonyms) = flow.synonyms.as_deref() {
        let classification = dataset_information
            .remove("classificationInformation")
            .expect("native flow classification exists");
        dataset_information.insert("common:synonyms".to_owned(), localized(synonyms));
        dataset_information.insert("classificationInformation".to_owned(), classification);
    }
    if let Some(cas_number) = flow.cas_number.as_deref() {
        dataset_information.insert("CASNumber".to_owned(), Value::String(cas_number.to_owned()));
    }
    if let Some(formula) = flow.sum_formula.as_deref() {
        dataset_information.insert("sumFormula".to_owned(), Value::String(formula.to_owned()));
    }
    if !flow.source_trace.is_null() {
        dataset_information.insert("common:other".to_owned(), import_trace(&flow.source_trace));
    }
    document
}

fn flow_name(flow: &CanonicalFlow) -> Value {
    let mut name = Map::from_iter([("baseName".to_owned(), localized(&flow.name.base_name))]);
    for (field, value) in [
        (
            "treatmentStandardsRoutes",
            flow.name.treatment_standards_routes.as_deref(),
        ),
        (
            "mixAndLocationTypes",
            flow.name.mix_and_location_types.as_deref(),
        ),
        ("flowProperties", flow.name.flow_properties.as_deref()),
    ] {
        if let Some(value) = value {
            name.insert(field.to_owned(), localized(value));
        }
    }
    Value::Object(name)
}

fn flow_classification(
    classification: &CanonicalClassification,
    dataset_type: FlowDatasetType,
) -> Value {
    let trace = import_trace(&crate::normalization::classification_trace(
        classification,
        dataset_type.as_tidas(),
    ));
    if dataset_type == FlowDatasetType::Elementary {
        let categories = classification
            .categories
            .iter()
            .map(|category| {
                json!({
                    "@level": category.level,
                    "@catId": category.category_id,
                    "#text": category.label,
                })
            })
            .collect::<Vec<_>>();
        json!({
            "common:elementaryFlowCategorization": {
                "common:category": categories,
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
        "unitId",
        "unitName",
        "providerRefId",
        "providerName",
        "flowPropertyRefId",
        "flowPropertyName",
        "location",
        "sourceLocation",
        "dqEntry",
        "amountFormula",
        "isQuantitativeReference",
        "isAvoidedProduct",
        "sourceAmount",
        "sourceUnitId",
        "sourceUnitName",
        "sourceFlowPropertyRefId",
        "sourceFlowPropertyName",
        "amountNormalization",
        "activityLinkId",
        "productionVolumeAmount",
        "sourceExchangeNumber",
        "sourceExchangeId",
        "sourceIdentifiers",
        "sourceClassification",
    ] {
        if let Some(value) = exchange.get(field).cloned() {
            metadata.insert(field.to_owned(), value);
        }
    }
    import_trace(&json!({
        "sourceTrace": exchange.get("sourceTrace").cloned().unwrap_or(Value::Null),
        "exchangeMetadata": metadata
    }))
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

    #[test]
    fn ilcd_elementary_compartment_uses_the_locked_tidas_category() {
        let entity = CanonicalEntity {
            entity_type: "flows".to_owned(),
            internal_id: "218d3b51-1339-4389-b4f9-f4fbe8deea46".to_owned(),
            external_id: None,
            name: Some("Lead-210".to_owned()),
            category_path: Vec::new(),
            raw: Map::from_iter([
                (
                    "flowType".to_owned(),
                    Value::String("ELEMENTARY_FLOW".to_owned()),
                ),
                (
                    "elementaryCategorization".to_owned(),
                    json!([
                        {"level": "0", "text": "Emissions"},
                        {"level": "1", "text": "Emissions to water"},
                        {"level": "2", "text": "Emissions to fresh water"}
                    ]),
                ),
            ]),
        };
        let value = flow(&crate::normalization::normalize_flow(&entity).unwrap());
        let categories = &value["flowDataSet"]["flowInformation"]["dataSetInformation"]["classificationInformation"]
            ["common:elementaryFlowCategorization"]["common:category"];
        assert_eq!(categories[2]["@catId"], "1.1.1");
        assert_eq!(categories[2]["#text"], "Emissions to fresh water");
    }
}
