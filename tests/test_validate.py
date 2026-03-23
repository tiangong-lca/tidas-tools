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
