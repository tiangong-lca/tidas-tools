use std::collections::BTreeMap;
use std::str::FromStr;

use bigdecimal::BigDecimal;
use serde_json::{Map, Value, json};

use super::{AdapterContext, AdapterError};
use crate::report::{ImportIssue, IssueSeverity, IssueSink};
use crate::store::CanonicalStore;

#[derive(Clone)]
struct UnitRecord {
    factor: BigDecimal,
    group_id: String,
    name: Option<String>,
}

#[derive(Clone)]
struct GroupReference {
    unit_id: Option<String>,
    unit_name: String,
}

#[derive(Clone)]
struct FlowRecord {
    factors: BTreeMap<String, BigDecimal>,
    reference_property_id: String,
    reference_property_name: Option<String>,
}

#[derive(Default)]
struct Indexes {
    units: BTreeMap<String, UnitRecord>,
    group_references: BTreeMap<String, GroupReference>,
    property_groups: BTreeMap<String, String>,
    group_properties: BTreeMap<String, Vec<String>>,
    flows: BTreeMap<String, FlowRecord>,
}

#[derive(Default)]
struct Stats {
    scanned: u64,
    normalized_same_property: u64,
    normalized_cross_property: u64,
    already_reference: u64,
    no_unit_info: u64,
    unresolved: BTreeMap<String, u64>,
    unresolved_samples: Vec<Value>,
}

pub(super) fn normalize_exchange_amounts(
    context: &AdapterContext<'_>,
    store: &CanonicalStore,
    issues: &mut dyn IssueSink,
) -> Result<(), AdapterError> {
    let indexes = build_indexes(store)?;
    let processes = store
        .iter_type("processes")?
        .collect::<Result<Vec<_>, _>>()?;
    let mut stats = Stats::default();
    for process in processes {
        let process_id = process.internal_id.clone();
        store.rewrite_process_exchanges::<AdapterError>(&process_id, |exchange| {
            context.cancellation.check()?;
            stats.scanned = stats.scanned.saturating_add(1);
            match normalize_exchange(exchange, &indexes) {
                Outcome::NoUnitInfo => {
                    stats.no_unit_info = stats.no_unit_info.saturating_add(1);
                }
                Outcome::AlreadyReference => {
                    stats.already_reference = stats.already_reference.saturating_add(1);
                }
                Outcome::Normalized {
                    cross_property: true,
                } => {
                    stats.normalized_cross_property =
                        stats.normalized_cross_property.saturating_add(1);
                }
                Outcome::Normalized {
                    cross_property: false,
                } => {
                    stats.normalized_same_property =
                        stats.normalized_same_property.saturating_add(1);
                }
                Outcome::Unresolved(reason) => {
                    *stats.unresolved.entry(reason.to_owned()).or_default() += 1;
                    if stats.unresolved_samples.len() < 20 {
                        stats.unresolved_samples.push(json!({
                            "process_id": process_id,
                            "internalId": exchange.get("internalId").cloned().unwrap_or(Value::Null),
                            "unitId": exchange.get("unitId").cloned().unwrap_or(Value::Null),
                            "unitName": exchange.get("unitName").cloned().unwrap_or(Value::Null),
                            "reason": reason,
                        }));
                    }
                }
            }
            Ok(())
        })?;
    }
    emit_issues(&stats, issues)?;
    Ok(())
}

fn emit_issues(stats: &Stats, issues: &mut dyn IssueSink) -> Result<(), AdapterError> {
    let normalized = stats
        .normalized_same_property
        .saturating_add(stats.normalized_cross_property);
    let unresolved_total = stats.unresolved.values().copied().sum::<u64>();
    if stats.scanned == 0 || (normalized == 0 && unresolved_total == 0) {
        return Ok(());
    }
    let context = BTreeMap::from([
        ("scanned".to_owned(), json!(stats.scanned)),
        (
            "normalized_same_property".to_owned(),
            json!(stats.normalized_same_property),
        ),
        (
            "normalized_cross_property".to_owned(),
            json!(stats.normalized_cross_property),
        ),
        (
            "already_reference".to_owned(),
            json!(stats.already_reference),
        ),
        ("no_unit_info".to_owned(), json!(stats.no_unit_info)),
        ("unresolved".to_owned(), json!(stats.unresolved)),
        (
            "unresolved_samples".to_owned(),
            Value::Array(stats.unresolved_samples.clone()),
        ),
    ]);
    issues.push(&ImportIssue {
        severity: IssueSeverity::Warning,
        code: "exchange_amounts_normalized_to_reference_units".to_owned(),
        message: "Normalized exchange amounts to each flow's reference flow-property unit."
            .to_owned(),
        source_object: None,
        context,
    })?;
    if unresolved_total > 0 {
        issues.push(&ImportIssue {
            severity: IssueSeverity::Warning,
            code: "exchange_unit_normalization_unresolved".to_owned(),
            message: "Some exchange amounts could not be normalized and were left unchanged."
                .to_owned(),
            source_object: None,
            context: BTreeMap::from([
                ("scanned".to_owned(), json!(stats.scanned)),
                ("unresolved_total".to_owned(), json!(unresolved_total)),
                ("unresolved".to_owned(), json!(stats.unresolved)),
                (
                    "unresolved_samples".to_owned(),
                    Value::Array(stats.unresolved_samples.clone()),
                ),
            ]),
        })?;
    }
    Ok(())
}

enum Outcome {
    NoUnitInfo,
    AlreadyReference,
    Normalized { cross_property: bool },
    Unresolved(&'static str),
}

fn normalize_exchange(exchange: &mut Map<String, Value>, indexes: &Indexes) -> Outcome {
    let Some(unit_id) = string(exchange.get("unitId")) else {
        return Outcome::NoUnitInfo;
    };
    let Some(unit) = indexes.units.get(unit_id) else {
        return Outcome::Unresolved("unknown_unit");
    };
    let Some((property_id, property_group)) = exchange_property(exchange, unit, indexes) else {
        return Outcome::Unresolved("unknown_flow_property");
    };
    if property_group != unit.group_id {
        return Outcome::Unresolved("unit_not_in_property_group");
    }
    let Some(flow_id) = string(exchange.get("flowRefId")) else {
        return Outcome::Unresolved("missing_flow_factors");
    };
    let Some(flow) = indexes.flows.get(flow_id) else {
        return Outcome::Unresolved("missing_flow_factors");
    };
    let Some(property_factor) = flow.factors.get(property_id) else {
        return Outcome::Unresolved("missing_flow_factors");
    };
    let Some(reference_factor) = flow.factors.get(&flow.reference_property_id) else {
        return Outcome::Unresolved("missing_flow_factors");
    };
    if property_factor == &BigDecimal::from(0) || reference_factor == &BigDecimal::from(0) {
        return Outcome::Unresolved("zero_factor");
    }
    let cross_property = property_id != flow.reference_property_id;
    let target_group_id = if cross_property {
        let Some(group) = indexes.property_groups.get(&flow.reference_property_id) else {
            return Outcome::Unresolved("unknown_flow_property");
        };
        group
    } else {
        &unit.group_id
    };
    let Some(target) = indexes.group_references.get(target_group_id) else {
        return Outcome::Unresolved("missing_reference_unit");
    };
    let factor = (&unit.factor * reference_factor) / property_factor;
    if factor == 1 {
        return Outcome::AlreadyReference;
    }
    let Some(amount) = decimal(exchange.get("amount")) else {
        return Outcome::Unresolved("non_numeric_amount");
    };

    preserve(exchange, "amount", "sourceAmount");
    preserve(exchange, "unitId", "sourceUnitId");
    preserve(exchange, "unitName", "sourceUnitName");
    preserve(exchange, "flowPropertyRefId", "sourceFlowPropertyRefId");
    preserve(exchange, "flowPropertyName", "sourceFlowPropertyName");
    exchange.insert(
        "amount".to_owned(),
        Value::String(decimal_text(&(amount * &factor))),
    );
    match &target.unit_id {
        Some(id) => {
            exchange.insert("unitId".to_owned(), Value::String(id.clone()));
        }
        None => {
            exchange.remove("unitId");
        }
    }
    exchange.insert(
        "unitName".to_owned(),
        Value::String(target.unit_name.clone()),
    );
    if cross_property {
        exchange.insert(
            "flowPropertyRefId".to_owned(),
            Value::String(flow.reference_property_id.clone()),
        );
        match &flow.reference_property_name {
            Some(name) => {
                exchange.insert("flowPropertyName".to_owned(), Value::String(name.clone()));
            }
            None => {
                exchange.remove("flowPropertyName");
            }
        }
    }
    for key in ["minimumAmount", "maximumAmount"] {
        if let Some(bound) = decimal(exchange.get(key)) {
            exchange.insert(
                key.to_owned(),
                Value::String(decimal_text(&(bound * &factor))),
            );
        }
    }
    exchange.insert(
        "amountNormalization".to_owned(),
        json!({
            "factor": decimal_text(&factor),
            "sourceUnit": exchange.get("sourceUnitName").cloned().or_else(|| unit.name.clone().map(Value::String)).unwrap_or(Value::Null),
            "targetUnit": target.unit_name,
            "crossProperty": cross_property,
            "amountFormulaNotRescaled": exchange.contains_key("amountFormula"),
        }),
    );
    Outcome::Normalized { cross_property }
}

fn exchange_property<'a>(
    exchange: &'a Map<String, Value>,
    unit: &'a UnitRecord,
    indexes: &'a Indexes,
) -> Option<(&'a str, &'a str)> {
    if let Some(property_id) = string(exchange.get("flowPropertyRefId")) {
        let group = indexes.property_groups.get(property_id)?;
        return Some((property_id, group));
    }
    let candidates = indexes.group_properties.get(&unit.group_id)?;
    let flow = string(exchange.get("flowRefId")).and_then(|id| indexes.flows.get(id));
    let mut matching = candidates
        .iter()
        .filter(|candidate| flow.is_none_or(|flow| flow.factors.contains_key(*candidate)));
    let property = matching.next()?;
    matching
        .next()
        .is_none()
        .then_some((property.as_str(), unit.group_id.as_str()))
}

fn build_indexes(store: &CanonicalStore) -> Result<Indexes, AdapterError> {
    let mut indexes = Indexes::default();
    add_unit_indexes(store, &mut indexes)?;
    add_property_indexes(store, &mut indexes)?;
    add_flow_indexes(store, &mut indexes)?;
    Ok(indexes)
}

fn add_unit_indexes(store: &CanonicalStore, indexes: &mut Indexes) -> Result<(), AdapterError> {
    for group in store.iter_type("unitgroups")? {
        let group = group?;
        let Some(units) = group.raw.get("units").and_then(Value::as_array) else {
            continue;
        };
        let reference = units
            .iter()
            .filter_map(Value::as_object)
            .find(|unit| {
                unit.get("referenceUnit")
                    .and_then(Value::as_bool)
                    .unwrap_or(false)
            })
            .or_else(|| {
                units
                    .iter()
                    .filter_map(Value::as_object)
                    .find(|unit| decimal(unit.get("conversionFactor")) == Some(BigDecimal::from(1)))
            });
        if let Some(reference) = reference
            && let Some(name) = object_name(reference)
        {
            indexes.group_references.insert(
                group.internal_id.clone(),
                GroupReference {
                    unit_id: object_id(reference).map(ToOwned::to_owned),
                    unit_name: name.to_owned(),
                },
            );
        }
        for unit in units.iter().filter_map(Value::as_object) {
            let Some(id) = object_id(unit) else {
                continue;
            };
            let Some(factor) = decimal(unit.get("conversionFactor")) else {
                continue;
            };
            indexes.units.insert(
                id.to_owned(),
                UnitRecord {
                    factor,
                    group_id: group.internal_id.clone(),
                    name: object_name(unit).map(ToOwned::to_owned),
                },
            );
        }
    }
    Ok(())
}

fn add_property_indexes(store: &CanonicalStore, indexes: &mut Indexes) -> Result<(), AdapterError> {
    for property in store.iter_type("flowproperties")? {
        let property = property?;
        let Some(group_id) = string(property.raw.get("unitGroupRefId")) else {
            continue;
        };
        indexes
            .property_groups
            .insert(property.internal_id.clone(), group_id.to_owned());
        indexes
            .group_properties
            .entry(group_id.to_owned())
            .or_default()
            .push(property.internal_id);
    }
    Ok(())
}

fn add_flow_indexes(store: &CanonicalStore, indexes: &mut Indexes) -> Result<(), AdapterError> {
    for flow in store.iter_type("flows")? {
        let flow = flow?;
        let Some(entries) = flow.raw.get("flowProperties").and_then(Value::as_array) else {
            continue;
        };
        let mut factors = BTreeMap::new();
        let mut first = None;
        let mut reference = None;
        for entry in entries.iter().filter_map(Value::as_object) {
            let Some(property) = entry.get("flowProperty").and_then(Value::as_object) else {
                continue;
            };
            let Some(id) = object_id(property) else {
                continue;
            };
            let Some(factor) = entry
                .get("conversionFactor")
                .map_or_else(|| Some(BigDecimal::from(1)), |value| decimal(Some(value)))
            else {
                continue;
            };
            let pair = (id.to_owned(), object_name(property).map(ToOwned::to_owned));
            first.get_or_insert_with(|| pair.clone());
            if entry
                .get("isRefFlowProperty")
                .and_then(Value::as_bool)
                .unwrap_or(false)
            {
                reference.get_or_insert_with(|| pair.clone());
            }
            factors.insert(id.to_owned(), factor);
        }
        let Some((reference_property_id, reference_property_name)) = reference.or(first) else {
            continue;
        };
        indexes.flows.insert(
            flow.internal_id,
            FlowRecord {
                factors,
                reference_property_id,
                reference_property_name,
            },
        );
    }
    Ok(())
}

fn preserve(exchange: &mut Map<String, Value>, source: &str, target: &str) {
    if let Some(value) = exchange.get(source).cloned() {
        exchange.insert(target.to_owned(), value);
    }
}

fn decimal(value: Option<&Value>) -> Option<BigDecimal> {
    match value? {
        Value::String(value) => BigDecimal::from_str(value.trim()).ok(),
        Value::Number(value) => BigDecimal::from_str(&value.to_string()).ok(),
        _ => None,
    }
}

fn decimal_text(value: &BigDecimal) -> String {
    value.normalized().to_string()
}

fn string(value: Option<&Value>) -> Option<&str> {
    value
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
}

fn object_id(object: &Map<String, Value>) -> Option<&str> {
    string(object.get("@id"))
}

fn object_name(object: &Map<String, Value>) -> Option<&str> {
    string(object.get("name"))
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use tidas_runtime::{CancellationToken, MemoryBudget};
    use tidas_validation::{ValidationRequest, validate_tidas_package};

    use super::*;
    use crate::model::CanonicalEntity;
    use crate::report::IssueSpool;
    use crate::writers::{TidasWriteRequest, write_tidas_package};

    const MASS_GROUP: &str = "11111111-1111-4111-8111-111111111111";
    const ENERGY_GROUP: &str = "22222222-2222-4222-8222-222222222222";
    const KG: &str = "33333333-3333-4333-8333-333333333333";
    const GRAM: &str = "44444444-4444-4444-8444-444444444444";
    const MJ: &str = "55555555-5555-4555-8555-555555555555";
    const UNKNOWN: &str = "66666666-6666-4666-8666-666666666666";
    const MASS_PROPERTY: &str = "77777777-7777-4777-8777-777777777777";
    const ENERGY_PROPERTY: &str = "88888888-8888-4888-8888-888888888888";
    const STEEL: &str = "99999999-9999-4999-8999-999999999999";
    const FUEL: &str = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa";
    const PROCESS: &str = "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb";

    #[test]
    fn frozen_python_unit_normalization_semantics_are_streamed() {
        let store = normalization_fixture();
        let cancellation = CancellationToken::default();
        let memory_budget = MemoryBudget::new(4 * 1024 * 1024);
        let context = AdapterContext {
            source: Path::new("fixture"),
            cancellation: &cancellation,
            memory_budget: &memory_budget,
            max_entry_bytes: 1024,
        };
        let mut issues = IssueSpool::new(Vec::new(), 64 * 1024);
        normalize_exchange_amounts(&context, &store, &mut issues).unwrap();
        let (issue_bytes, summary) = issues.finish().unwrap();
        let values = store
            .iter_process_exchanges(PROCESS)
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();

        assert_normalized_values(&values);
        assert_eq!(summary.warning_count, 2);
        let issue_text = String::from_utf8(issue_bytes).unwrap();
        assert!(issue_text.contains("exchange_amounts_normalized_to_reference_units"));
        assert!(issue_text.contains("exchange_unit_normalization_unresolved"));

        let output = tempfile::tempdir().unwrap();
        write_tidas_package(&TidasWriteRequest {
            store: &store,
            output_dir: output.path(),
            cancellation: &cancellation,
            memory_budget: &memory_budget,
        })
        .unwrap();
        let validation = validate_tidas_package(&ValidationRequest {
            input_dir: output.path().to_path_buf(),
            issue_spool: None,
            cancellation,
            memory_budget,
            queue_capacity: 2,
            progress: None,
        })
        .unwrap();
        assert!(validation.summary.ok);
    }

    fn normalization_fixture() -> CanonicalStore {
        let mut store = CanonicalStore::create(None).unwrap();
        add_entity(
            &mut store,
            "unitgroups",
            MASS_GROUP,
            json!([
                {"@id": KG, "name": "kg", "conversionFactor": 1, "referenceUnit": true},
                {"@id": GRAM, "name": "g", "conversionFactor": 0.001}
            ]),
        );
        add_entity(
            &mut store,
            "unitgroups",
            ENERGY_GROUP,
            json!([
                {"@id": MJ, "name": "MJ", "conversionFactor": 1, "referenceUnit": true}
            ]),
        );
        add_property(&mut store, MASS_PROPERTY, MASS_GROUP, "Mass");
        add_property(&mut store, ENERGY_PROPERTY, ENERGY_GROUP, "Energy");
        add_flow(&mut store, STEEL, vec![(MASS_PROPERTY, "Mass", "1", true)]);
        add_flow(
            &mut store,
            FUEL,
            vec![
                (MASS_PROPERTY, "Mass", "1", true),
                (ENERGY_PROPERTY, "Energy", "50", false),
            ],
        );
        store
            .add(&CanonicalEntity {
                entity_type: "processes".to_owned(),
                internal_id: PROCESS.to_owned(),
                external_id: Some(PROCESS.to_owned()),
                name: Some("Steel production".to_owned()),
                category_path: Vec::new(),
                raw: Map::from_iter([(
                    "dataQualityIndicators".to_owned(),
                    json!([
                        {"@name": "Methodological appropriateness and consistency", "@value": "Good"},
                        {"@name": "Completeness", "@value": "Very good"}
                    ]),
                )]),
            })
            .unwrap();
        store.begin_process_exchanges(PROCESS).unwrap();
        for exchange in [
            exchange(1, STEEL, "1", KG, "kg", MASS_PROPERTY),
            exchange(2, STEEL, "500", GRAM, "g", MASS_PROPERTY)
                .into_iter()
                .chain([
                    ("minimumAmount".to_owned(), json!(400)),
                    ("maximumAmount".to_owned(), json!(600)),
                ])
                .collect(),
            exchange(3, FUEL, "100", MJ, "MJ", ENERGY_PROPERTY),
            exchange(4, STEEL, "7", UNKNOWN, "bogus", MASS_PROPERTY),
        ] {
            store.add_process_exchange(PROCESS, &exchange).unwrap();
        }
        store
    }

    fn assert_normalized_values(values: &[Map<String, Value>]) {
        assert_eq!(values[0]["amount"], "1");
        assert_eq!(values[1]["amount"], "0.5");
        assert_eq!(values[1]["minimumAmount"], "0.4");
        assert_eq!(values[1]["maximumAmount"], "0.6");
        assert_eq!(values[1]["sourceAmount"], "500");
        assert_eq!(values[1]["sourceUnitId"], GRAM);
        assert_eq!(values[1]["unitId"], KG);
        assert_eq!(values[1]["amountNormalization"]["factor"], "0.001");
        assert_eq!(values[1]["amountNormalization"]["crossProperty"], false);
        assert_eq!(values[2]["amount"], "2");
        assert_eq!(values[2]["sourceUnitId"], MJ);
        assert_eq!(values[2]["unitId"], KG);
        assert_eq!(values[2]["flowPropertyRefId"], MASS_PROPERTY);
        assert_eq!(values[2]["sourceFlowPropertyRefId"], ENERGY_PROPERTY);
        assert_eq!(values[2]["amountNormalization"]["factor"], "0.02");
        assert_eq!(values[2]["amountNormalization"]["crossProperty"], true);
        assert_eq!(values[3]["amount"], "7");
        assert!(values[3].get("amountNormalization").is_none());
    }

    fn add_entity(store: &mut CanonicalStore, kind: &str, id: &str, units: Value) {
        store
            .add(&CanonicalEntity {
                entity_type: kind.to_owned(),
                internal_id: id.to_owned(),
                external_id: Some(id.to_owned()),
                name: Some(kind.to_owned()),
                category_path: Vec::new(),
                raw: Map::from_iter([("units".to_owned(), units)]),
            })
            .unwrap();
    }

    fn add_property(store: &mut CanonicalStore, id: &str, group_id: &str, name: &str) {
        store
            .add(&CanonicalEntity {
                entity_type: "flowproperties".to_owned(),
                internal_id: id.to_owned(),
                external_id: Some(id.to_owned()),
                name: Some(name.to_owned()),
                category_path: Vec::new(),
                raw: Map::from_iter([(
                    "unitGroupRefId".to_owned(),
                    Value::String(group_id.to_owned()),
                )]),
            })
            .unwrap();
    }

    fn add_flow(store: &mut CanonicalStore, id: &str, properties: Vec<(&str, &str, &str, bool)>) {
        let properties = properties
            .into_iter()
            .map(|(id, name, factor, reference)| {
                json!({
                    "flowProperty": {"@id": id, "name": name},
                    "conversionFactor": factor,
                    "isRefFlowProperty": reference,
                })
            })
            .collect();
        store
            .add(&CanonicalEntity {
                entity_type: "flows".to_owned(),
                internal_id: id.to_owned(),
                external_id: Some(id.to_owned()),
                name: Some(id.to_owned()),
                category_path: Vec::new(),
                raw: Map::from_iter([
                    ("flowProperties".to_owned(), Value::Array(properties)),
                    (
                        "flowName".to_owned(),
                        json!({
                            "treatmentStandardsRoutes": "fixture route",
                            "mixAndLocationTypes": "GLO"
                        }),
                    ),
                ]),
            })
            .unwrap();
    }

    fn exchange(
        internal_id: u64,
        flow_id: &str,
        amount: &str,
        unit_id: &str,
        unit_name: &str,
        property_id: &str,
    ) -> Map<String, Value> {
        Map::from_iter([
            ("internalId".to_owned(), json!(internal_id)),
            ("flowRefId".to_owned(), Value::String(flow_id.to_owned())),
            ("amount".to_owned(), Value::String(amount.to_owned())),
            ("unitId".to_owned(), Value::String(unit_id.to_owned())),
            ("unitName".to_owned(), Value::String(unit_name.to_owned())),
            (
                "flowPropertyRefId".to_owned(),
                Value::String(property_id.to_owned()),
            ),
        ])
    }
}
