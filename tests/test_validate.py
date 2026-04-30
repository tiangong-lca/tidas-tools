import importlib.resources as pkg_resources
import json

from jsonschema import Draft7Validator

import tidas_tools.tidas.schemas as schemas
from tidas_tools.validate import (
    category_validate,
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
        }
    )


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
