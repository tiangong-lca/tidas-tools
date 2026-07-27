use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::sync::OnceLock;

use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::model::CanonicalEntity;

use super::FlowDatasetType;

const BASE_TAXONOMY_ID: &str = "ilcd-flow-categorization";
const BASE_TAXONOMY_VERSION: &str = "1.1";

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CanonicalCategory {
    pub level: String,
    pub category_id: String,
    pub label: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CanonicalClassification {
    pub categories: Vec<CanonicalCategory>,
    pub taxonomy_id: String,
    pub taxonomy_version: String,
    pub extension_node_id: Option<String>,
    pub source_path: Vec<String>,
    pub source_evidence: Value,
    pub match_kind: String,
}

pub(super) fn normalize(
    entity: &CanonicalEntity,
    dataset_type: FlowDatasetType,
) -> CanonicalClassification {
    if dataset_type != FlowDatasetType::Elementary {
        return CanonicalClassification {
            categories: Vec::new(),
            taxonomy_id: "cpc".to_owned(),
            taxonomy_version: "2.1".to_owned(),
            extension_node_id: None,
            source_path: entity.category_path.clone(),
            source_evidence: json!({"categoryPath": entity.category_path}),
            match_kind: "non-elementary".to_owned(),
        };
    }
    let source_evidence = entity
        .raw
        .get("elementaryCategorization")
        .cloned()
        .unwrap_or_else(|| json!({"categoryPath": entity.category_path}));
    let source_categories = entity
        .raw
        .get("elementaryCategorization")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.get("text").and_then(Value::as_str))
                .map(ToOwned::to_owned)
                .collect::<Vec<_>>()
        })
        .filter(|items| !items.is_empty())
        .unwrap_or_else(|| entity.category_path.clone());
    if let Some(categories) = exact_path(&source_categories) {
        let extension = extension_ids();
        let extension_node_id = categories
            .iter()
            .rev()
            .find(|category| extension.contains(&category.category_id))
            .map(|category| category.category_id.clone());
        return CanonicalClassification {
            categories,
            taxonomy_id: if extension_node_id.is_some() {
                extension_registry().taxonomy_id.clone()
            } else {
                BASE_TAXONOMY_ID.to_owned()
            },
            taxonomy_version: if extension_node_id.is_some() {
                extension_registry().taxonomy_version.to_string()
            } else {
                BASE_TAXONOMY_VERSION.to_owned()
            },
            extension_node_id,
            source_path: source_categories,
            source_evidence,
            match_kind: "exact-path".to_owned(),
        };
    }
    if let Some(categories) = deterministic_match(&source_categories) {
        let extension_node_id = categories
            .last()
            .map(|category| category.category_id.clone());
        return CanonicalClassification {
            categories,
            taxonomy_id: extension_registry().taxonomy_id.clone(),
            taxonomy_version: extension_registry().taxonomy_version.to_string(),
            extension_node_id,
            source_path: source_categories,
            source_evidence,
            match_kind: "deterministic-compartment".to_owned(),
        };
    }
    CanonicalClassification {
        categories: categories(&[
            "Emissions",
            "Emissions to air",
            "Emissions to air, unspecified",
        ]),
        taxonomy_id: BASE_TAXONOMY_ID.to_owned(),
        taxonomy_version: BASE_TAXONOMY_VERSION.to_owned(),
        extension_node_id: None,
        source_path: source_categories,
        source_evidence,
        match_kind: "fallback-air-unspecified".to_owned(),
    }
}

fn exact_path(source: &[String]) -> Option<Vec<CanonicalCategory>> {
    if source.is_empty() {
        return None;
    }
    if let Some(category_id) = extension_source_path_index().get(source) {
        return category_ancestors(category_id);
    }
    let index = category_index();
    let matched = source
        .iter()
        .map(|label| index.get(label).cloned())
        .collect::<Option<Vec<_>>>()?;
    let leaf = matched.last()?;
    category_ancestors(&leaf.category_id)
}

fn category_ancestors(category_id: &str) -> Option<Vec<CanonicalCategory>> {
    let by_id = category_id_index();
    category_id
        .split('.')
        .scan(String::new(), |prefix, segment| {
            if !prefix.is_empty() {
                prefix.push('.');
            }
            prefix.push_str(segment);
            Some(prefix.clone())
        })
        .map(|category_id| by_id.get(&category_id).cloned())
        .collect::<Option<Vec<_>>>()
}

fn deterministic_match(source: &[String]) -> Option<Vec<CanonicalCategory>> {
    let text = source.join(" ").to_ascii_lowercase();
    let leaf = if text.contains("industrial soil") {
        "Other emissions to industrial soil"
    } else if text.contains("indoor") {
        "Emissions to air, indoor"
    } else if text.contains("non-urban") && text.contains("very high stack") {
        "Emissions to non-urban air very high stack"
    } else if text.contains("non-urban") && text.contains("high stack") {
        "Emissions to non-urban air high stack"
    } else if text.contains("non-urban") && text.contains("low stack") {
        "Emissions to non-urban air low stack"
    } else if text.contains("non-urban") && text.contains("close to ground") {
        "Emissions to non-urban air close to ground"
    } else if text.contains("urban") && text.contains("very high stack") {
        "Emissions to urban air very high stack"
    } else if text.contains("urban") && text.contains("high stack") {
        "Emissions to urban air high stack"
    } else if text.contains("urban") && text.contains("low stack") {
        "Emissions to urban air low stack"
    } else {
        return None;
    };
    let path: &[&str] = if leaf == "Other emissions to industrial soil" {
        &["Emissions", "Emissions to industrial soil", leaf]
    } else {
        &["Emissions", "Emissions to air", leaf]
    };
    Some(categories(path))
}

fn categories(labels: &[&str]) -> Vec<CanonicalCategory> {
    let index = category_index();
    labels
        .iter()
        .map(|label| {
            index
                .get(*label)
                .unwrap_or_else(|| panic!("locked taxonomy is missing {label}"))
                .clone()
        })
        .collect()
}

fn category_index() -> &'static BTreeMap<String, CanonicalCategory> {
    static INDEX: OnceLock<BTreeMap<String, CanonicalCategory>> = OnceLock::new();
    INDEX.get_or_init(|| {
        let asset = tidas_assets::bundled_asset(
            "assets/tidas/schemas/tidas_flows_elementary_category.json",
        )
        .expect("elementary category schema is a locked executable asset");
        let schema: Value =
            serde_json::from_slice(asset.bytes).expect("locked elementary schema is valid JSON");
        schema["oneOf"]
            .as_array()
            .expect("elementary schema declares oneOf")
            .iter()
            .filter_map(|entry| {
                let properties = entry.get("properties")?;
                let label = properties.get("#text")?.get("const")?.as_str()?;
                Some((
                    label.to_owned(),
                    CanonicalCategory {
                        level: properties.get("@level")?.get("const")?.as_str()?.to_owned(),
                        category_id: properties.get("@catId")?.get("const")?.as_str()?.to_owned(),
                        label: label.to_owned(),
                    },
                ))
            })
            .collect()
    })
}

fn category_id_index() -> &'static BTreeMap<String, CanonicalCategory> {
    static INDEX: OnceLock<BTreeMap<String, CanonicalCategory>> = OnceLock::new();
    INDEX.get_or_init(|| {
        category_index()
            .values()
            .map(|category| (category.category_id.clone(), category.clone()))
            .collect()
    })
}

fn extension_ids() -> &'static BTreeSet<String> {
    static IDS: OnceLock<BTreeSet<String>> = OnceLock::new();
    IDS.get_or_init(|| {
        extension_registry()
            .nodes
            .iter()
            .map(|node| node.cat_id.clone())
            .collect()
    })
}

fn extension_source_path_index() -> &'static BTreeMap<Vec<String>, String> {
    static INDEX: OnceLock<BTreeMap<Vec<String>, String>> = OnceLock::new();
    INDEX.get_or_init(|| {
        extension_registry()
            .nodes
            .iter()
            .flat_map(|node| {
                node.source_paths
                    .iter()
                    .map(move |path| (path.clone(), node.cat_id.clone()))
            })
            .collect()
    })
}

#[derive(Debug, Deserialize)]
struct ExtensionRegistry {
    taxonomy_id: String,
    taxonomy_version: u64,
    base_taxonomy: BaseTaxonomy,
    nodes: Vec<ExtensionNode>,
}

#[derive(Debug, Deserialize)]
struct ExtensionNode {
    cat_id: String,
    source_paths: Vec<Vec<String>>,
}

#[derive(Debug, Deserialize)]
struct BaseTaxonomy {
    node_count: usize,
    sha256: String,
}

fn extension_registry() -> &'static ExtensionRegistry {
    static REGISTRY: OnceLock<ExtensionRegistry> = OnceLock::new();
    REGISTRY.get_or_init(|| {
        let asset = tidas_assets::bundled_asset(
            "assets/tidas/methodologies/elementary_flow_taxonomy_extension.v1.json",
        )
        .expect("elementary taxonomy extension is a locked executable asset");
        let registry: ExtensionRegistry = serde_json::from_slice(asset.bytes)
            .expect("elementary taxonomy extension is valid JSON");
        let official = tidas_assets::bundled_asset(
            "assets/eilcd/stylesheets/ILCDFlowCategorization_Reference.xml",
        )
        .expect("official ILCD taxonomy is a locked executable asset");
        let official_sha256 = hex_sha256(official.bytes);
        assert_eq!(
            official_sha256, registry.base_taxonomy.sha256,
            "official ILCD taxonomy changed without an extension base-version decision"
        );
        assert_eq!(
            category_index().len(),
            registry.base_taxonomy.node_count + registry.nodes.len(),
            "effective taxonomy is not base plus the versioned extension"
        );
        registry
    })
}

fn hex_sha256(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .fold(String::with_capacity(64), |mut output, byte| {
            write!(output, "{byte:02x}").expect("writing to a string cannot fail");
            output
        })
}

pub(crate) fn trace(classification: &CanonicalClassification, source_type: &str) -> Value {
    json!({
        "sourceFlowType": source_type,
        "taxonomyId": classification.taxonomy_id,
        "taxonomyVersion": classification.taxonomy_version,
        "extensionNodeId": classification.extension_node_id,
        "sourcePath": classification.source_path,
        "sourceClassification": classification.source_evidence,
        "mappingKind": classification.match_kind,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn specific_compartment_matching_orders_non_urban_and_very_high_first() {
        let non_urban = deterministic_match(&["non-urban air high stack".to_owned()]).unwrap();
        assert_eq!(non_urban.last().unwrap().category_id, "1.3.12");
        let very_high = deterministic_match(&["urban air very high stack".to_owned()]).unwrap();
        assert_eq!(very_high.last().unwrap().category_id, "1.3.9");
        let indoor = deterministic_match(&["indoor air".to_owned()]).unwrap();
        assert_eq!(indoor.last().unwrap().category_id, "1.3.6");
        assert!(deterministic_match(&["agricultural soil".to_owned()]).is_none());
    }

    #[test]
    fn extension_registry_has_ten_unique_nodes() {
        assert_eq!(extension_registry().nodes.len(), 10);
        assert_eq!(extension_ids().len(), 10);
        assert_eq!(extension_registry().base_taxonomy.node_count, 55);
        assert_eq!(category_index().len(), 65);
    }

    #[test]
    fn nine_declared_source_paths_resolve_exactly_to_the_versioned_extension() {
        let registry = extension_registry();
        let source_paths = registry
            .nodes
            .iter()
            .flat_map(|node| {
                node.source_paths
                    .iter()
                    .map(move |path| (node.cat_id.as_str(), path))
            })
            .collect::<Vec<_>>();
        assert_eq!(source_paths.len(), 9);
        for (expected_id, path) in source_paths {
            let categories = exact_path(path).unwrap();
            assert_eq!(categories.first().unwrap().category_id, "1");
            assert_eq!(categories.last().unwrap().category_id, expected_id);
            assert!(extension_ids().contains(expected_id));
        }
    }

    #[test]
    fn official_ilcd_taxonomy_bytes_remain_immutable() {
        let asset = tidas_assets::bundled_asset(
            "assets/eilcd/stylesheets/ILCDFlowCategorization_Reference.xml",
        )
        .unwrap();
        let digest = hex_sha256(asset.bytes);
        assert_eq!(digest, extension_registry().base_taxonomy.sha256);
        let official = std::str::from_utf8(asset.bytes).unwrap();
        for extension_id in extension_ids() {
            assert!(
                !official.contains(&format!("id=\"{extension_id}\"")),
                "official-only taxonomy unexpectedly contains {extension_id}"
            );
        }
    }
}
