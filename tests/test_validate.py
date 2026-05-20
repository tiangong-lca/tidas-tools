import importlib.resources as pkg_resources
import json

from jsonschema import Draft7Validator
from lxml import etree
from referencing import Registry

import tidas_tools.eilcd.stylesheets as eilcd_stylesheets
import tidas_tools.tidas.schemas as schemas
from tidas_tools.validate import (
    FORMAT_CHECKER,
    _collect_ilcd_cas_number_issues,
    category_validate,
    is_valid_cas_number,
    retrieve_schema,
    validate_ilcd_package_dir,
    validate_package_dir,
    validate_localized_text_language_constraints,
)


def build_data_type_validator(definition_name):
    schema_file_path = pkg_resources.files(schemas) / "tidas_data_types.json"
    with schema_file_path.open() as schema_file:
        root_schema = json.load(schema_file)

    return Draft7Validator(
        {
            "$schema": root_schema["$schema"],
            "$defs": root_schema["$defs"],
            **root_schema["$defs"][definition_name],
        },
        format_checker=FORMAT_CHECKER,
    )


def load_tidas_schema(schema_name):
    schema_file_path = pkg_resources.files(schemas) / schema_name
    with schema_file_path.open() as schema_file:
        return json.load(schema_file)


def build_tidas_schema_fragment_validator(schema_fragment):
    return Draft7Validator(
        schema_fragment,
        registry=Registry(retrieve=retrieve_schema),
        format_checker=FORMAT_CHECKER,
    )


def process_exchange_location_schema():
    schema = load_tidas_schema("tidas_processes.json")
    return schema["properties"]["processDataSet"]["properties"]["exchanges"][
        "properties"
    ]["exchange"]["items"]["properties"]["location"]


def test_process_supply_volume_schema_contract():
    schema = load_tidas_schema("tidas_processes.json")
    data_sources_schema = schema["properties"]["processDataSet"]["properties"][
        "modellingAndValidation"
    ]["properties"]["dataSourcesTreatmentAndRepresentativeness"]

    properties = data_sources_schema["properties"]

    assert properties["annualSupplyOrProductionVolume"]["$ref"] == (
        "tidas_data_types.json#/$defs/AnnualSupplyOrProductionVolumeMultiLang"
    )
    assert properties["percentageSupplyOrProductionCovered"]["$ref"] == (
        "tidas_data_types.json#/$defs/Perc"
    )
    assert "annualSupplyOrProductionVolume" in data_sources_schema["required"]


def test_process_exchange_location_schema_prefers_location_codes_but_keeps_string_compatibility():
    location_schema = process_exchange_location_schema()

    assert location_schema["anyOf"] == [
        {"$ref": "tidas_locations_category.json"},
        {"$ref": "tidas_data_types.json#/$defs/String"},
    ]
    assert "StringMultiLang" not in json.dumps(location_schema)


def test_process_exchange_location_accepts_location_code_and_legacy_string():
    validator = build_tidas_schema_fragment_validator(
        process_exchange_location_schema()
    )

    assert list(validator.iter_errors("CN")) == []
    assert list(validator.iter_errors("GLO")) == []
    assert list(validator.iter_errors("Legacy plant area")) == []


def test_process_exchange_location_rejects_empty_and_multilang_shapes():
    validator = build_tidas_schema_fragment_validator(
        process_exchange_location_schema()
    )

    assert list(validator.iter_errors(""))
    assert list(validator.iter_errors({"@xml:lang": "en", "#text": "CN"}))
    assert list(validator.iter_errors([{"@xml:lang": "en", "#text": "CN"}]))


def test_elementary_flow_classification_hierarchy_checks_adjacent_parent():
    from tidas_tools.validate import validate_elementary_flows_classification_hierarchy

    result = validate_elementary_flows_classification_hierarchy(
        [
            {"@level": "0", "@catId": "1", "#text": "Emissions"},
            {"@level": "1", "@catId": "1.3", "#text": "Emissions to air"},
            {
                "@level": "2",
                "@catId": "1.3.4",
                "#text": "Emissions to air, unspecified",
            },
        ]
    )

    assert result == {"valid": True}


def test_cas_number_checksum_validator():
    assert is_valid_cas_number("64-17-5")
    assert is_valid_cas_number("7732-18-5")
    assert is_valid_cas_number("007732-18-5")
    assert is_valid_cas_number("50-00-0")

    assert not is_valid_cas_number("64-17-6")
    assert not is_valid_cas_number("7732-18-4")
    assert not is_valid_cas_number("")
    assert not is_valid_cas_number("2023600")


def test_tidas_cas_number_schema_enforces_checksum_format():
    validator = build_data_type_validator("CASNumber")

    assert list(validator.iter_errors("64-17-5")) == []

    errors = list(validator.iter_errors("64-17-6"))

    assert [error.validator for error in errors] == ["format"]


def test_ilcd_cas_number_supplemental_validation_enforces_checksum():
    document = etree.ElementTree(etree.fromstring("""
            <flowDataSet xmlns="http://lca.jrc.it/ILCD/Flow">
              <flowInformation>
                <dataSetInformation>
                  <CASNumber>64-17-6</CASNumber>
                </dataSetInformation>
              </flowInformation>
            </flowDataSet>
            """))

    issues = _collect_ilcd_cas_number_issues(document, "flows", "flow.xml")

    assert len(issues) == 1
    assert issues[0].issue_code == "cas_number_checksum_error"
    assert "invalid check digit" in issues[0].message


def test_ilcd_cas_number_supplemental_validation_defers_structure_errors_to_xsd():
    document = etree.ElementTree(etree.fromstring("""
            <flowDataSet xmlns="http://lca.jrc.it/ILCD/Flow">
              <flowInformation>
                <dataSetInformation>
                  <CASNumber>2023600</CASNumber>
                  <CASNumber> 64-17-6 </CASNumber>
                  <CASNumber />
                </dataSetInformation>
              </flowInformation>
            </flowDataSet>
            """))

    assert _collect_ilcd_cas_number_issues(document, "flows", "flow.xml") == []


def test_annual_supply_volume_multilang_accepts_numeric_text_with_suffix():
    validator = build_data_type_validator("AnnualSupplyOrProductionVolumeMultiLang")

    assert (
        list(validator.iter_errors({"@xml:lang": "en", "#text": "123 kg/year"})) == []
    )
    assert (
        list(
            validator.iter_errors(
                [
                    {"@xml:lang": "en", "#text": "1.23E+4 kg/year"},
                    {"@xml:lang": "zh-CN", "#text": "123 千克/年"},
                ]
            )
        )
        == []
    )


def test_annual_supply_volume_multilang_rejects_missing_suffix():
    validator = build_data_type_validator("AnnualSupplyOrProductionVolumeMultiLang")

    assert list(validator.iter_errors({"@xml:lang": "en", "#text": "123"}))
    assert list(validator.iter_errors({"@xml:lang": "en", "#text": "123 "}))


def test_annual_supply_volume_multilang_rejects_non_numeric_prefix():
    validator = build_data_type_validator("AnnualSupplyOrProductionVolumeMultiLang")

    assert list(validator.iter_errors({"@xml:lang": "en", "#text": "abc kg/year"}))


def test_annual_supply_volume_multilang_keeps_localized_language_rules():
    validator = build_data_type_validator("AnnualSupplyOrProductionVolumeMultiLang")

    assert list(validator.iter_errors({"@xml:lang": "en", "#text": "123 千克/年"}))


def test_category_validate(tmp_path):
    sources_dir = tmp_path / "sources"
    sources_dir.mkdir()

    category_validate(str(sources_dir), "sources")


def test_category_validate_returns_structured_issues(tmp_path):
    sources_dir = tmp_path / "sources"
    sources_dir.mkdir()
    (sources_dir / "bad.json").write_text("{}", encoding="utf-8")

    report = category_validate(str(sources_dir), "sources")

    assert report["category"] == "sources"
    assert report["summary"]["issue_count"] >= 1
    first_issue = report["issues"][0]
    assert first_issue["issue_code"] in {
        "schema_error",
        "validation_error",
        "localized_text_language_error",
        "classification_hierarchy_error",
    }
    assert first_issue["severity"] == "error"
    assert first_issue["category"] == "sources"
    assert first_issue["file_path"].endswith("bad.json")


def test_category_validate_avoids_cascading_classification_errors(tmp_path):
    sources_dir = tmp_path / "sources"
    sources_dir.mkdir()
    (sources_dir / "bad.json").write_text("{}", encoding="utf-8")

    report = category_validate(str(sources_dir), "sources")

    assert [issue["issue_code"] for issue in report["issues"]] == ["schema_error"]


def test_validate_package_dir_returns_structured_report(tmp_path):
    package_dir = tmp_path / "package"
    sources_dir = package_dir / "sources"
    sources_dir.mkdir(parents=True)
    (sources_dir / "bad.json").write_text("{}", encoding="utf-8")

    report = validate_package_dir(str(package_dir))

    assert report["ok"] is False
    assert report["summary"]["issue_count"] >= 1
    assert report["summary"]["error_count"] >= 1
    assert report["issues"][0]["category"] == "sources"


def test_validate_ilcd_package_dir_accepts_valid_location_xml(tmp_path):
    source_path = pkg_resources.files(eilcd_stylesheets) / "ILCDLocations_Reference.xml"
    xml_path = tmp_path / "ILCDLocations.xml"
    xml_path.write_text(source_path.read_text(encoding="utf-8"), encoding="utf-8")

    report = validate_ilcd_package_dir(str(tmp_path))

    assert report["ok"] is True
    assert report["summary"]["category_count"] == 1
    assert report["categories"][0]["category"] == "locations"


def test_validate_ilcd_package_dir_returns_schema_errors(tmp_path):
    xml_path = tmp_path / "bad-locations.xml"
    xml_path.write_text(
        '<ILCDLocations xmlns="http://lca.jrc.it/ILCD/Locations"/>',
        encoding="utf-8",
    )

    report = validate_ilcd_package_dir(str(tmp_path))

    assert report["ok"] is False
    assert report["summary"]["issue_count"] == 1
    issue = report["issues"][0]
    assert issue["issue_code"] == "ilcd_schema_error"
    assert issue["category"] == "locations"
    assert issue["file_path"].endswith("bad-locations.xml")


def test_validate_ilcd_package_dir_rejects_unknown_xml_roots(tmp_path):
    xml_path = tmp_path / "unknown.xml"
    xml_path.write_text("<custom />", encoding="utf-8")

    report = validate_ilcd_package_dir(str(tmp_path))

    assert report["ok"] is False
    assert report["issues"][0]["issue_code"] == "unsupported_ilcd_root"


def test_validate_ilcd_package_dir_skips_packaged_stylesheet_helpers(tmp_path):
    data_dir = tmp_path / "data"
    stylesheets_dir = tmp_path / "stylesheets"
    data_dir.mkdir()
    stylesheets_dir.mkdir()
    (stylesheets_dir / "reference_types.xml").write_text(
        "<referenceTypes />",
        encoding="utf-8",
    )

    report = validate_ilcd_package_dir(str(tmp_path))

    assert report["ok"] is True
    assert report["summary"]["category_count"] == 0


def test_string_multilang_schema_requires_chinese_for_zh():
    validator = build_data_type_validator("StringMultiLang")

    errors = list(
        validator.iter_errors({"@xml:lang": "zh-CN", "#text": "english only"})
    )

    assert errors


def test_string_multilang_schema_rejects_chinese_for_en():
    validator = build_data_type_validator("StringMultiLang")

    errors = list(validator.iter_errors({"@xml:lang": "en", "#text": "English 中文"}))

    assert errors


def test_string_multilang_schema_accepts_valid_localized_text():
    validator = build_data_type_validator("StringMultiLang")

    errors = list(
        validator.iter_errors(
            [
                {"@xml:lang": "zh", "#text": "中文名称"},
                {"@xml:lang": "en-US", "#text": "English title"},
                {"@xml:lang": "fr", "#text": "Bonjour 中文"},
            ]
        )
    )

    assert errors == []


def test_common_other_schema_accepts_non_common_extension_elements():
    validator = build_data_type_validator("CommonOther")

    errors = list(
        validator.iter_errors(
            {
                "@xmlns:ecn": "http://echa.europa.eu/schema/ecn",
                "ecn:ECNumber": "600-786-0",
                "customElement": {
                    "@unit": "kg",
                    "#text": "1.0",
                },
            }
        )
    )

    assert errors == []


def test_common_other_schema_rejects_legacy_string_payload():
    validator = build_data_type_validator("CommonOther")

    errors = list(validator.iter_errors("legacy extension text"))

    assert errors


def test_common_other_schema_requires_extension_content():
    validator = build_data_type_validator("CommonOther")

    assert list(validator.iter_errors({}))
    assert list(
        validator.iter_errors({"@xmlns:ecn": "http://echa.europa.eu/schema/ecn"})
    )


def test_common_other_schema_rejects_common_namespace_children():
    validator = build_data_type_validator("CommonOther")

    errors = list(validator.iter_errors({"common:note": "not allowed here"}))

    assert errors


def test_common_other_schema_references_common_other_definition():
    schema_dir = pkg_resources.files(schemas)
    expected = {"$ref": "tidas_data_types.json#/$defs/CommonOther"}
    mismatches = []

    for schema_file_path in sorted(
        item
        for item in schema_dir.iterdir()
        if item.name.startswith("tidas_") and item.name.endswith(".json")
    ):
        if schema_file_path.name in {
            "tidas_data_types.json",
            "tidas_flows_elementary_category.json",
            "tidas_flows_product_category.json",
            "tidas_processes_category.json",
        }:
            continue

        schema = json.loads(schema_file_path.read_text(encoding="utf-8"))

        def walk(node, path=""):
            if isinstance(node, dict):
                for key, value in node.items():
                    child_path = f"{path}/{key}" if path else key
                    if key == "common:other" and value != expected:
                        mismatches.append(f"{schema_file_path.name}:{child_path}")
                    walk(value, child_path)
            elif isinstance(node, list):
                for index, value in enumerate(node):
                    walk(value, f"{path}/{index}")

        walk(schema)

    assert mismatches == []


def test_validate_localized_text_language_constraints_reports_nested_paths():
    payload = {
        "processDataSet": {
            "processInformation": {
                "dataSetInformation": {
                    "name": {
                        "baseName": [
                            {"@xml:lang": "zh-CN", "#text": "english only"},
                            {"@xml:lang": "en", "#text": "English 中文"},
                        ]
                    }
                }
            }
        }
    }

    errors = validate_localized_text_language_constraints(payload)

    assert errors == [
        "Localized text error at processDataSet/processInformation/dataSetInformation/name/baseName/0: @xml:lang 'zh-CN' must include at least one Chinese character",
        "Localized text error at processDataSet/processInformation/dataSetInformation/name/baseName/1: @xml:lang 'en' must not contain Chinese characters",
    ]
