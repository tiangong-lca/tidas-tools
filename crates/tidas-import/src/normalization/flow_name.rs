use serde_json::Value;

use crate::model::CanonicalEntity;

use super::{FlowDatasetType, FlowNormalizationError};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CanonicalFlowName {
    pub base_name: String,
    pub treatment_standards_routes: Option<String>,
    pub mix_and_location_types: Option<String>,
    pub flow_properties: Option<String>,
}

pub(super) fn normalize(
    entity: &CanonicalEntity,
    dataset_type: FlowDatasetType,
    source_object: &str,
) -> Result<CanonicalFlowName, FlowNormalizationError> {
    let base_name = entity
        .name
        .as_deref()
        .map(str::trim)
        .filter(|value| !is_placeholder(value))
        .ok_or_else(|| FlowNormalizationError::MissingFact {
            flow_id: entity.internal_id.clone(),
            source_object: source_object.to_owned(),
            canonical_path: "CanonicalFlow.name.baseName",
            reason: "the source did not provide a non-placeholder base name",
        })?
        .to_owned();
    let parts = entity.raw.get("flowName").and_then(Value::as_object);
    let treatment_standards_routes = part(parts, "treatmentStandardsRoutes");
    let mix_and_location_types = part(parts, "mixAndLocationTypes");
    let flow_properties = part(parts, "flowProperties");
    if dataset_type == FlowDatasetType::Elementary {
        return Ok(CanonicalFlowName {
            base_name,
            treatment_standards_routes: None,
            mix_and_location_types: None,
            flow_properties: None,
        });
    }
    if treatment_standards_routes.is_none() {
        return Err(FlowNormalizationError::MissingFact {
            flow_id: entity.internal_id.clone(),
            source_object: source_object.to_owned(),
            canonical_path: "CanonicalFlow.name.treatmentStandardsRoutes",
            reason: "Product, Waste, and Other flows require source-backed route facts",
        });
    }
    if mix_and_location_types.is_none() {
        return Err(FlowNormalizationError::MissingFact {
            flow_id: entity.internal_id.clone(),
            source_object: source_object.to_owned(),
            canonical_path: "CanonicalFlow.name.mixAndLocationTypes",
            reason: "Product, Waste, and Other flows require source-backed mix/location facts",
        });
    }
    Ok(CanonicalFlowName {
        base_name,
        treatment_standards_routes,
        mix_and_location_types,
        flow_properties,
    })
}

fn part(parts: Option<&serde_json::Map<String, Value>>, field: &str) -> Option<String> {
    parts?
        .get(field)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !is_placeholder(value))
        .map(ToOwned::to_owned)
}

fn is_placeholder(value: &str) -> bool {
    matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "" | "-" | "n/a" | "source-described route" | "source-described geography"
    )
}

#[cfg(test)]
mod tests {
    use serde_json::Map;

    use super::*;

    fn entity(flow_type: &str, raw: Map<String, Value>) -> CanonicalEntity {
        let mut raw = raw;
        raw.insert("flowType".to_owned(), Value::String(flow_type.to_owned()));
        CanonicalEntity {
            entity_type: "flows".to_owned(),
            internal_id: "11111111-1111-4111-8111-111111111111".to_owned(),
            external_id: None,
            name: Some("Carbon dioxide".to_owned()),
            category_path: Vec::new(),
            raw,
        }
    }

    #[test]
    fn elementary_flow_requires_only_base_name() {
        let value = entity(
            "ELEMENTARY_FLOW",
            Map::from_iter([(
                "flowName".to_owned(),
                serde_json::json!({
                    "treatmentStandardsRoutes": "must be removed",
                    "mixAndLocationTypes": "must be removed",
                    "flowProperties": "must be removed"
                }),
            )]),
        );
        let normalized = normalize(&value, FlowDatasetType::Elementary, "fixture").unwrap();
        assert_eq!(normalized.base_name, "Carbon dioxide");
        assert_eq!(normalized.treatment_standards_routes, None);
        assert_eq!(normalized.mix_and_location_types, None);
    }

    #[test]
    fn non_elementary_placeholder_is_rejected() {
        let value = entity(
            "PRODUCT_FLOW",
            Map::from_iter([(
                "flowName".to_owned(),
                serde_json::json!({
                    "treatmentStandardsRoutes": "source-described route",
                    "mixAndLocationTypes": "GLO"
                }),
            )]),
        );
        assert!(matches!(
            normalize(&value, FlowDatasetType::Product, "fixture"),
            Err(FlowNormalizationError::MissingFact {
                canonical_path: "CanonicalFlow.name.treatmentStandardsRoutes",
                ..
            })
        ));
    }
}
