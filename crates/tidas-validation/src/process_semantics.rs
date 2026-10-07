//! Pure allocation/reference consumer policy. No I/O and no authored-data mutation.
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

pub const PROCESS_SEMANTIC_PROFILE: &str = "tidas.process-allocation-reference.v1";
pub const ALLOCATION_SUM_TOLERANCE: f64 = 0.001_000_000_1;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExactFlowEvidence {
    pub uuid: String,
    pub version: String,
    pub r#type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_hash: Option<String>,
}

/// JSON-compatible analysis shared with the TypeScript SDK conformance contract.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcessSemanticAnalysis {
    pub profile: &'static str,
    pub tolerance: f64,
    pub complete: bool,
    pub valid: bool,
    pub coverage: Vec<Value>,
    pub validation_issues: Vec<Value>,
    pub reference: Value,
    pub interpretations: Vec<Value>,
}
impl ProcessSemanticAnalysis {
    fn check(&mut self, name: &str, path: &[Value], status: &str, code: &str, params: Value) {
        self.coverage
            .push(json!({"check":name,"path":path,"status":status}));
        if status == "unresolved" {
            self.complete = false;
        }
        if status == "invalid" || status == "unresolved" {
            self.valid = false;
            let mut issue = json!({"code":code,"path":path,"severity":"error","message":code,"rawCode":"process_semantics"});
            issue
                .as_object_mut()
                .expect("issue is an object")
                .insert("params".to_owned(), params);
            self.validation_issues.push(issue);
        }
    }
}
fn path(base: &[Value], field: &str) -> Vec<Value> {
    let mut p = base.to_vec();
    p.push(json!(field));
    p
}
fn entries<'a>(value: &'a Value, base: &[Value]) -> Vec<(&'a Value, Vec<Value>)> {
    match value {
        Value::Null => vec![],
        Value::Array(a) => a
            .iter()
            .enumerate()
            .map(|(i, v)| {
                let mut p = base.to_vec();
                p.push(json!(i));
                (v, p)
            })
            .collect(),
        _ => vec![(value, base.to_vec())],
    }
}
fn id(value: &Value) -> Option<String> {
    if let Some(s) = value.as_str()
        && !s.is_empty()
        && s.len() <= 6
        && (s == "0" || !s.starts_with('0'))
        && s.bytes().all(|b| b.is_ascii_digit())
    {
        return Some(s.to_owned());
    }
    value
        .as_u64()
        .filter(|v| *v <= 999_999)
        .map(|v| v.to_string())
}
fn decimal(s: &str) -> bool {
    let s = s.strip_prefix(['+', '-']).unwrap_or(s);
    let mut exp = s.split(['e', 'E']);
    let mantissa = exp.next().unwrap_or("");
    if let Some(e) = exp.next() {
        let e = e.strip_prefix(['+', '-']).unwrap_or(e);
        if e.is_empty() || !e.bytes().all(|b| b.is_ascii_digit()) {
            return false;
        }
    }
    if exp.next().is_some() {
        return false;
    }
    let mut dot = false;
    let mut digits = 0;
    for b in mantissa.bytes() {
        if b == b'.' && !dot {
            dot = true;
        } else if b.is_ascii_digit() {
            digits += 1;
        } else {
            return false;
        }
    }
    digits > 0
}
fn fraction(value: &Value) -> Option<f64> {
    let n = if let Some(n) = value.as_f64() {
        n
    } else {
        let s = value.as_str()?.trim();
        if !decimal(s) {
            return None;
        }
        s.parse::<f64>().ok()?
    };
    (n.is_finite() && (0.0..=100.0).contains(&n)).then_some(n)
}
fn legacy_fraction(value: &Value) -> Option<f64> {
    if let Some(text) = value.as_str() {
        let text = text.trim();
        fraction(&json!(text.strip_suffix('%').unwrap_or(text).trim()))
    } else {
        fraction(value)
    }
}
fn coefficients(ids: &[String], value: impl Fn(&str) -> f64) -> Value {
    json!(
        ids.iter()
            .map(|id| json!({"referenceId":id,"coefficient":value(id)}))
            .collect::<Vec<_>>()
    )
}

/// Caller supplies exact, bounded Flow evidence. Missing evidence never passes applicable checks.
#[must_use]
#[allow(clippy::too_many_lines)] // Shared policy state machine preserves deterministic finding order.
pub fn analyze_process_semantics(
    process: &Value,
    flows: &[ExactFlowEvidence],
) -> ProcessSemanticAnalysis {
    let mut r = ProcessSemanticAnalysis {
        profile: PROCESS_SEMANTIC_PROFILE,
        tolerance: ALLOCATION_SUM_TOLERANCE,
        complete: true,
        valid: true,
        coverage: vec![],
        validation_issues: vec![],
        reference: json!({"type":"","ids":[],"calculationApplicability":"unsupported"}),
        interpretations: vec![],
    };
    let dataset = &process["processDataSet"];
    let ex_path = vec![
        json!("processDataSet"),
        json!("exchanges"),
        json!("exchange"),
    ];
    let exchanges = entries(&dataset["exchanges"]["exchange"], &ex_path);
    let mut indexed: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    for (i, (ex, p)) in exchanges.iter().enumerate() {
        if let Some(key) = id(&ex["@dataSetInternalID"]) {
            indexed.entry(key).or_default().push(i);
        } else {
            r.check(
                "exchange-identity",
                &path(p, "@dataSetInternalID"),
                "invalid",
                "exchange_internal_id_invalid",
                json!({}),
            );
        }
    }
    // Declaration order is shared with JS Map insertion order.
    let mut recorded = BTreeSet::new();
    for (ex, _) in &exchanges {
        if let Some(key) = id(&ex["@dataSetInternalID"])
            && recorded.insert(key.clone())
        {
            let matches = &indexed[&key];
            for i in matches {
                r.check(
                    "exchange-identity",
                    &path(&exchanges[*i].1, "@dataSetInternalID"),
                    if matches.len() == 1 {
                        "passed"
                    } else {
                        "invalid"
                    },
                    "exchange_internal_id_ambiguous",
                    json!({"id":key}),
                );
            }
        }
    }
    let qr_path = vec![
        json!("processDataSet"),
        json!("processInformation"),
        json!("quantitativeReference"),
    ];
    let qr = &dataset["processInformation"]["quantitativeReference"];
    let qr_type = qr["@type"].as_str().unwrap_or("");
    r.reference["type"] = json!(qr_type);
    let mut ids = vec![];
    if qr_type == "Reference flow(s)" {
        let refs = entries(
            &qr["referenceToReferenceFlow"],
            &path(&qr_path, "referenceToReferenceFlow"),
        );
        let mut seen = BTreeSet::new();
        let mut valid = true;
        if refs.is_empty() {
            valid = false;
            r.check(
                "quantitative-reference",
                &path(&qr_path, "referenceToReferenceFlow"),
                "invalid",
                "quantitative_reference_missing",
                json!({}),
            );
        }
        for (v, p) in &refs {
            let key = id(v);
            let matches = key
                .as_ref()
                .and_then(|k| indexed.get(k))
                .map_or(0, Vec::len);
            let code = match key.as_ref() {
                None => "quantitative_reference_invalid",
                Some(k) if seen.contains(k) => "quantitative_reference_duplicate",
                _ if matches == 0 => "quantitative_reference_missing",
                _ if matches > 1 => "quantitative_reference_ambiguous",
                _ => "",
            };
            r.check(
                "quantitative-reference",
                p,
                if code.is_empty() {
                    "passed"
                } else {
                    valid = false;
                    "invalid"
                },
                code,
                json!({}),
            );
            if let Some(k) = key {
                seen.insert(k.clone());
                ids.push(k);
            }
        }
        if refs.len() == 1 && valid {
            r.reference["calculationApplicability"] = json!("single-reference");
        }
    } else if ["Functional unit", "Other parameter", "Production period"].contains(&qr_type) {
        let texts = entries(
            &qr["functionalUnitOrOther"],
            &path(&qr_path, "functionalUnitOrOther"),
        );
        let valid = !texts.is_empty()
            && texts.iter().all(|(v, _)| {
                v["@xml:lang"].as_str().is_some()
                    && v["#text"].as_str().is_some_and(|s| !s.trim().is_empty())
            });
        r.check(
            "quantitative-reference",
            &path(&qr_path, "functionalUnitOrOther"),
            if valid { "passed" } else { "invalid" },
            "quantitative_reference_basis_missing",
            json!({}),
        );
    } else {
        r.check(
            "quantitative-reference",
            &path(&qr_path, "@type"),
            "invalid",
            "quantitative_reference_type_invalid",
            json!({}),
        );
    }
    r.reference["ids"] = json!(ids);
    let mut legacy: Vec<(usize, f64, Vec<Value>, String, Value)> = vec![];
    let mut explicit_count = 0;
    for (ex, p) in &exchanges {
        let key = id(&ex["@dataSetInternalID"]).unwrap_or_default();
        let raw = &ex["allocations"]["allocation"];
        let ap = path(&path(p, "allocations"), "allocation");
        let mut interpretation = json!({"exchangeId":key,"mode":"invalid","coefficients":[]});
        if ex.get("allocations").is_none() || raw.as_object().is_some_and(serde_json::Map::is_empty)
        {
            interpretation["mode"] = json!(if ex.get("allocations").is_none() {
                "undeclared"
            } else {
                "legacy-scalar-empty"
            });
            interpretation["coefficients"] = coefficients(&ids, |_| 1.0);
            r.check("allocation-vector", &ap, "not-applicable", "", json!({}));
            r.interpretations.push(interpretation);
            continue;
        }
        let allocations = entries(raw, &ap);
        if allocations.is_empty()
            || allocations
                .iter()
                .any(|(v, _)| v.as_object().is_none_or(serde_json::Map::is_empty))
        {
            r.check(
                "allocation-vector",
                &ap,
                "invalid",
                "allocation_vector_malformed",
                json!({}),
            );
            r.interpretations.push(interpretation);
            continue;
        }
        let targeted = allocations
            .iter()
            .filter(|(v, _)| v.get("@internalReferenceToCoProduct").is_some())
            .count();
        if targeted > 0 && targeted != allocations.len() {
            r.check(
                "allocation-vector",
                &ap,
                "invalid",
                "allocation_mixed_modes",
                json!({}),
            );
            r.interpretations.push(interpretation);
            continue;
        }
        if targeted == 0 {
            if allocations.len() != 1 {
                r.check(
                    "allocation-vector",
                    &ap,
                    "invalid",
                    "allocation_targetless_malformed",
                    json!({}),
                );
            } else if let Some(n) = legacy_fraction(&allocations[0].0["@allocatedFraction"]) {
                legacy.push((
                    r.interpretations.len(),
                    n,
                    ap,
                    ex["exchangeDirection"].as_str().unwrap_or("").to_owned(),
                    allocations[0].0["@allocatedFraction"].clone(),
                ));
            } else {
                r.check(
                    "allocation-fraction",
                    &path(&allocations[0].1, "@allocatedFraction"),
                    "invalid",
                    "allocation_fraction_invalid",
                    json!({}),
                );
            }
            r.interpretations.push(interpretation);
            continue;
        }
        explicit_count += 1;
        let mut seen = BTreeSet::new();
        let mut amounts = BTreeMap::new();
        let mut total = 0.0;
        let mut fractions_valid = true;
        let initial = r.validation_issues.len();
        for (a, p) in &allocations {
            let target = id(&a["@internalReferenceToCoProduct"]);
            let tp = path(p, "@internalReferenceToCoProduct");
            let matches = target.as_ref().and_then(|t| indexed.get(t));
            let count = matches.map_or(0, Vec::len);
            let code = match target.as_ref() {
                None => "allocation_target_id_invalid",
                Some(t) if seen.contains(t) => "allocation_target_duplicate",
                _ if count == 0 => "allocation_coproduct_reference_missing",
                _ if count > 1 => "allocation_target_ambiguous",
                _ => "",
            };
            r.check(
                "allocation-target",
                &tp,
                if code.is_empty() { "passed" } else { "invalid" },
                code,
                json!({}),
            );
            if let Some(t) = &target {
                seen.insert(t.clone());
            }
            if count == 1 {
                let target_ex = exchanges[matches.unwrap()[0]].0;
                let direction = target_ex["exchangeDirection"].as_str().unwrap_or("");
                r.check(
                    "allocation-target-direction",
                    &tp,
                    if ["Input", "Output"].contains(&direction) {
                        "passed"
                    } else {
                        "invalid"
                    },
                    "allocation_target_direction_invalid",
                    json!({}),
                );
                let reference = &target_ex["referenceToFlowDataSet"];
                let (status, code) = match (
                    reference["@refObjectId"].as_str(),
                    reference["@version"].as_str(),
                ) {
                    (Some(uuid), Some(version)) if !uuid.is_empty() && !version.is_empty() => {
                        let found = flows
                            .iter()
                            .filter(|f| f.uuid == uuid && f.version == version)
                            .collect::<Vec<_>>();
                        if found.len() > 1 {
                            ("unresolved", "allocation_flow_evidence_ambiguous")
                        } else if let Some(f) = found.first() {
                            match f.r#type.as_str() {
                                "Product flow" | "Waste flow" => ("passed", ""),
                                "Elementary flow" | "Other flow" => {
                                    ("invalid", "allocation_target_flow_type_invalid")
                                }
                                _ => ("unresolved", "allocation_flow_evidence_invalid"),
                            }
                        } else if flows.iter().any(|f| f.uuid == uuid) {
                            ("unresolved", "allocation_flow_version_mismatch")
                        } else {
                            ("unresolved", "allocation_flow_evidence_unavailable")
                        }
                    }
                    _ => ("unresolved", "allocation_flow_reference_inexact"),
                };
                r.check("allocation-target-type", &tp, status, code, json!({}));
            }
            let n = fraction(&a["@allocatedFraction"]);
            r.check(
                "allocation-fraction",
                &path(p, "@allocatedFraction"),
                if n.is_some() { "passed" } else { "invalid" },
                "allocation_fraction_invalid",
                json!({}),
            );
            if let Some(n) = n {
                total += n;
                if let Some(t) = target {
                    amounts.insert(t, n / 100.0);
                }
            } else {
                fractions_valid = false;
            }
        }
        if fractions_valid {
            r.check(
                "allocation-vector",
                &ap,
                if (total - 100.0).abs() <= ALLOCATION_SUM_TOLERANCE {
                    "passed"
                } else {
                    "invalid"
                },
                "allocation_vector_sum_invalid",
                json!({"sum":total}),
            );
        }
        if r.validation_issues.len() == initial {
            interpretation["mode"] = json!("explicit-vector");
            interpretation["allocations"]=json!(allocations.iter().map(|(a,_)|json!({"targetId":id(&a["@internalReferenceToCoProduct"]).unwrap(),"fraction":fraction(&a["@allocatedFraction"]).unwrap()})).collect::<Vec<_>>());
            interpretation["coefficients"] =
                coefficients(&ids, |id| amounts.get(id).copied().unwrap_or(0.0));
        }
        r.interpretations.push(interpretation);
    }
    if !legacy.is_empty() {
        let output_shares = legacy.iter().any(|l| l.3 == "Output");
        let mixed = explicit_count > 0 || (output_shares && legacy.iter().any(|l| l.3 != "Output"));
        let sum = legacy.iter().map(|l| l.1).sum::<f64>();
        let shares = legacy
            .iter()
            .map(|l| json!({"targetId":r.interpretations[l.0]["exchangeId"],"fraction":l.1}))
            .collect::<Vec<_>>();
        let mut accepted_process_mode = true;
        for (index, n, p, _, raw) in &legacy {
            let unique_reference = r.reference["calculationApplicability"] == "single-reference";
            let full = n.to_bits() == 100.0_f64.to_bits()
                && raw
                    .as_str()
                    .is_none_or(|s| !s.contains('%') || s.trim() == "100%");
            let code = if mixed {
                "allocation_mixed_modes"
            } else if output_shares {
                if (sum - 100.0).abs() <= ALLOCATION_SUM_TOLERANCE {
                    ""
                } else {
                    "allocation_legacy_sum_invalid"
                }
            } else if !unique_reference {
                "allocation_legacy_reference_ambiguous"
            } else if !full {
                "allocation_legacy_full_invalid"
            } else {
                ""
            };
            let accepted = code.is_empty();
            accepted_process_mode &= accepted;
            r.check(
                "allocation-legacy",
                p,
                if accepted { "passed" } else { "invalid" },
                code,
                json!({"sum":if output_shares {sum} else {*n}}),
            );
            if accepted && !output_shares {
                r.interpretations[*index]["mode"] = json!("legacy-targetless-full");
                r.interpretations[*index]["coefficients"] = coefficients(&ids, |_| 1.0);
                r.interpretations[*index]["allocations"] =
                    json!([{"targetId":ids[0],"fraction":100}]);
            }
        }
        if output_shares && accepted_process_mode {
            for interpretation in &mut r.interpretations {
                interpretation["mode"] = json!("legacy-output-share");
                interpretation["allocations"] = json!(shares);
                interpretation["coefficients"] = coefficients(&ids, |id| {
                    shares
                        .iter()
                        .find(|s| s["targetId"] == id)
                        .map_or(100.0, |s| s["fraction"].as_f64().unwrap())
                        / 100.0
                });
            }
        }
    }
    if !r
        .coverage
        .iter()
        .any(|c| c["check"] == "allocation-target-type")
    {
        r.check(
            "allocation-target-type",
            &ex_path,
            "not-applicable",
            "",
            json!({}),
        );
    }
    r
}
