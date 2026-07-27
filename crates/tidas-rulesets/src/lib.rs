//! Integrity-locked runtime methodology/ruleset catalog.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use thiserror::Error;
use tidas_assets::{AssetKind, bundled_asset};

const RUNTIME_RULESETS_PATH: &str = "assets/tidas/methodologies/runtime_rulesets.json";
const RUNTIME_RULESETS_SCHEMA_PATH: &str =
    "assets/tidas/methodologies/runtime_rulesets.schema.json";
pub const RULESET_DESCRIPTION_SCHEMA_V1: &str = "tidas.ruleset-description.v1";
pub const METHODOLOGY_VALIDATION_REPORT_SCHEMA_V1: &str = "tidas.methodology-validation-report.v1";
pub const RULESET_DESCRIPTION_JSON_SCHEMA_V1: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/contracts/ruleset-description.v1.schema.json"
));
pub const METHODOLOGY_VALIDATION_REPORT_JSON_SCHEMA_V1: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/contracts/methodology-validation-report.v1.schema.json"
));

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RulesetDescriptionV1 {
    pub schema_version: String,
    pub ruleset_version: String,
    pub catalog_sha256: String,
    pub ruleset_count: u64,
    pub rule_count: u64,
    pub ruleset_ids: Vec<String>,
    pub methodology_file_count: u64,
    pub methodology_warning_count: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct MethodologyValidationReportV1 {
    pub schema_version: String,
    pub ok: bool,
    pub file_count: u64,
    pub error_count: u64,
    pub warning_count: u64,
    pub files: Vec<MethodologyFileReportV1>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct MethodologyFileReportV1 {
    pub methodology_file: String,
    pub schema_file: String,
    pub status: String,
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
}

#[derive(Debug)]
pub struct RulesetCatalog {
    metadata: Value,
    rules_by_id: BTreeMap<String, Value>,
    profile_rule_ids: BTreeMap<String, Vec<String>>,
    description: RulesetDescriptionV1,
    methodology_report: MethodologyValidationReportV1,
}

impl RulesetCatalog {
    pub fn load() -> Result<Self, RulesetError> {
        let metadata_asset = required_asset(RUNTIME_RULESETS_PATH)?;
        let schema_asset = required_asset(RUNTIME_RULESETS_SCHEMA_PATH)?;
        let metadata: Value = serde_json::from_slice(metadata_asset.bytes)?;
        let schema: Value = serde_json::from_slice(schema_asset.bytes)?;
        let validator = jsonschema::draft202012::new(&schema)
            .map_err(|error| RulesetError::SchemaCompile(error.to_string()))?;
        if let Some(error) = validator.iter_errors(&metadata).next() {
            return Err(RulesetError::SchemaValidation(error.to_string()));
        }

        let rules = metadata
            .get("rules")
            .and_then(Value::as_array)
            .ok_or(RulesetError::MissingRules)?;
        let mut rules_by_id = BTreeMap::new();
        for rule in rules {
            let id = required_id(rule, "rule")?;
            if rules_by_id.insert(id.clone(), rule.clone()).is_some() {
                return Err(RulesetError::DuplicateRule(id));
            }
        }
        let profiles = metadata
            .get("rulesets")
            .and_then(Value::as_array)
            .ok_or(RulesetError::MissingRulesets)?;
        let mut profile_rule_ids = BTreeMap::new();
        for profile in profiles {
            let id = required_id(profile, "ruleset")?;
            let rule_ids = profile
                .get("rule_ids")
                .and_then(Value::as_array)
                .ok_or_else(|| RulesetError::MissingRuleIds(id.clone()))?
                .iter()
                .map(|value| {
                    value
                        .as_str()
                        .map(ToOwned::to_owned)
                        .ok_or_else(|| RulesetError::InvalidRuleId(id.clone()))
                })
                .collect::<Result<Vec<_>, _>>()?;
            for rule_id in &rule_ids {
                if !rules_by_id.contains_key(rule_id) {
                    return Err(RulesetError::UnknownRule {
                        ruleset: id.clone(),
                        rule: rule_id.clone(),
                    });
                }
            }
            if profile_rule_ids.insert(id.clone(), rule_ids).is_some() {
                return Err(RulesetError::DuplicateRuleset(id));
            }
        }
        let ruleset_version = metadata
            .get("ruleset_version")
            .and_then(Value::as_str)
            .ok_or(RulesetError::MissingVersion)?
            .to_owned();
        let canonical = serde_json::to_vec(&metadata)?;
        let methodology_report = validate_methodologies()?;
        let description = RulesetDescriptionV1 {
            schema_version: RULESET_DESCRIPTION_SCHEMA_V1.to_owned(),
            ruleset_version,
            catalog_sha256: digest_hex(&Sha256::digest(canonical)),
            ruleset_count: u64::try_from(profile_rule_ids.len())
                .map_err(|_| RulesetError::SizeOverflow)?,
            rule_count: u64::try_from(rules_by_id.len()).map_err(|_| RulesetError::SizeOverflow)?,
            ruleset_ids: profile_rule_ids.keys().cloned().collect(),
            methodology_file_count: methodology_report.file_count,
            methodology_warning_count: methodology_report.warning_count,
        };
        Ok(Self {
            metadata,
            rules_by_id,
            profile_rule_ids,
            description,
            methodology_report,
        })
    }

    #[must_use]
    pub fn metadata(&self) -> &Value {
        &self.metadata
    }

    #[must_use]
    pub const fn description(&self) -> &RulesetDescriptionV1 {
        &self.description
    }

    #[must_use]
    pub const fn methodology_report(&self) -> &MethodologyValidationReportV1 {
        &self.methodology_report
    }

    pub fn rules_for(&self, ruleset_id: &str) -> Result<Vec<&Value>, RulesetError> {
        let ids = self
            .profile_rule_ids
            .get(ruleset_id)
            .ok_or_else(|| RulesetError::UnknownRuleset(ruleset_id.to_owned()))?;
        Ok(ids
            .iter()
            .map(|id| {
                self.rules_by_id
                    .get(id)
                    .expect("referential integrity was checked while loading")
            })
            .collect())
    }
}

fn validate_methodologies() -> Result<MethodologyValidationReportV1, RulesetError> {
    let mut files = Vec::new();
    let mut total_errors = 0_u64;
    let mut total_warnings = 0_u64;
    for asset in tidas_assets::bundled_assets().into_iter().filter(|asset| {
        asset.kind == AssetKind::Methodology
            && std::path::Path::new(&asset.path)
                .extension()
                .is_some_and(|extension| extension.eq_ignore_ascii_case("yaml"))
    }) {
        let report = validate_methodology_asset(&asset)?;
        total_errors = total_errors
            .checked_add(
                u64::try_from(report.errors.len()).map_err(|_| RulesetError::SizeOverflow)?,
            )
            .ok_or(RulesetError::SizeOverflow)?;
        total_warnings = total_warnings
            .checked_add(
                u64::try_from(report.warnings.len()).map_err(|_| RulesetError::SizeOverflow)?,
            )
            .ok_or(RulesetError::SizeOverflow)?;
        files.push(report);
    }
    files.sort_by(|left, right| left.methodology_file.cmp(&right.methodology_file));
    let file_count = u64::try_from(files.len()).map_err(|_| RulesetError::SizeOverflow)?;
    Ok(MethodologyValidationReportV1 {
        schema_version: METHODOLOGY_VALIDATION_REPORT_SCHEMA_V1.to_owned(),
        ok: total_errors == 0,
        file_count,
        error_count: total_errors,
        warning_count: total_warnings,
        files,
    })
}

fn validate_methodology_asset(
    asset: &tidas_assets::BundledAsset,
) -> Result<MethodologyFileReportV1, RulesetError> {
    let methodology_file = asset
        .path
        .rsplit('/')
        .next()
        .expect("embedded asset paths are non-empty")
        .to_owned();
    let stem = std::path::Path::new(&methodology_file)
        .file_stem()
        .and_then(|value| value.to_str())
        .expect("embedded methodology filenames are controlled UTF-8");
    let schema_file = format!("{stem}.json");
    let schema_path = format!("assets/tidas/schemas/{schema_file}");
    let Some(schema_asset) = bundled_asset(&schema_path) else {
        return Ok(MethodologyFileReportV1 {
            methodology_file,
            schema_file,
            status: "error".to_owned(),
            errors: vec!["No corresponding schema file found".to_owned()],
            warnings: Vec::new(),
        });
    };
    let yaml_text =
        std::str::from_utf8(asset.bytes).map_err(|error| RulesetError::MethodologyParse {
            path: asset.path.clone(),
            reason: error.to_string(),
        })?;
    let yaml: Value = noyalib::compat::serde_yaml::from_str(yaml_text).map_err(|error| {
        RulesetError::MethodologyParse {
            path: asset.path.clone(),
            reason: error.to_string(),
        }
    })?;
    let schema: Value = serde_json::from_slice(schema_asset.bytes)?;
    let normalized_yaml: BTreeMap<String, String> = extract_methodology_paths(&yaml)
        .into_iter()
        .map(|path| (normalize_path(&path), path))
        .collect();
    let normalized_schema: BTreeMap<String, String> = extract_schema_paths(&schema)
        .into_iter()
        .map(|path| (normalize_path(&path), path))
        .collect();
    let mut warnings = methodology_only_warnings(&normalized_yaml, &normalized_schema);
    warnings.extend(schema_only_warnings(&normalized_yaml, &normalized_schema));
    warnings.sort();
    Ok(MethodologyFileReportV1 {
        methodology_file,
        schema_file,
        status: if warnings.is_empty() {
            "ok".to_owned()
        } else {
            "warning".to_owned()
        },
        errors: Vec::new(),
        warnings,
    })
}

fn methodology_only_warnings(
    methodology: &BTreeMap<String, String>,
    schema: &BTreeMap<String, String>,
) -> Vec<String> {
    methodology
        .iter()
        .filter(|(key, _)| !schema.contains_key(*key))
        .map(|(_, path)| format!("Field '{path}' in YAML methodology not found in schema"))
        .collect()
}

fn schema_only_warnings(
    methodology: &BTreeMap<String, String>,
    schema: &BTreeMap<String, String>,
) -> Vec<String> {
    const IMPORTANT: [&str; 5] = [
        "processDataSet",
        "processInformation",
        "modellingAndValidation",
        "administrativeInformation",
        "exchanges",
    ];
    schema
        .iter()
        .filter(|(key, _)| !methodology.contains_key(*key))
        .filter(|(_, path)| {
            IMPORTANT.iter().any(|important| {
                path.contains(important) && path.split('.').next() == Some(important)
            })
        })
        .map(|(_, path)| format!("Schema field '{path}' not covered in YAML methodology"))
        .collect()
}

fn extract_methodology_paths(value: &Value) -> BTreeSet<String> {
    fn visit(value: &Value, current: &str, output: &mut BTreeSet<String>) {
        let Some(object) = value.as_object() else {
            return;
        };
        for (key, child) in object {
            if matches!(key.as_str(), "<rules>" | "metadata" | "global_rules") {
                continue;
            }
            let path = if current.is_empty() {
                key.clone()
            } else {
                format!("{current}.{key}")
            };
            output.insert(path.clone());
            visit(child, &path, output);
        }
    }
    let mut paths = BTreeSet::new();
    visit(value, "", &mut paths);
    paths
}

fn extract_schema_paths(value: &Value) -> BTreeSet<String> {
    fn visit(value: &Value, current: &str, output: &mut BTreeSet<String>) {
        let Some(object) = value.as_object() else {
            return;
        };
        if object.get("type").and_then(Value::as_str) == Some("array") {
            match object.get("items") {
                Some(Value::Object(item)) => visit(&Value::Object(item.clone()), current, output),
                Some(Value::Array(items)) => {
                    for item in items {
                        visit(item, current, output);
                    }
                }
                _ => {}
            }
        }
        if let Some(properties) = object.get("properties").and_then(Value::as_object) {
            for (name, schema) in properties {
                let clean = name.replace("common:", "").replace('@', "");
                let path = if current.is_empty() {
                    clean
                } else {
                    format!("{current}.{clean}")
                };
                output.insert(path.clone());
                visit(schema, &path, output);
            }
        }
    }
    let mut paths = BTreeSet::new();
    visit(value, "", &mut paths);
    paths
}

fn normalize_path(path: &str) -> String {
    path.replace("common:", "")
        .replace('@', "")
        .replace("UUID", "uuid")
        .replace("timeStamp", "timestamp")
        .replace("dataSetVersion", "datasetversion")
        .to_ascii_lowercase()
}

fn required_asset(path: &str) -> Result<tidas_assets::BundledAsset, RulesetError> {
    let asset = bundled_asset(path).ok_or_else(|| RulesetError::MissingAsset(path.to_owned()))?;
    if asset.kind != AssetKind::RuntimeRuleset {
        return Err(RulesetError::UnexpectedAssetKind(path.to_owned()));
    }
    Ok(asset)
}

fn required_id(value: &Value, kind: &'static str) -> Result<String, RulesetError> {
    value
        .get("id")
        .and_then(Value::as_str)
        .filter(|id| !id.is_empty())
        .map(ToOwned::to_owned)
        .ok_or(RulesetError::MissingId(kind))
}

fn digest_hex(digest: &[u8]) -> String {
    let mut output = String::with_capacity(digest.len() * 2);
    for byte in digest {
        write!(&mut output, "{byte:02x}").expect("writing to a String cannot fail");
    }
    output
}

#[derive(Debug, Error)]
pub enum RulesetError {
    #[error("required ruleset asset is missing: {0}")]
    MissingAsset(String),
    #[error("ruleset asset has an unexpected kind: {0}")]
    UnexpectedAssetKind(String),
    #[error("runtime ruleset schema failed to compile: {0}")]
    SchemaCompile(String),
    #[error("runtime ruleset metadata failed schema validation: {0}")]
    SchemaValidation(String),
    #[error("runtime ruleset metadata has no rules array")]
    MissingRules,
    #[error("runtime ruleset metadata has no rulesets array")]
    MissingRulesets,
    #[error("runtime ruleset metadata has no ruleset_version")]
    MissingVersion,
    #[error("{0} entry has no non-empty id")]
    MissingId(&'static str),
    #[error("duplicate rule id: {0}")]
    DuplicateRule(String),
    #[error("duplicate ruleset id: {0}")]
    DuplicateRuleset(String),
    #[error("ruleset {0} has no rule_ids array")]
    MissingRuleIds(String),
    #[error("ruleset {0} contains a non-string rule id")]
    InvalidRuleId(String),
    #[error("ruleset {ruleset} references unknown rule {rule}")]
    UnknownRule { ruleset: String, rule: String },
    #[error("unknown ruleset id: {0}")]
    UnknownRuleset(String),
    #[error("ruleset catalog size cannot be represented safely")]
    SizeOverflow,
    #[error("methodology asset {path} is invalid: {reason}")]
    MethodologyParse { path: String, reason: String },
    #[error("ruleset JSON is invalid: {0}")]
    Json(#[from] serde_json::Error),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packaged_rulesets_are_schema_valid_and_referentially_complete() {
        let catalog = RulesetCatalog::load().unwrap();
        assert_eq!(catalog.description.ruleset_count, 7);
        assert!(catalog.description.rule_count > 10);
        assert!(
            catalog
                .rules_for("process-authoring/strict")
                .unwrap()
                .iter()
                .any(|rule| rule["id"] == "tidas.process.quantitative-reference.required")
        );
        assert!(matches!(
            catalog.rules_for("missing/default"),
            Err(RulesetError::UnknownRuleset(_))
        ));
        assert_eq!(catalog.methodology_report.file_count, 2);
        assert!(catalog.methodology_report.ok);
    }

    #[test]
    fn warning_and_blocker_severities_survive_the_rust_catalog() {
        let catalog = RulesetCatalog::load().unwrap();
        let severities: BTreeSet<&str> = catalog
            .rules_for("process-authoring/strict")
            .unwrap()
            .into_iter()
            .filter_map(|rule| rule["severity"].as_str())
            .collect();
        assert!(severities.contains("warning"));
        assert!(severities.contains("blocker"));
    }

    #[test]
    fn public_ruleset_reports_match_their_checked_in_contracts() {
        let catalog = RulesetCatalog::load().unwrap();
        for (schema, instance) in [
            (
                RULESET_DESCRIPTION_JSON_SCHEMA_V1,
                serde_json::to_value(catalog.description()).unwrap(),
            ),
            (
                METHODOLOGY_VALIDATION_REPORT_JSON_SCHEMA_V1,
                serde_json::to_value(catalog.methodology_report()).unwrap(),
            ),
        ] {
            let schema: Value = serde_json::from_str(schema).unwrap();
            let validator = jsonschema::draft202012::new(&schema).unwrap();
            let errors: Vec<_> = validator
                .iter_errors(&instance)
                .map(|error| error.to_string())
                .collect();
            assert!(errors.is_empty(), "{errors:?}");
        }
    }
}
