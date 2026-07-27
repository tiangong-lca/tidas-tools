use serde_json::{Value, json};
use uuid::Uuid;

use crate::model::CanonicalEntity;

use super::FlowNormalizationError;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CanonicalFlowPropertyAssignment {
    pub flow_property_uuid: String,
    pub flow_property_name: String,
    pub version: Option<String>,
    pub conversion_factor: String,
    pub is_reference: bool,
    pub source_order: usize,
    pub source_evidence: Value,
}

pub(super) fn normalize(
    entity: &CanonicalEntity,
    source_object: &str,
) -> Result<Vec<CanonicalFlowPropertyAssignment>, FlowNormalizationError> {
    let mut assignments = entity
        .raw
        .get("flowProperties")
        .and_then(Value::as_array)
        .map(|entries| {
            entries
                .iter()
                .enumerate()
                .map(|(source_order, entry)| {
                    parse_entry(entity, source_object, source_order, entry)
                })
                .collect::<Result<Vec<_>, _>>()
        })
        .transpose()?
        .unwrap_or_default();
    if assignments.is_empty() {
        assignments.push(legacy_single_property(entity));
    }
    let references = assignments
        .iter()
        .filter(|assignment| assignment.is_reference)
        .count();
    if assignments.len() == 1 && references == 0 {
        assignments[0].is_reference = true;
        assignments[0].source_evidence = json!({
            "source": assignments[0].source_evidence,
            "referenceDecision": "single-property-source-contract"
        });
    } else if references != 1 {
        return Err(FlowNormalizationError::ReferencePropertyCardinality {
            flow_id: entity.internal_id.clone(),
            source_object: source_object.to_owned(),
            count: references,
        });
    }
    assignments.sort_by_key(|assignment| (!assignment.is_reference, assignment.source_order));
    Ok(assignments)
}

fn parse_entry(
    entity: &CanonicalEntity,
    source_object: &str,
    source_order: usize,
    entry: &Value,
) -> Result<CanonicalFlowPropertyAssignment, FlowNormalizationError> {
    let property = entry
        .get("flowProperty")
        .and_then(Value::as_object)
        .ok_or_else(|| {
            invalid(
                entity,
                source_object,
                source_order,
                "missing flowProperty object",
            )
        })?;
    let flow_property_uuid = property
        .get("@id")
        .or_else(|| property.get("@refObjectId"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            invalid(
                entity,
                source_object,
                source_order,
                "missing flow-property UUID",
            )
        })?
        .to_owned();
    let flow_property_name = property
        .get("name")
        .or_else(|| property.get("common:shortDescription"))
        .and_then(localized_text)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            invalid(
                entity,
                source_object,
                source_order,
                "missing flow-property name",
            )
        })?
        .to_owned();
    let conversion_factor = entry
        .get("conversionFactor")
        .or_else(|| entry.get("meanValue"))
        .and_then(decimal_text)
        .ok_or_else(|| {
            invalid(
                entity,
                source_object,
                source_order,
                "missing or non-decimal conversion factor",
            )
        })?;
    let version = property
        .get("@version")
        .or_else(|| property.get("version"))
        .and_then(Value::as_str)
        .map(ToOwned::to_owned);
    Ok(CanonicalFlowPropertyAssignment {
        flow_property_uuid,
        flow_property_name,
        version,
        conversion_factor,
        is_reference: entry
            .get("isRefFlowProperty")
            .or_else(|| entry.get("isReference"))
            .and_then(Value::as_bool)
            .unwrap_or(false),
        source_order,
        source_evidence: entry
            .get("sourceEvidence")
            .cloned()
            .unwrap_or_else(|| json!({"sourceObject": source_object, "sourceOrder": source_order})),
    })
}

fn legacy_single_property(entity: &CanonicalEntity) -> CanonicalFlowPropertyAssignment {
    let unit_name = entity
        .raw
        .get("unitName")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let flow_property_uuid = entity
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
    CanonicalFlowPropertyAssignment {
        flow_property_uuid,
        flow_property_name,
        version: None,
        conversion_factor: "1".to_owned(),
        is_reference: true,
        source_order: 0,
        source_evidence: json!({"referenceDecision": "legacy-single-property-source-contract"}),
    }
}

fn stable_id(seed: &str) -> String {
    Uuid::new_v5(&Uuid::NAMESPACE_URL, seed.as_bytes()).to_string()
}

fn localized_text(value: &Value) -> Option<&str> {
    value
        .as_str()
        .or_else(|| value.get("#text").and_then(Value::as_str))
}

fn decimal_text(value: &Value) -> Option<String> {
    match value {
        Value::String(value) if value.trim().parse::<bigdecimal::BigDecimal>().is_ok() => {
            Some(value.trim().to_owned())
        }
        Value::Number(value) => Some(value.to_string()),
        _ => None,
    }
}

fn invalid(
    entity: &CanonicalEntity,
    source_object: &str,
    source_order: usize,
    reason: &'static str,
) -> FlowNormalizationError {
    FlowNormalizationError::InvalidFlowProperty {
        flow_id: entity.internal_id.clone(),
        source_object: source_object.to_owned(),
        source_order,
        reason,
    }
}

#[cfg(test)]
mod tests {
    use serde_json::Map;

    use super::*;

    #[test]
    fn mass_and_ncv_keep_uuid_order_reference_and_exact_factor() {
        let entity = CanonicalEntity {
            entity_type: "flows".to_owned(),
            internal_id: "11111111-1111-4111-8111-111111111111".to_owned(),
            external_id: None,
            name: Some("Fuel".to_owned()),
            category_path: Vec::new(),
            raw: Map::from_iter([(
                "flowProperties".to_owned(),
                json!([
                    {
                        "flowProperty": {"@id": "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb", "name": "Mass"},
                        "conversionFactor": "1",
                        "isRefFlowProperty": true
                    },
                    {
                        "flowProperty": {"@id": "cccccccc-cccc-4ccc-8ccc-cccccccccccc", "name": "Net calorific value"},
                        "conversionFactor": "42.5",
                        "isRefFlowProperty": false
                    }
                ]),
            )]),
        };
        let values = normalize(&entity, "fixture").unwrap();
        assert_eq!(values.len(), 2);
        assert_eq!(values[0].flow_property_name, "Mass");
        assert!(values[0].is_reference);
        assert_eq!(values[1].flow_property_name, "Net calorific value");
        assert_eq!(values[1].conversion_factor, "42.5");
    }

    #[test]
    fn multi_property_reference_cardinality_is_strict() {
        for (first_reference, second_reference, expected_count) in
            [(false, false, 0), (true, true, 2)]
        {
            let entity = CanonicalEntity {
                entity_type: "flows".to_owned(),
                internal_id: "11111111-1111-4111-8111-111111111111".to_owned(),
                external_id: None,
                name: Some("Fuel".to_owned()),
                category_path: Vec::new(),
                raw: Map::from_iter([(
                    "flowProperties".to_owned(),
                    json!([
                        {
                            "flowProperty": {
                                "@id": "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb",
                                "name": "Mass"
                            },
                            "conversionFactor": "1",
                            "isRefFlowProperty": first_reference
                        },
                        {
                            "flowProperty": {
                                "@id": "cccccccc-cccc-4ccc-8ccc-cccccccccccc",
                                "name": "Net calorific value"
                            },
                            "conversionFactor": "42.5",
                            "isRefFlowProperty": second_reference
                        }
                    ]),
                )]),
            };
            assert!(matches!(
                normalize(&entity, "fixture"),
                Err(FlowNormalizationError::ReferencePropertyCardinality {
                    count,
                    ..
                }) if count == expected_count
            ));
        }
    }
}
