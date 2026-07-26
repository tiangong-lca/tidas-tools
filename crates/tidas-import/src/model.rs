use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EntityRef {
    pub target_type: String,
    pub target_id: Option<String>,
    pub source_label: Option<String>,
    pub resolved: bool,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CanonicalEntity {
    pub entity_type: String,
    pub internal_id: String,
    pub external_id: Option<String>,
    pub name: Option<String>,
    #[serde(default)]
    pub category_path: Vec<String>,
    #[serde(default)]
    pub raw: serde_json::Map<String, serde_json::Value>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CanonicalExchange {
    pub internal_id: String,
    pub flow_ref: EntityRef,
    pub direction: String,
    pub amount: Option<String>,
    pub formula: Option<String>,
    pub unit_ref: Option<EntityRef>,
    pub flow_property_ref: Option<EntityRef>,
    pub provider_ref: Option<EntityRef>,
    pub location: Option<String>,
    pub uncertainty: Option<serde_json::Map<String, serde_json::Value>>,
    pub dq_entry: Option<String>,
    #[serde(default)]
    pub raw: serde_json::Map<String, serde_json::Value>,
}
