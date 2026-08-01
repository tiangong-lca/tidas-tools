use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use serde_json::{Map, Value};
use tidas_assets::{AssetKind, bundled_assets};

use crate::ConversionError;

const TIDAS_SCHEMA_PREFIX: &str = "assets/tidas/schemas/";

/// Reusable schema-ordering catalog for TIDAS JSON documents.
pub struct TidasSchemaOrderer {
    catalog: BTreeMap<String, Value>,
}

impl TidasSchemaOrderer {
    /// Load the integrity-locked English TIDAS schemas embedded in the binary.
    pub fn from_bundled_assets() -> Result<Self, ConversionError> {
        let catalog = bundled_assets()
            .into_iter()
            .filter(|asset| {
                asset.kind == AssetKind::JsonSchema && asset.path.starts_with(TIDAS_SCHEMA_PREFIX)
            })
            .map(|asset| {
                let value = serde_json::from_slice(asset.bytes)?;
                Ok((asset.path, value))
            })
            .collect::<Result<_, serde_json::Error>>()?;
        Ok(Self { catalog })
    }

    /// Return a copy whose object members follow the declared schema property order.
    pub fn order_document(
        &self,
        document: &Value,
        category: &str,
    ) -> Result<Value, ConversionError> {
        let schema_path = format!("{TIDAS_SCHEMA_PREFIX}tidas_{category}.json");
        let schema = self
            .catalog
            .get(&schema_path)
            .ok_or_else(|| ConversionError::OrderingSchemaMissing(category.to_owned()))?;
        order_value(document, schema, &schema_path, &self.catalog)
    }
}

fn resolve_schema(
    schema: &Value,
    schema_path: &str,
    catalog: &BTreeMap<String, Value>,
) -> Result<(Value, String), ConversionError> {
    let mut current = schema.clone();
    let mut current_path = schema_path.to_owned();
    let mut seen = BTreeSet::new();
    loop {
        let Some(reference) = current.get("$ref").and_then(Value::as_str) else {
            return Ok((current, current_path));
        };
        let (file_name, fragment) = reference.split_once('#').unwrap_or((reference, ""));
        let target_path = if file_name.is_empty() {
            current_path.clone()
        } else {
            let parent = Path::new(&current_path)
                .parent()
                .unwrap_or_else(|| Path::new(""));
            portable_path(&parent.join(file_name))?
        };
        if !seen.insert((target_path.clone(), fragment.to_owned())) {
            return Err(ConversionError::OrderingSchemaCycle(reference.to_owned()));
        }
        let target = catalog
            .get(&target_path)
            .ok_or_else(|| ConversionError::OrderingSchemaReference(reference.to_owned()))?;
        current = if fragment.is_empty() {
            target.clone()
        } else {
            target
                .pointer(fragment)
                .cloned()
                .ok_or_else(|| ConversionError::OrderingSchemaReference(reference.to_owned()))?
        };
        current_path = target_path;
    }
}

fn select_schema(
    schema: &Value,
    schema_path: &str,
    value: &Value,
    catalog: &BTreeMap<String, Value>,
) -> Result<(Value, String), ConversionError> {
    let (resolved, resolved_path) = resolve_schema(schema, schema_path, catalog)?;
    if let Some(alternatives) = resolved
        .get("oneOf")
        .or_else(|| resolved.get("anyOf"))
        .and_then(Value::as_array)
    {
        for candidate in alternatives {
            if schema_matches(candidate, &resolved_path, value, catalog)? {
                return select_schema(candidate, &resolved_path, value, catalog);
            }
        }
    }
    Ok((resolved, resolved_path))
}

fn schema_matches(
    schema: &Value,
    schema_path: &str,
    value: &Value,
    catalog: &BTreeMap<String, Value>,
) -> Result<bool, ConversionError> {
    let (resolved, resolved_path) = resolve_schema(schema, schema_path, catalog)?;
    if let Some(alternatives) = resolved
        .get("oneOf")
        .or_else(|| resolved.get("anyOf"))
        .and_then(Value::as_array)
    {
        return alternatives
            .iter()
            .map(|candidate| schema_matches(candidate, &resolved_path, value, catalog))
            .try_fold(false, |matched, candidate| {
                candidate.map(|value| matched || value)
            });
    }
    let types: Vec<&str> = match resolved.get("type") {
        Some(Value::String(value)) => vec![value],
        Some(Value::Array(values)) => values.iter().filter_map(Value::as_str).collect(),
        _ => Vec::new(),
    };
    Ok(types.is_empty()
        || match value {
            Value::Object(_) => types.contains(&"object") || resolved.get("properties").is_some(),
            Value::Array(_) => types.contains(&"array") || resolved.get("items").is_some(),
            Value::Null => types.contains(&"null"),
            Value::Bool(_) => types.contains(&"boolean"),
            Value::Number(_) => types.contains(&"number") || types.contains(&"integer"),
            Value::String(_) => types.contains(&"string"),
        })
}

fn property_schemas(
    schema: &Value,
    schema_path: &str,
    value: &Value,
    catalog: &BTreeMap<String, Value>,
) -> Result<Vec<(String, Value, String)>, ConversionError> {
    let (resolved, resolved_path) = select_schema(schema, schema_path, value, catalog)?;
    let mut properties = Vec::new();
    if let Some(all_of) = resolved.get("allOf").and_then(Value::as_array) {
        for component in all_of {
            for item in property_schemas(component, &resolved_path, value, catalog)? {
                if let Some(position) = properties.iter().position(|(name, _, _)| name == &item.0) {
                    properties.remove(position);
                }
                properties.push(item);
            }
        }
    }
    if let Some(object) = resolved.get("properties").and_then(Value::as_object) {
        for (name, child) in object {
            if let Some(position) = properties
                .iter()
                .position(|(existing, _, _)| existing == name)
            {
                properties.remove(position);
            }
            properties.push((name.clone(), child.clone(), resolved_path.clone()));
        }
    }
    Ok(properties)
}

fn order_value(
    value: &Value,
    schema: &Value,
    schema_path: &str,
    catalog: &BTreeMap<String, Value>,
) -> Result<Value, ConversionError> {
    match value {
        Value::Array(items) => {
            let (resolved, resolved_path) = select_schema(schema, schema_path, value, catalog)?;
            let Some(item_schema) = resolved.get("items") else {
                return Ok(value.clone());
            };
            items
                .iter()
                .map(|item| order_value(item, item_schema, &resolved_path, catalog))
                .collect::<Result<Vec<_>, _>>()
                .map(Value::Array)
        }
        Value::Object(object) => {
            let properties = property_schemas(schema, schema_path, value, catalog)?;
            let mut ordered = Map::new();
            for (name, child_schema, child_path) in properties {
                if let Some(child) = object.get(&name) {
                    ordered.insert(
                        name,
                        order_value(child, &child_schema, &child_path, catalog)?,
                    );
                }
            }
            let mut remaining: Vec<&String> = object
                .keys()
                .filter(|name| !ordered.contains_key(*name))
                .collect();
            remaining.sort();
            for name in remaining {
                ordered.insert(name.clone(), object[name].clone());
            }
            Ok(Value::Object(ordered))
        }
        _ => Ok(value.clone()),
    }
}

fn portable_path(path: &Path) -> Result<String, ConversionError> {
    path.components()
        .map(|component| {
            component
                .as_os_str()
                .to_str()
                .ok_or_else(|| ConversionError::NonPortablePath(path.to_path_buf()))
        })
        .collect::<Result<Vec<_>, _>>()
        .map(|components| components.join("/"))
}
