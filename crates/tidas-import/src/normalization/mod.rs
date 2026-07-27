mod elementary_taxonomy;
mod flow_name;
mod flow_properties;

use serde_json::Value;
use thiserror::Error;

use crate::model::CanonicalEntity;

pub(crate) use elementary_taxonomy::trace as classification_trace;
pub use elementary_taxonomy::{CanonicalCategory, CanonicalClassification};
pub use flow_name::CanonicalFlowName;
pub use flow_properties::CanonicalFlowPropertyAssignment;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FlowDatasetType {
    Elementary,
    Product,
    Waste,
    Other,
}

impl FlowDatasetType {
    #[must_use]
    pub fn from_source(value: Option<&str>) -> Self {
        let value = value.unwrap_or("PRODUCT_FLOW").to_ascii_uppercase();
        if value.contains("ELEMENTARY") {
            Self::Elementary
        } else if value.contains("WASTE") {
            Self::Waste
        } else if value.contains("OTHER") {
            Self::Other
        } else {
            Self::Product
        }
    }

    #[must_use]
    pub const fn as_tidas(self) -> &'static str {
        match self {
            Self::Elementary => "Elementary flow",
            Self::Product => "Product flow",
            Self::Waste => "Waste flow",
            Self::Other => "Other flow",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct CanonicalFlow {
    pub id: String,
    pub dataset_type: FlowDatasetType,
    pub name: CanonicalFlowName,
    pub classification: CanonicalClassification,
    pub flow_properties: Vec<CanonicalFlowPropertyAssignment>,
    pub reference_property_internal_id: String,
    pub version: Option<String>,
    pub synonyms: Option<String>,
    pub cas_number: Option<String>,
    pub sum_formula: Option<String>,
    pub source_trace: Value,
}

pub fn normalize_flow(entity: &CanonicalEntity) -> Result<CanonicalFlow, FlowNormalizationError> {
    let dataset_type =
        FlowDatasetType::from_source(entity.raw.get("flowType").and_then(Value::as_str));
    let source_object = source_object(entity);
    let name = flow_name::normalize(entity, dataset_type, &source_object)?;
    let classification = elementary_taxonomy::normalize(entity, dataset_type);
    let flow_properties = flow_properties::normalize(entity, &source_object)?;
    let reference_index = flow_properties
        .iter()
        .position(|property| property.is_reference)
        .expect("flow-property normalization proves one reference");
    let reference_property_internal_id = reference_index.saturating_add(1).to_string();
    Ok(CanonicalFlow {
        id: entity.internal_id.clone(),
        dataset_type,
        name,
        classification,
        flow_properties,
        reference_property_internal_id,
        version: string_field(entity, "version"),
        synonyms: string_field(entity, "synonyms"),
        cas_number: string_field(entity, "CASNumber"),
        sum_formula: string_field(entity, "sumFormula"),
        source_trace: entity
            .raw
            .get("sourceTrace")
            .cloned()
            .unwrap_or(Value::Null),
    })
}

fn string_field(entity: &CanonicalEntity, field: &str) -> Option<String> {
    entity
        .raw
        .get(field)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn source_object(entity: &CanonicalEntity) -> String {
    let trace = entity.raw.get("sourceTrace");
    let format = trace
        .and_then(|trace| trace.get("format"))
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    let object = trace
        .and_then(|trace| trace.get("sourceObject"))
        .and_then(Value::as_str)
        .unwrap_or("flow");
    format!("{format}:{object}:{}", entity.internal_id)
}

#[derive(Debug, Error)]
pub enum FlowNormalizationError {
    #[error(
        "flow import preflight failed for {flow_id} ({source_object}): missing canonical field {canonical_path}: {reason}"
    )]
    MissingFact {
        flow_id: String,
        source_object: String,
        canonical_path: &'static str,
        reason: &'static str,
    },
    #[error(
        "flow import preflight failed for {flow_id} ({source_object}): {count} reference flow properties were observed at CanonicalFlow.flowProperties"
    )]
    ReferencePropertyCardinality {
        flow_id: String,
        source_object: String,
        count: usize,
    },
    #[error(
        "flow import preflight failed for {flow_id} ({source_object}): invalid flow property at source order {source_order}: {reason}"
    )]
    InvalidFlowProperty {
        flow_id: String,
        source_object: String,
        source_order: usize,
        reason: &'static str,
    },
}
