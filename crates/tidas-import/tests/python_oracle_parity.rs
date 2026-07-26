use std::fmt::Write as _;
use std::fs;
use std::fs::File;
use std::io::Write as _;
use std::path::{Path, PathBuf};

use serde_json::Value;
use tempfile::tempdir;
use tidas_import::{ImportRequest, ImportTarget, SourceFormat, run_import};
use tidas_runtime::{CancellationToken, MemoryBudget};
use zip::write::SimpleFileOptions;

const SOURCE: &[u8] = include_bytes!("fixtures/python-oracle-v1/simapro.csv");
const EXPECTED: &str = include_str!("fixtures/python-oracle-v1/simapro.expected.json");

#[test]
fn frozen_python_simapro_oracle_matches_complete_rust_import() {
    let directory = tempdir().unwrap();
    let source = directory.path().join("simapro.csv");
    fs::write(&source, SOURCE).unwrap();
    let output = directory.path().join("output");
    let first = run_fixture(&source, &output);
    let first_mapping = fs::read(output.join("mapping.csv.gz")).unwrap();
    let first_semantics = emitted_semantics(&output);
    let second = run_fixture(&source, &output);
    let second_mapping = fs::read(output.join("mapping.csv.gz")).unwrap();
    let second_semantics = emitted_semantics(&output);
    let relocated_directory = tempdir().unwrap();
    let relocated_source = relocated_directory.path().join("simapro.csv");
    let relocated_output = relocated_directory.path().join("output");
    fs::write(&relocated_source, SOURCE).unwrap();
    let relocated = run_fixture(&relocated_source, &relocated_output);
    let expected: Value = serde_json::from_str(EXPECTED).unwrap();

    assert_eq!(first.detected_format.as_str(), expected["source_format"]);
    assert_eq!(
        serde_json::to_value(&first.object_counts).unwrap(),
        expected["object_counts"]
    );
    assert_eq!(first.warning_count, expected["warning_count"]);
    assert_eq!(first.error_count, expected["error_count"]);
    assert_eq!(first.tidas_validation_issue_count, 0);
    assert_eq!(first.ilcd_validation_issue_count, Some(0));
    assert_eq!(first_semantics, expected["process"]);
    assert_eq!(second_semantics, first_semantics);
    assert_repeated_outputs_match(&first, &second, &first_mapping, &second_mapping);
    assert_relocated_outputs_match(&first, &relocated, &first_mapping, &relocated_output);

    let generated = &expected["generated_unit"];
    assert!(
        output
            .join("tidas/unitgroups")
            .join(json_name(generated, "unit_group_id"))
            .is_file()
    );
    assert!(
        output
            .join("tidas/flowproperties")
            .join(json_name(generated, "flow_property_id"))
            .is_file()
    );
    for (category, key) in [
        ("contacts", "contact_id"),
        ("sources", "format_source_id"),
        ("sources", "compliance_source_id"),
    ] {
        assert!(
            output
                .join("tidas")
                .join(category)
                .join(json_name(&expected["fixed_dependencies"], key))
                .is_file()
        );
    }
    assert!(output.join("process-bundles/index.json").is_file());
}

#[test]
fn frozen_python_semantic_matrix_matches_all_native_adapters() {
    let fixture_root =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/python-oracle-v1");
    for fixture_name in [
        "ecospold1.expected.json",
        "ecospold2.expected.json",
        "openlca.expected.json",
        "openlca-xlsx.expected.json",
        "ilcd.expected.json",
    ] {
        eprintln!("running frozen oracle fixture {fixture_name}");
        let expected: Value =
            serde_json::from_slice(&fs::read(fixture_root.join(fixture_name)).unwrap()).unwrap();
        assert_eq!(
            expected["oracle"],
            "Python tidas_tools.import_lca captured before the feature freeze"
        );
        let directory = tempdir().unwrap();
        let source = if expected["source"] == "generated:openlca-process-xlsx" {
            let source = directory.path().join("process.xlsx");
            write_xlsx_fixture(&source);
            source
        } else {
            fixture_root.join(expected["source"].as_str().unwrap())
        };
        let output = directory.path().join("output");
        let report = run_matrix_fixture(
            &source,
            &output,
            source_format(expected["source_format"].as_str().unwrap()),
        );
        let repeated = run_matrix_fixture(
            &source,
            &output,
            source_format(expected["source_format"].as_str().unwrap()),
        );
        assert_eq!(report.detected_format.as_str(), expected["source_format"]);
        assert_eq!(report.error_count, 0, "{fixture_name}");
        assert!(report.warning_count > 0, "{fixture_name}");
        assert_eq!(report.tidas_validation_issue_count, 0, "{fixture_name}");
        assert_eq!(
            report.ilcd_validation_issue_count,
            Some(0),
            "{fixture_name}"
        );
        assert_eq!(
            report.tidas_package.output_tree_sha256, repeated.tidas_package.output_tree_sha256,
            "{fixture_name}"
        );
        assert_eq!(
            report.ilcd_conversion.as_ref().unwrap().output_tree_sha256,
            repeated
                .ilcd_conversion
                .as_ref()
                .unwrap()
                .output_tree_sha256,
            "{fixture_name}"
        );
        for assertion in expected["assertions"].as_array().unwrap() {
            assert_golden_value(&output, assertion, fixture_name);
        }
    }
}

fn run_matrix_fixture(
    source: &Path,
    output: &Path,
    requested_format: SourceFormat,
) -> tidas_import::ImportExecutionReportV1 {
    run_import(&ImportRequest {
        source: source.to_path_buf(),
        requested_format: Some(requested_format),
        output_dir: output.to_path_buf(),
        target: ImportTarget::Both,
        write_mapping: false,
        write_process_bundles: false,
        cancellation: CancellationToken::default(),
        memory_budget: MemoryBudget::new(32 * 1024 * 1024),
        queue_capacity: 2,
        max_entry_bytes: 1024 * 1024,
        max_issue_bytes: 64 * 1024,
    })
    .unwrap()
}

fn source_format(value: &str) -> SourceFormat {
    match value {
        "ecospold1" => SourceFormat::Ecospold1,
        "ecospold2" => SourceFormat::Ecospold2,
        "openlca-jsonld" => SourceFormat::OpenlcaJsonld,
        "openlca-process-xlsx" => SourceFormat::OpenlcaProcessXlsx,
        "ilcd" => SourceFormat::Ilcd,
        _ => panic!("unknown frozen oracle format {value}"),
    }
}

fn write_xlsx_fixture(path: &Path) {
    let sheets = [
        (
            "General information",
            sheet(&[
                &["General information"],
                &["UUID", "22222222-2222-4222-8222-222222222222"],
                &["Name", "XLSX test process"],
                &["Description", "Frozen Python workbook fixture"],
            ]),
        ),
        (
            "Flows",
            sheet(&[
                &["UUID", "Name", "Category", "Type"],
                &[
                    "11111111-1111-4111-8111-111111111111",
                    "test product",
                    "products",
                    "Product flow",
                ],
                &[
                    "33333333-3333-4333-8333-333333333333",
                    "test input",
                    "materials",
                    "Product flow",
                ],
                &[
                    "44444444-4444-4444-8444-444444444444",
                    "carbon dioxide",
                    "air",
                    "Elementary flow",
                ],
            ]),
        ),
        (
            "Outputs",
            sheet(&[
                &["Flow", "Category", "Amount", "Unit", "Is reference?"],
                &["test product", "products", "1", "kg", "true"],
                &["carbon dioxide", "air", "1.5", "kg", "false"],
            ]),
        ),
        (
            "Inputs",
            sheet(&[
                &["Flow", "Category", "Amount", "Unit"],
                &["test input", "materials", "0.2", "kg"],
            ]),
        ),
    ];
    let mut archive = zip::ZipWriter::new(File::create(path).unwrap());
    let options = SimpleFileOptions::default();
    write_zip_entry(
        &mut archive,
        options,
        "[Content_Types].xml",
        content_types(),
    );
    write_zip_entry(&mut archive, options, "xl/workbook.xml", &workbook(&sheets));
    write_zip_entry(
        &mut archive,
        options,
        "xl/_rels/workbook.xml.rels",
        &workbook_relationships(sheets.len()),
    );
    for (index, (_, xml)) in sheets.iter().enumerate() {
        write_zip_entry(
            &mut archive,
            options,
            &format!("xl/worksheets/sheet{}.xml", index + 1),
            xml,
        );
    }
    archive.finish().unwrap();
}

fn write_zip_entry(
    archive: &mut zip::ZipWriter<File>,
    options: SimpleFileOptions,
    name: &str,
    content: &str,
) {
    archive.start_file(name, options).unwrap();
    archive.write_all(content.as_bytes()).unwrap();
}

fn content_types() -> &'static str {
    r#"<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/></Types>"#
}

fn workbook(sheets: &[(&str, String)]) -> String {
    let mut items = String::new();
    for (index, (name, _)) in sheets.iter().enumerate() {
        write!(
            items,
            r#"<sheet name="{name}" sheetId="{}" r:id="rId{}"/>"#,
            index + 1,
            index + 1
        )
        .unwrap();
    }
    format!(
        r#"<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets>{items}</sheets></workbook>"#
    )
}

fn workbook_relationships(count: usize) -> String {
    let mut items = String::new();
    for index in 1..=count {
        write!(
            items,
            r#"<Relationship Id="rId{index}" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet{index}.xml"/>"#
        )
        .unwrap();
    }
    format!(
        r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">{items}</Relationships>"#
    )
}

fn sheet(rows: &[&[&str]]) -> String {
    let mut body = String::new();
    for (row_index, row) in rows.iter().enumerate() {
        body.push_str("<row>");
        for (column, value) in row.iter().enumerate() {
            write!(
                body,
                r#"<c r="{}{}" t="inlineStr"><is><t>{value}</t></is></c>"#,
                column_name(column),
                row_index + 1
            )
            .unwrap();
        }
        body.push_str("</row>");
    }
    format!("<worksheet><sheetData>{body}</sheetData></worksheet>")
}

fn column_name(mut index: usize) -> String {
    index = index.saturating_add(1);
    let mut output = String::new();
    while index > 0 {
        let remainder = (index - 1) % 26;
        output.insert(
            0,
            char::from_u32(u32::try_from(remainder + 65).unwrap()).unwrap(),
        );
        index = (index - 1) / 26;
    }
    output
}

fn assert_golden_value(output: &Path, assertion: &Value, fixture_name: &str) {
    let category = assertion["category"].as_str().unwrap();
    let id = assertion["id"].as_str().unwrap();
    let document: Value = serde_json::from_slice(
        &fs::read(
            output
                .join("tidas")
                .join(category)
                .join(format!("{id}.json")),
        )
        .unwrap(),
    )
    .unwrap();
    let pointer = assertion["pointer"].as_str().unwrap();
    assert_eq!(
        document.pointer(pointer),
        Some(&assertion["equals"]),
        "{fixture_name}: {category}/{id}.json{pointer}"
    );
}

fn run_fixture(source: &Path, output: &Path) -> tidas_import::ImportExecutionReportV1 {
    run_import(&ImportRequest {
        source: source.to_path_buf(),
        requested_format: None,
        output_dir: output.to_path_buf(),
        target: ImportTarget::Both,
        write_mapping: true,
        write_process_bundles: true,
        cancellation: CancellationToken::default(),
        memory_budget: MemoryBudget::new(32 * 1024 * 1024),
        queue_capacity: 2,
        max_entry_bytes: 1024 * 1024,
        max_issue_bytes: 64 * 1024,
    })
    .unwrap()
}

fn assert_repeated_outputs_match(
    first: &tidas_import::ImportExecutionReportV1,
    second: &tidas_import::ImportExecutionReportV1,
    first_mapping: &[u8],
    second_mapping: &[u8],
) {
    assert_eq!(
        first.tidas_package.output_tree_sha256,
        second.tidas_package.output_tree_sha256
    );
    assert_eq!(
        first.ilcd_conversion.as_ref().unwrap().output_tree_sha256,
        second.ilcd_conversion.as_ref().unwrap().output_tree_sha256
    );
    assert_eq!(
        first.mapping.as_ref().unwrap().output_sha256,
        second.mapping.as_ref().unwrap().output_sha256
    );
    assert_eq!(first_mapping, second_mapping);
}

fn assert_relocated_outputs_match(
    first: &tidas_import::ImportExecutionReportV1,
    relocated: &tidas_import::ImportExecutionReportV1,
    first_mapping: &[u8],
    relocated_output: &Path,
) {
    assert_eq!(
        first.tidas_package.output_tree_sha256,
        relocated.tidas_package.output_tree_sha256
    );
    assert_eq!(
        first.ilcd_conversion.as_ref().unwrap().output_tree_sha256,
        relocated
            .ilcd_conversion
            .as_ref()
            .unwrap()
            .output_tree_sha256
    );
    assert_eq!(
        first.mapping.as_ref().unwrap().output_sha256,
        relocated.mapping.as_ref().unwrap().output_sha256
    );
    assert_eq!(
        first_mapping,
        fs::read(relocated_output.join("mapping.csv.gz")).unwrap()
    );
}

fn json_name(value: &Value, key: &str) -> String {
    format!("{}.json", value[key].as_str().unwrap())
}

fn emitted_semantics(output: &Path) -> Value {
    let process_path = only_json(output.join("tidas/processes"));
    let process: Value = serde_json::from_slice(&fs::read(process_path).unwrap()).unwrap();
    let dataset = &process["processDataSet"];
    let exchanges = dataset["exchanges"]["exchange"]
        .as_array()
        .unwrap()
        .iter()
        .map(|exchange| {
            serde_json::json!({
                "flow": exchange["referenceToFlowDataSet"]["common:shortDescription"]["#text"],
                "direction": exchange["exchangeDirection"],
                "amount": exchange["meanAmount"],
            })
        })
        .collect::<Vec<_>>();
    serde_json::json!({
        "name": dataset["processInformation"]["dataSetInformation"]["name"]["baseName"]["#text"],
        "functional_unit": dataset["processInformation"]["quantitativeReference"]["functionalUnitOrOther"]["#text"],
        "exchanges": exchanges,
    })
}

fn only_json(directory: PathBuf) -> PathBuf {
    let mut files = fs::read_dir(directory)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "json")
        })
        .collect::<Vec<_>>();
    files.sort();
    assert_eq!(files.len(), 1);
    files.pop().unwrap()
}
