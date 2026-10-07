use serde_json::Value;
use tidas_validation::{ExactFlowEvidence, analyze_process_semantics};
#[test]
fn shared_sdk_native_conformance() {
    let fixture: Value =
        serde_json::from_str(include_str!("fixtures/process-semantics.v1.json")).unwrap();
    for c in fixture["cases"].as_array().unwrap() {
        let flows: Vec<ExactFlowEvidence> = serde_json::from_value(c["flows"].clone()).unwrap();
        let before = c["process"].clone();
        let r = serde_json::to_value(analyze_process_semantics(&c["process"], &flows)).unwrap();
        assert_eq!(r["valid"], c["expected"]["valid"], "{}", c["name"]);
        assert_eq!(r["complete"], c["expected"]["complete"], "{}", c["name"]);
        let codes: Vec<&Value> = r["validationIssues"]
            .as_array()
            .unwrap()
            .iter()
            .map(|i| &i["code"])
            .collect();
        assert_eq!(
            serde_json::to_value(codes).unwrap(),
            c["expected"]["codes"],
            "{}",
            c["name"]
        );
        if let Some(modes) = c["expected"].get("modes") {
            let actual: Vec<&Value> = r["interpretations"]
                .as_array()
                .unwrap()
                .iter()
                .map(|i| &i["mode"])
                .collect();
            assert_eq!(
                serde_json::to_value(actual).unwrap(),
                *modes,
                "{}",
                c["name"]
            );
        }
        if let Some(coefficients) = c["expected"].get("coefficients") {
            let actual: Vec<f64> = r["interpretations"]
                .as_array()
                .unwrap()
                .iter()
                .map(|i| i["coefficients"][0]["coefficient"].as_f64().unwrap())
                .collect();
            let expected: Vec<f64> = serde_json::from_value(coefficients.clone()).unwrap();
            assert_eq!(actual, expected, "{}", c["name"]);
        }
        for (expected_name, actual_name) in [
            ("allocationVectors", "allocations"),
            ("coefficientVectors", "coefficients"),
        ] {
            if let Some(expected) = c["expected"].get(expected_name) {
                let actual = r["interpretations"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|i| {
                        if actual_name == "coefficients" {
                            serde_json::json!(
                                i[actual_name]
                                    .as_array()
                                    .unwrap()
                                    .iter()
                                    .map(|c| c["coefficient"].clone())
                                    .collect::<Vec<_>>()
                            )
                        } else {
                            i.get(actual_name)
                                .cloned()
                                .unwrap_or_else(|| serde_json::json!([]))
                        }
                    })
                    .collect::<Vec<_>>();
                assert_eq!(
                    normalize_numbers(serde_json::json!(actual)),
                    normalize_numbers(expected.clone()),
                    "{} {expected_name}",
                    c["name"]
                );
            }
        }
        assert_eq!(c["process"], before);
        assert_eq!(
            r,
            serde_json::to_value(analyze_process_semantics(&c["process"], &flows)).unwrap()
        );
    }
}

fn normalize_numbers(value: Value) -> Value {
    match value {
        Value::Number(n) => serde_json::json!(n.as_f64().unwrap()),
        Value::Array(a) => Value::Array(a.into_iter().map(normalize_numbers).collect()),
        Value::Object(o) => Value::Object(
            o.into_iter()
                .map(|(k, v)| (k, normalize_numbers(v)))
                .collect(),
        ),
        other => other,
    }
}
