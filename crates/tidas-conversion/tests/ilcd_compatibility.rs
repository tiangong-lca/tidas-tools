use serde_json::{Value, json};
use std::{fs, path::Path};
use tempfile::tempdir;
use tidas_conversion::{
    ConversionDirection, ConversionRequest, convert_directory, convert_xml_to_json,
    project_tidas_to_eilcd, restore_tidas_projection,
};
use tidas_runtime::{CancellationToken, MemoryBudget};
use tidas_validation::{ValidationRequest, validate_ilcd_package};

fn request(input: &Path, output: &Path, direction: ConversionDirection) -> ConversionRequest {
    ConversionRequest {
        input_dir: input.into(),
        output_dir: output.into(),
        direction,
        cancellation: CancellationToken::default(),
        memory_budget: MemoryBudget::new(32 * 1024 * 1024),
        queue_capacity: 2,
        progress: None,
    }
}
fn collapsed_pointer<'a>(value: &'a Value, path: &str) -> Option<&'a Value> {
    let mut node = value;
    for token in path.split('/').skip(1) {
        if token == "0" && node.is_object() {
            continue;
        }
        node = if let Some(items) = node.as_array() {
            items.get(token.parse::<usize>().ok()?)?
        } else {
            node.get(token)?
        };
    }
    Some(node)
}
fn reference(kind: &str) -> Value {
    json!({"@type":kind,"@refObjectId":"11111111-1111-4111-8111-111111111111","@version":"01.00.000"})
}
fn lcia(legacy: bool) -> Value {
    let scope = if legacy { "common:scope" } else { "scope" };
    let method = if legacy { "common:method" } else { "method" };
    let location = if legacy {
        "intervensionSubLocation"
    } else {
        "interventionSubLocation"
    };
    let source = if legacy {
        "referenceToDataSource"
    } else {
        "referencesToDataSource"
    };
    json!({"LCIAMethodDataSet":{
        "@xmlns":"http://lca.jrc.it/ILCD/LCIAMethod", "@xmlns:common":"http://lca.jrc.it/ILCD/Common", "@version":"1.1",
        "LCIAMethodInformation": {"dataSetInformation":{"common:UUID":"22222222-2222-4222-8222-222222222222","methodology":["Method A","Method B"]},"geography":{(location):{"#text":"GLO"}}},
        "modellingAndValidation":{"validation":{"review":[{"@type":"Independent external review",(scope):[{"@name":"Documentation",(method):{"@name":"Expert judgement"}}],"common:reviewDetails":{"@xml:lang":"en","#text":"Reviewed by experts"}},{"@type":"Not reviewed"}]}},
        "characterisationFactors":{"factor":[{"referenceToFlowDataSet":reference("flow data set"),"exchangeDirection":"Output","meanValue":"1.5",(source):if legacy {reference("source data set")} else {json!({"referenceToDataSource":reference("source data set")})}}]}
    }})
}
fn model(legacy: bool) -> Value {
    let scaling = if legacy {
        "scalingFactors"
    } else {
        "scalingFactor"
    };
    let value = if legacy { "parameter" } else { "#text" };
    json!({"lifeCycleModelDataSet":{
        "@xmlns":"http://eplca.jrc.ec.europa.eu/ILCD/LifeCycleModel/2017","@locations":"../ILCDLocations.xml","@xmlns:common":"http://lca.jrc.it/ILCD/Common","@version":"1.1",
        "lifeCycleModelInformation":{"dataSetInformation":{"common:UUID":"33333333-3333-4333-8333-333333333333"},"technology":{"processes":{"processInstance":[{"@dataSetInternalID":"1","@multiplicationFactor":"1",(scaling):"2","parameters":{"parameter":[{"@name":"a",(value):"1.5"},{"@name":"b",(value):"-2"}]}}]}}},
        "modellingAndValidation":{},"administrativeInformation":{}
    }})
}

#[test]
fn canonical_and_legacy_fields_reach_native_xml_and_roundtrip() {
    for legacy in [false, true] {
        let temp = tempdir().unwrap();
        let input = temp.path().join("in");
        let xml = temp.path().join("xml");
        let back = temp.path().join("back");
        for (category, doc) in [
            ("lciamethods", lcia(legacy)),
            ("lifecyclemodels", model(legacy)),
        ] {
            fs::create_dir_all(input.join(category)).unwrap();
            fs::write(
                input.join(category).join("sample.json"),
                serde_json::to_vec(&doc).unwrap(),
            )
            .unwrap();
        }
        convert_directory(&request(&input, &xml, ConversionDirection::TidasToIlcd)).unwrap();
        let issues = temp.path().join("issues.jsonl");
        let validation = validate_ilcd_package(&ValidationRequest {
            input_dir: xml.join("data"),
            issue_spool: Some(issues.clone()),
            cancellation: CancellationToken::default(),
            memory_budget: MemoryBudget::new(32 * 1024 * 1024),
            queue_capacity: 2,
            progress: None,
        })
        .unwrap();
        assert!(
            validation.summary.ok,
            "{}",
            fs::read_to_string(issues).unwrap()
        );
        let method_xml = fs::read(xml.join("data/lciamethods/sample.xml")).unwrap();
        let native: Value = serde_json::from_slice(
            &convert_xml_to_json(&method_xml, &CancellationToken::default()).unwrap(),
        )
        .unwrap();
        assert_eq!(
            native.pointer(
                "/LCIAMethodDataSet/modellingAndValidation/validation/review/0/scope/method/@name"
            ),
            Some(&json!("Expert judgement"))
        );
        assert!(native.pointer("/LCIAMethodDataSet/characterisationFactors/factor/referencesToDataSource/referenceToDataSource").is_some());
        let model_xml = fs::read(xml.join("data/lifecyclemodels/sample.xml")).unwrap();
        let native: Value = serde_json::from_slice(
            &convert_xml_to_json(&model_xml, &CancellationToken::default()).unwrap(),
        )
        .unwrap();
        assert_eq!(native.pointer("/lifeCycleModelDataSet/lifeCycleModelInformation/technology/processes/processInstance/scalingFactor"),Some(&json!("2")));
        assert_eq!(native.pointer("/lifeCycleModelDataSet/lifeCycleModelInformation/technology/processes/processInstance/parameters/parameter/1/#text"),Some(&json!("-2")));
        convert_directory(&request(
            &xml.join("data"),
            &back,
            ConversionDirection::IlcdToTidas,
        ))
        .unwrap();
        for (category, original) in [
            ("lciamethods", lcia(legacy)),
            ("lifecyclemodels", model(legacy)),
        ] {
            let recovered: Value = serde_json::from_slice(
                &fs::read(back.join("data").join(category).join("sample.json")).unwrap(),
            )
            .unwrap();
            // XML collapses singleton arrays; projection recovery must restore every aliased fragment.
            if legacy {
                let projected = project_tidas_to_eilcd(&original, category).unwrap();
                for restoration in projected.recovery.unwrap().restorations {
                    assert_eq!(
                        collapsed_pointer(&recovered, &restoration.path),
                        original.pointer(&restoration.path)
                    );
                }
            }
        }
    }
}

#[test]
fn competing_aliases_fail_without_replacing_an_existing_output() {
    let temp = tempdir().unwrap();
    let input = temp.path().join("in");
    let output = temp.path().join("out");
    fs::create_dir_all(input.join("lciamethods")).unwrap();
    fs::create_dir_all(&output).unwrap();
    fs::write(output.join("keep.txt"), "keep").unwrap();
    let mut doc = lcia(true);
    doc["LCIAMethodDataSet"]["LCIAMethodInformation"]["geography"]["interventionSubLocation"] =
        json!("DE");
    fs::write(
        input.join("lciamethods/sample.json"),
        serde_json::to_vec(&doc).unwrap(),
    )
    .unwrap();
    let err =
        convert_directory(&request(&input, &output, ConversionDirection::TidasToIlcd)).unwrap_err();
    assert!(err.to_string().contains("conflicting legacy field"));
    assert_eq!(fs::read_to_string(output.join("keep.txt")).unwrap(), "keep");
    assert_eq!(fs::read_dir(output).unwrap().count(), 1);
}

#[test]
fn native_process_variables_results_and_review_details_survive_projection() {
    let source = json!({"processDataSet":{"processInformation":{"mathematicalRelations":{"variableParameter":[{"@name":"a","meanValue":"1"},{"@name":"b","meanValue":"2"}]}},"LCIAResults":{"LCIAResult":[{"meanAmount":"1"},{"meanAmount":"2"}]},"modellingAndValidation":{"validation":{"review":[{"common:reviewDetails":{"@xml:lang":"en","#text":"Review evidence"}}]}}}});
    assert_eq!(
        project_tidas_to_eilcd(&source, "processes")
            .unwrap()
            .document,
        source
    );
}

#[test]
fn intentional_primary_compliance_and_citation_limits_remain_recoverable() {
    let cases = [
        (
            "lifecyclemodels",
            json!({"lifeCycleModelDataSet":{"modellingAndValidation":{"complianceDeclarations":[{"compliance":[{"common:overallCompliance":"Fully compliant"},{"common:overallCompliance":"Not compliant"}]},{"compliance":{"common:overallCompliance":"Not defined"}}]}}}),
            "select-primary-lifecycle-model-compliance",
        ),
        (
            "sources",
            json!({"sourceDataSet":{"sourceInformation":{"dataSetInformation":{"sourceCitation":"字".repeat(1001)}}}}),
            "truncate-ilcd-source-citation",
        ),
    ];
    for (category, source, rule) in cases {
        let projection = project_tidas_to_eilcd(&source, category).unwrap();
        let recovery = projection.recovery.unwrap();
        assert_eq!(recovery.adaptations.get(rule), Some(&1));
        if category == "sources" {
            assert_eq!(
                projection
                    .document
                    .pointer("/sourceDataSet/sourceInformation/dataSetInformation/sourceCitation")
                    .unwrap()
                    .as_str()
                    .unwrap()
                    .chars()
                    .count(),
                1000
            );
        } else {
            assert_eq!(
                projection
                    .document
                    .pointer("/lifeCycleModelDataSet/modellingAndValidation/complianceDeclarations")
                    .unwrap()
                    .as_array()
                    .unwrap()
                    .len(),
                2
            );
        }
        let mut restored = projection.document;
        restore_tidas_projection(&mut restored, &recovery).unwrap();
        assert_eq!(restored, source);
    }
}

// Keep the complete audited family table together for review.
#[test]
#[allow(clippy::too_many_lines)]
fn preserved_projection_families_have_explicit_recovery_evidence() {
    let cases = [
        (
            "contacts",
            json!({"contactDataSet":{"reference":{"@uri":"../contacts/a.json"}}}),
            "map-tidas-reference-uri",
        ),
        (
            "contacts",
            json!({"contactDataSet":{"text":[{"@xml:lang":"en","#text":"one"},{"@xml:lang":"en","#text":"two"}]}}),
            "merge-localized-language",
        ),
        (
            "contacts",
            json!({"contactDataSet":{"tidasimport:note":"note"}}),
            "omit-unbound-extension-element",
        ),
        (
            "contacts",
            json!({"contactDataSet":{"items":[]}}),
            "omit-empty-array",
        ),
        (
            "contacts",
            json!({"contactDataSet":{"text":" a\r\nb "}}),
            "normalize-xml-character-data",
        ),
        (
            "contacts",
            json!({"contactDataSet":{"text":{"#text":""}}}),
            "omit-empty-text",
        ),
        (
            "contacts",
            json!({"contactDataSet":{"geography":{}}}),
            "omit-empty-optional-element",
        ),
        (
            "processes",
            json!({"processDataSet":{"processInformation":{"time":{"timeRepresentativenessDescription":{"@xml:lang":"en","#text":"Time"}}}}}),
            "map-process-time-description",
        ),
        (
            "processes",
            json!({"processDataSet":{"generatedFromLifecycleModel":"extension"}}),
            "omit-tidas-process-extension",
        ),
        (
            "processes",
            json!({"processDataSet":{"processInformation":{"mathematicalRelations":{"variableParameter":{"meanValue":"2"}}}}}),
            "omit-incomplete-variable-parameter",
        ),
        (
            "processes",
            json!({"processDataSet":{"dataQualityIndicators":{}}}),
            "omit-empty-data-quality-indicators",
        ),
        (
            "processes",
            json!({"processDataSet":{"unmatched:placeholder":"placeholder"}}),
            "omit-import-placeholder",
        ),
        (
            "processes",
            json!({"processDataSet":{"generalComment":{"@xml:lang":"en","#text":"x".repeat(501)}}}),
            "truncate-ilcd-multilang-text",
        ),
        (
            "flows",
            json!({"flowDataSet":{"common:dateOfLastRevision":"date"}}),
            "omit-unsupported-flow-extension",
        ),
        (
            "flows",
            json!({"flowDataSet":{"category":{"@classId":"","@catId":"a"}}}),
            "omit-empty-elementary-class-id",
        ),
        (
            "flows",
            json!({"flowDataSet":{"modellingAndValidation":{"validation":{"review":"note"}}}}),
            "omit-unsupported-flow-validation",
        ),
        (
            "flows",
            json!({"flowDataSet":{"administrativeInformation":{"publicationAndOwnership":{"common:licenseType":"license"}}}}),
            "omit-unsupported-flow-publication-metadata",
        ),
        (
            "flows",
            json!({"flowDataSet":{"flowInformation":{"dataSetInformation":{"common:shortName":"short"}}}}),
            "omit-unsupported-flow-short-name",
        ),
        (
            "lifecyclemodels",
            json!({"lifeCycleModelDataSet":{"common:workflowAndPublicationStatus":"draft"}}),
            "omit-unsupported-lifecycle-model-metadata",
        ),
        (
            "lifecyclemodels",
            json!({"lifeCycleModelDataSet":{"administrativeInformation":{"dataEntryBy":{"common:referenceToDataSetUseApproval":reference("source data set")}}}}),
            "omit-unsupported-lifecycle-model-source-reference",
        ),
        (
            "lifecyclemodels",
            json!({"lifeCycleModelDataSet":{"lifeCycleModelInformation":{"dataSetInformation":{"name":{"flowProperties":{"@xml:lang":"en","#text":"mass"}}}}}}),
            "map-lifecycle-model-flow-properties",
        ),
        (
            "lifecyclemodels",
            json!({"lifeCycleModelDataSet":{"modellingAndValidation":{"validation":{"review":{"@type":"Independent external review"}}}}}),
            "omit-unsupported-lifecycle-model-review-type",
        ),
        (
            "lciamethods",
            json!({"LCIAMethodDataSet":{"common:dateOfLastRevision":"date"}}),
            "omit-unsupported-lcia-method-metadata",
        ),
        (
            "sources",
            json!({"sourceDataSet":{"common:licenseType":"license"}}),
            "omit-unsupported-source-metadata",
        ),
    ];
    for (category, source, rule) in cases {
        let projected = project_tidas_to_eilcd(&source, category).unwrap();
        let recovery = projected
            .recovery
            .unwrap_or_else(|| panic!("missing recovery for {rule}"));
        assert!(
            recovery.adaptations.contains_key(rule),
            "{rule}: {:?}",
            recovery.adaptations
        );
        let mut restored = projected.document;
        restore_tidas_projection(&mut restored, &recovery).unwrap();
        assert_eq!(restored, source, "{rule}");
    }
}

#[test]
fn contact_classification_extension_moves_without_losing_legacy_structure() {
    let source = json!({"contactDataSet":{"contactInformation":{"dataSetInformation":{"classificationInformation":{"common:classification":{"@name":"External","common:class":{"@level":"0","#text":"A"}},"common:other":{"ext:note":"extra"}}}}}});
    let projection = project_tidas_to_eilcd(&source, "contacts").unwrap();
    assert_eq!(projection.document.pointer("/contactDataSet/contactInformation/dataSetInformation/classificationInformation/common:classification/common:other/ext:note"),Some(&json!("extra")));
    let mut restored = projection.document;
    restore_tidas_projection(&mut restored, projection.recovery.as_ref().unwrap()).unwrap();
    assert_eq!(restored, source);
    let mut ambiguous = source.clone();
    let info = ambiguous
        .pointer_mut(
            "/contactDataSet/contactInformation/dataSetInformation/classificationInformation",
        )
        .unwrap();
    info["common:classification"] = json!([
        info["common:classification"].clone(),
        info["common:classification"].clone()
    ]);
    assert!(project_tidas_to_eilcd(&ambiguous, "contacts").is_err());
}

#[test]
fn legacy_recovery_cannot_mask_edits_to_native_xml_values() {
    let temp = tempdir().unwrap();
    let input = temp.path().join("in");
    let xml = temp.path().join("xml");
    let back = temp.path().join("back");
    fs::create_dir_all(input.join("lifecyclemodels")).unwrap();
    fs::write(
        input.join("lifecyclemodels/sample.json"),
        serde_json::to_vec(&model(true)).unwrap(),
    )
    .unwrap();
    convert_directory(&request(&input, &xml, ConversionDirection::TidasToIlcd)).unwrap();
    let file = xml.join("data/lifecyclemodels/sample.xml");
    let text = fs::read_to_string(&file).unwrap();
    let changed = text.replace(
        "<scalingFactor>2</scalingFactor>",
        "<scalingFactor>3</scalingFactor>",
    );
    assert_ne!(text, changed);
    fs::write(file, changed).unwrap();
    let error = convert_directory(&request(
        &xml.join("data"),
        &back,
        ConversionDirection::IlcdToTidas,
    ))
    .unwrap_err();
    assert!(
        error.to_string().contains("alias projection was modified"),
        "{error}"
    );
    assert!(!back.exists());
}
