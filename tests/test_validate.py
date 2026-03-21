import importlib.resources as pkg_resources
import json

from jsonschema import Draft7Validator

import tidas_tools.tidas.schemas as schemas
from tidas_tools.validate import (
    category_validate,
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
