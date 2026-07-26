use serde_json::{Map, Value, json};
use uuid::Uuid;

use crate::model::CanonicalEntity;
use crate::store::{CanonicalStore, StoreError};

pub fn add_for_flow(store: &mut CanonicalStore, flow: &CanonicalEntity) -> Result<(), StoreError> {
    if flow.raw.contains_key("flowPropertyRefId") {
        return Ok(());
    }
    let Some(unit_name) = flow
        .raw
        .get("unitName")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return Ok(());
    };
    let unit_group_id = stable_id(&format!("tidas-tools/import/unitgroup/{unit_name}"));
    let flow_property_id = stable_id(&format!("tidas-tools/import/flowproperty/{unit_name}"));
    store.add(&CanonicalEntity {
        entity_type: "unitgroups".to_owned(),
        internal_id: unit_group_id.clone(),
        external_id: None,
        name: Some(format!("Units of {unit_name}")),
        category_path: Vec::new(),
        raw: Map::from_iter([(
            "units".to_owned(),
            json!([{"name": unit_name, "conversionFactor": 1.0}]),
        )]),
    })?;
    store.add(&CanonicalEntity {
        entity_type: "flowproperties".to_owned(),
        internal_id: flow_property_id,
        external_id: None,
        name: Some(format!("Amount in {unit_name}")),
        category_path: Vec::new(),
        raw: Map::from_iter([
            ("unitGroupRefId".to_owned(), Value::String(unit_group_id)),
            (
                "unitGroupName".to_owned(),
                Value::String(format!("Units of {unit_name}")),
            ),
        ]),
    })?;
    Ok(())
}

fn stable_id(seed: &str) -> String {
    Uuid::new_v5(&Uuid::NAMESPACE_URL, seed.as_bytes()).to_string()
}
