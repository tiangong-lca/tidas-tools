use std::collections::BTreeMap;

use jsonschema::{Resource, Validator};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;
use tidas_assets::{AssetKind, bundled_assets};

use crate::contracts::ValidationIssueV1;

const SCHEMA_ASSET_PREFIX: &str = "assets/tidas/schemas/";
const SCHEMA_BASE_URI: &str = "https://tiangong.earth/assets/tidas/schemas/";

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum TidasCategory {
    Contacts,
    Flowproperties,
    Flows,
    Lciamethods,
    Lifecyclemodels,
    Processes,
    Sources,
    Unitgroups,
}

pub const SUPPORTED_TIDAS_CATEGORIES: [TidasCategory; 8] = [
    TidasCategory::Contacts,
    TidasCategory::Flowproperties,
    TidasCategory::Flows,
    TidasCategory::Lciamethods,
    TidasCategory::Lifecyclemodels,
    TidasCategory::Processes,
    TidasCategory::Sources,
    TidasCategory::Unitgroups,
];

impl TidasCategory {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Contacts => "contacts",
            Self::Flowproperties => "flowproperties",
            Self::Flows => "flows",
            Self::Lciamethods => "lciamethods",
            Self::Lifecyclemodels => "lifecyclemodels",
            Self::Processes => "processes",
            Self::Sources => "sources",
            Self::Unitgroups => "unitgroups",
        }
    }

    fn schema_filename(self) -> String {
        format!("tidas_{}.json", self.as_str())
    }

    pub(crate) fn parse(value: &str) -> Option<Self> {
        SUPPORTED_TIDAS_CATEGORIES
            .into_iter()
            .find(|category| category.as_str() == value)
    }
}

pub(crate) struct SchemaCatalog {
    schemas: BTreeMap<String, Value>,
}

impl SchemaCatalog {
    pub(crate) fn load() -> Result<Self, SchemaError> {
        let mut schemas = BTreeMap::new();
        for asset in bundled_assets()
            .into_iter()
            .filter(|asset| asset.kind == AssetKind::JsonSchema)
        {
            let Some(filename) = asset.path.strip_prefix(SCHEMA_ASSET_PREFIX) else {
                continue;
            };
            let asset_path = asset.path.clone();
            let mut schema: Value = serde_json::from_slice(asset.bytes).map_err(|source| {
                SchemaError::InvalidAsset {
                    path: asset_path,
                    source,
                }
            })?;
            let object = schema
                .as_object_mut()
                .ok_or_else(|| SchemaError::AssetNotObject(filename.to_owned()))?;
            object.insert(
                "$id".to_owned(),
                Value::String(format!("{SCHEMA_BASE_URI}{filename}")),
            );
            schemas.insert(filename.to_owned(), schema);
        }
        if schemas.is_empty() {
            return Err(SchemaError::NoSchemas);
        }
        Ok(Self { schemas })
    }

    pub(crate) fn validator(&self, category: TidasCategory) -> Result<TidasValidator, SchemaError> {
        let filename = category.schema_filename();
        let root = self
            .schemas
            .get(&filename)
            .ok_or_else(|| SchemaError::MissingCategory(filename.clone()))?;
        let resources = self.schemas.iter().map(|(resource_name, schema)| {
            (
                format!("{SCHEMA_BASE_URI}{resource_name}"),
                Resource::from_contents(schema.clone()),
            )
        });
        let validator = jsonschema::draft7::options()
            .with_resources(resources)
            .with_format("cas-number", is_valid_cas_number)
            .should_validate_formats(true)
            .build(root)
            .map_err(|source| SchemaError::Compile {
                filename,
                reason: source.to_string(),
            })?;
        Ok(TidasValidator {
            category,
            validator,
        })
    }
}

pub(crate) struct TidasValidator {
    category: TidasCategory,
    validator: Validator,
}

impl TidasValidator {
    pub(crate) fn issues<'a>(
        &'a self,
        instance: &'a Value,
        file_path: &'a str,
    ) -> impl Iterator<Item = ValidationIssueV1> + 'a {
        self.validator.iter_errors(instance).map(move |error| {
            let raw_location = error.instance_path().to_string();
            let location = raw_location
                .strip_prefix('/')
                .unwrap_or(&raw_location)
                .to_owned();
            let location = if location.is_empty() {
                "<root>".to_owned()
            } else {
                location
            };
            ValidationIssueV1::error(
                "schema_error",
                self.category.as_str(),
                file_path,
                &location,
                format!("Schema Error at {location}: {error}"),
            )
        })
    }
}

#[must_use]
pub fn is_valid_cas_number(value: &str) -> bool {
    let mut parts = value.rsplitn(2, '-');
    let Some(check_digit) = parts.next().and_then(|part| part.parse::<u8>().ok()) else {
        return false;
    };
    let Some(body) = parts.next() else {
        return false;
    };
    let Some((first, second)) = body.split_once('-') else {
        return false;
    };
    if !(2..=7).contains(&first.len())
        || second.len() != 2
        || !first.bytes().all(|byte| byte.is_ascii_digit())
        || !second.bytes().all(|byte| byte.is_ascii_digit())
    {
        return false;
    }
    let checksum = first
        .bytes()
        .chain(second.bytes())
        .rev()
        .enumerate()
        .map(|(index, byte)| u64::from(byte - b'0') * (index as u64 + 1))
        .sum::<u64>();
    checksum % 10 == u64::from(check_digit)
}

#[derive(Debug, Error)]
pub enum SchemaError {
    #[error("no bundled TIDAS JSON schemas were found")]
    NoSchemas,
    #[error("bundled schema {0} is not a JSON object")]
    AssetNotObject(String),
    #[error("missing bundled category schema {0}")]
    MissingCategory(String),
    #[error("bundled schema {path} is invalid JSON: {source}")]
    InvalidAsset {
        path: String,
        source: serde_json::Error,
    },
    #[error("failed to compile bundled schema {filename}: {reason}")]
    Compile { filename: String, reason: String },
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn cas_number_contract_matches_the_python_oracle() {
        assert!(is_valid_cas_number("64-17-5"));
        assert!(is_valid_cas_number("7732-18-5"));
        assert!(!is_valid_cas_number("64-17-6"));
        assert!(!is_valid_cas_number("2023600"));
        assert!(!is_valid_cas_number(" 64-17-5 "));
    }

    #[test]
    fn every_supported_category_compiles_offline() {
        let catalog = SchemaCatalog::load().unwrap();
        for category in SUPPORTED_TIDAS_CATEGORIES {
            catalog.validator(category).unwrap();
        }
    }

    #[test]
    fn english_and_chinese_flow_schemas_share_the_type_aware_name_contract() {
        let fixtures: Value = serde_json::from_str(include_str!(
            "../tests/fixtures/flow-name-schema-v1/cases.json"
        ))
        .unwrap();
        let template: Value = serde_json::from_str(include_str!(
            "../tests/fixtures/flow-name-schema-v1/template.json"
        ))
        .unwrap();
        assert_eq!(
            fixtures["schema_version"],
            "tidas.flow-name-schema-fixtures.v1"
        );
        for (language, catalog) in [
            ("en", SchemaCatalog::load().unwrap()),
            ("zh", schema_catalog("assets/tidas/schemas_zh/")),
        ] {
            let validator = catalog.validator(TidasCategory::Flows).unwrap();
            for case in fixtures["cases"].as_array().unwrap() {
                let mut document = template.clone();
                document["flowDataSet"]["flowInformation"]["dataSetInformation"]["name"] =
                    case["flow_name"].clone();
                document["flowDataSet"]["modellingAndValidation"]["LCIMethod"]["typeOfDataSet"] =
                    case["flow_type"].clone();
                let issues = validator
                    .issues(&document, "flows/fixture.json")
                    .collect::<Vec<_>>();
                assert_eq!(
                    issues.is_empty(),
                    case["valid"].as_bool().unwrap(),
                    "{language} case {}: {}",
                    case["name"].as_str().unwrap(),
                    json!(issues),
                );
            }
        }
    }

    fn schema_catalog(prefix: &str) -> SchemaCatalog {
        let schemas = bundled_assets()
            .into_iter()
            .filter_map(|asset| {
                let filename = asset.path.strip_prefix(prefix)?;
                let mut schema: Value = serde_json::from_slice(asset.bytes).unwrap();
                schema.as_object_mut().unwrap().insert(
                    "$id".to_owned(),
                    Value::String(format!("{SCHEMA_BASE_URI}{filename}")),
                );
                Some((filename.to_owned(), schema))
            })
            .collect::<BTreeMap<_, _>>();
        assert!(!schemas.is_empty());
        SchemaCatalog { schemas }
    }
}
