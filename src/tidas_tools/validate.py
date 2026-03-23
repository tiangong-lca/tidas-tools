import argparse
import importlib.resources as pkg_resources
import json
import logging
import os
import re
import sys

from jsonschema import Draft7Validator
from referencing import Registry
from referencing.jsonschema import DRAFT7
from .tidas_log import setup_logging
from .validation_report import (
    ValidationIssue,
    build_category_report,
    build_package_report,
)

import tidas_tools.tidas.schemas as schemas

CHINESE_CHARACTER_RE = re.compile(r"[\u3400-\u4DBF\u4E00-\u9FFF\uF900-\uFAFF]")
SUPPORTED_CATEGORIES = [
    "contacts",
    "flowproperties",
    "flows",
    "lciamethods",
    "lifecyclemodels",
    "processes",
    "sources",
    "unitgroups",
]


def validate_elementary_flows_classification_hierarchy(class_items):

    errors = []

    for i, item in enumerate(class_items):
        level = int(item["@level"])
        if level != i:
            errors.append(
                f"Elementary flow classification level sorting error: at index {i}, expected level {i}, got {level}"
            )

    for i in range(1, len(class_items)):
        parent_id = class_items[i - 2]["@catId"]
        child_id = class_items[i]["@catId"]

        if not child_id.startswith(parent_id):
            errors.append(
                f"Elementary flow classification code error: child code '{child_id}' does not start with parent code '{parent_id}'"
            )

    if errors:
        return {"valid": False, "errors": errors}
    else:
        return {"valid": True}


def validate_product_flows_classification_hierarchy(class_items):

    errors = []

    for i, item in enumerate(class_items):
        level = int(item["@level"])
        if level != i:
            errors.append(
                f"Product flow classification level sorting error: at index {i}, expected level {i}, got {level}"
            )

    for i in range(1, len(class_items)):
        parent_id = class_items[i - 1]["@classId"]
        child_id = class_items[i]["@classId"]

        if not child_id.startswith(parent_id):
            errors.append(
                f"Product flow classification code error: child code '{child_id}' does not start with parent code '{parent_id}'"
            )

    if errors:
        return {"valid": False, "errors": errors}
    else:
        return {"valid": True}


def validate_processes_classification_hierarchy(class_items):
    errors = []

    # Derived from tidas_processes_category.json (level 0 -> level 1 codes)
    level0_to_level1_mapping = {
        "A": ["01", "02", "03"],
        "B": ["05", "06", "07", "08", "09"],
        "C": [f"{i:02d}" for i in range(10, 34)],  # 10 to 33
        "D": ["35"],
        "E": ["36", "37", "38", "39"],
        "F": ["41", "42", "43"],
        "G": ["46", "47"],
        "H": ["49", "50", "51", "52", "53"],
        "I": ["55", "56"],
        "J": ["58", "59", "60"],
        "K": ["61", "62", "63"],
        "L": ["64", "65", "66"],
        "M": ["68"],
        "N": ["69", "70", "71", "72", "73", "74", "75"],
        "O": ["77", "78", "79", "80", "81", "82"],
        "P": ["84"],
        "Q": ["85"],
        "R": ["86", "87", "88"],
        "S": ["90", "91", "92", "93"],
        "T": ["94", "95", "96"],
        "U": ["97", "98"],
        "V": ["99"],
    }

    for i, item in enumerate(class_items):
        level = int(item["@level"])
        if level != i:
            errors.append(
                f"Processes classification level sorting error: at index {i}, expected level {i}, got {level}"
            )

    for i in range(1, len(class_items)):
        parent = class_items[i - 1]
        child = class_items[i]

        parent_level = int(parent["@level"])
        child_level = int(child["@level"])

        parent_id = parent["@classId"]
        child_id = child["@classId"]

        if parent_level == 0 and child_level == 1:
            valid_level1_codes = level0_to_level1_mapping.get(parent_id, [])
            if child_id not in valid_level1_codes:
                errors.append(
                    f"Processes classification code error: level 1 code '{child_id}' does not correspond to level 0 code '{parent_id}'"
                )

        else:
            if not child_id.startswith(parent_id):
                errors.append(
                    f"Processes classification code error: child code '{child_id}' does not start with parent code '{parent_id}'"
                )

    if errors:
        return {"valid": False, "errors": errors}
    else:
        return {"valid": True}


def validate_sources_classification_hierarchy(class_items):
    errors = []

    # Normalize to list if the schema provided a single object
    if isinstance(class_items, dict):
        class_items = [class_items]

    # If the data is not a list at this point (e.g. None), treat as empty.
    if not isinstance(class_items, list):
        class_items = []

    for i, item in enumerate(class_items):
        try:
            level = int(item["@level"])
        except (KeyError, TypeError, ValueError):
            errors.append(
                f"Sources classification level parsing error: missing or invalid '@level' at index {i}"
            )
            continue
        if level != i:
            errors.append(
                f"Sources classification level sorting error: at index {i}, expected level {i}, got {level}"
            )

    for i in range(1, len(class_items)):
        parent_id = class_items[i - 1].get("@classId")
        child_id = class_items[i].get("@classId")

        if parent_id is None or child_id is None:
            errors.append(
                f"Sources classification code error: missing '@classId' for parent index {i-1} or child index {i}"
            )
            continue

        if not child_id.startswith(parent_id):
            errors.append(
                f"Sources classification code error: child code '{child_id}' does not start with parent code '{parent_id}'"
            )

    if errors:
        return {"valid": False, "errors": errors}
    else:
        return {"valid": True}


def validate_localized_text_language_constraints(node, path=""):
    errors = []

    if isinstance(node, dict):
        language = node.get("@xml:lang")
        text = node.get("#text")
        location = path or "<root>"

        if isinstance(language, str) and isinstance(text, str):
            normalized_language = language.lower()
            has_chinese = bool(CHINESE_CHARACTER_RE.search(text))

            if normalized_language == "zh" or normalized_language.startswith("zh-"):
                if not has_chinese:
                    errors.append(
                        f"Localized text error at {location}: @xml:lang '{language}' must include at least one Chinese character"
                    )
            elif normalized_language == "en" or normalized_language.startswith("en-"):
                if has_chinese:
                    errors.append(
                        f"Localized text error at {location}: @xml:lang '{language}' must not contain Chinese characters"
                    )

        for key, value in node.items():
            child_path = f"{path}/{key}" if path else key
            errors.extend(
                validate_localized_text_language_constraints(value, child_path)
            )

    elif isinstance(node, list):
        for index, value in enumerate(node):
            child_path = f"{path}/{index}" if path else str(index)
            errors.extend(
                validate_localized_text_language_constraints(value, child_path)
            )

    return errors


def _make_issue(
    *,
    issue_code: str,
    category: str,
    file_path: str,
    message: str,
    location: str = "<root>",
    context: dict | None = None,
) -> ValidationIssue:
    return ValidationIssue(
        issue_code=issue_code,
        severity="error",
        category=category,
        file_path=file_path,
        location=location,
        message=message,
        context=context or {},
    )


def _collect_classification_issues(json_item: dict, category: str, file_path: str):
    issues = []
    try:
        if category == "flows":
            data_set_type = json_item["flowDataSet"]["modellingAndValidation"]["LCIMethod"][
                "typeOfDataSet"
            ]
            if data_set_type == "Product flow":
                validation_result = validate_product_flows_classification_hierarchy(
                    json_item["flowDataSet"]["flowInformation"]["dataSetInformation"][
                        "classificationInformation"
                    ]["common:classification"]["common:class"]
                )
                if not validation_result["valid"]:
                    issues.extend(
                        _make_issue(
                            issue_code="classification_hierarchy_error",
                            category=category,
                            file_path=file_path,
                            message=message,
                        )
                        for message in validation_result["errors"]
                    )
            elif data_set_type == "Elementary flow":
                validation_result = validate_elementary_flows_classification_hierarchy(
                    json_item["flowDataSet"]["flowInformation"]["dataSetInformation"][
                        "classificationInformation"
                    ]["common:elementaryFlowCategorization"]["common:category"]
                )
                if not validation_result["valid"]:
                    issues.extend(
                        _make_issue(
                            issue_code="classification_hierarchy_error",
                            category=category,
                            file_path=file_path,
                            message=message,
                        )
                        for message in validation_result["errors"]
                    )

        elif category == "processes":
            validation_result = validate_processes_classification_hierarchy(
                json_item["processDataSet"]["processInformation"]["dataSetInformation"][
                    "classificationInformation"
                ]["common:classification"]["common:class"]
            )
            if not validation_result["valid"]:
                issues.extend(
                    _make_issue(
                        issue_code="classification_hierarchy_error",
                        category=category,
                        file_path=file_path,
                        message=message,
                    )
                    for message in validation_result["errors"]
                )

        elif category == "lifecyclemodels":
            validation_result = validate_processes_classification_hierarchy(
                json_item["lifecycleModelDataSet"]["lifecycleModelInformation"][
                    "dataSetInformation"
                ]["classificationInformation"]["common:classification"]["common:class"]
            )
            if not validation_result["valid"]:
                issues.extend(
                    _make_issue(
                        issue_code="classification_hierarchy_error",
                        category=category,
                        file_path=file_path,
                        message=message,
                    )
                    for message in validation_result["errors"]
                )

        elif category == "sources":
            validation_result = validate_sources_classification_hierarchy(
                json_item["sourceDataSet"]["sourceInformation"]["dataSetInformation"][
                    "classificationInformation"
                ]["common:classification"]["common:class"]
            )
            if not validation_result["valid"]:
                issues.extend(
                    _make_issue(
                        issue_code="classification_hierarchy_error",
                        category=category,
                        file_path=file_path,
                        message=message,
                    )
                    for message in validation_result["errors"]
                )
    except (KeyError, TypeError, ValueError):
        # Structural schema issues already cover missing hierarchy paths.
        return issues

    return issues


def _collect_item_issues(validator: Draft7Validator, json_item: dict, category: str, file_path: str):
    issues = []
    try:
        for schema_error in validator.iter_errors(json_item):
            location = (
                "/".join(str(part) for part in schema_error.path)
                if schema_error.path
                else "<root>"
            )
            issues.append(
                _make_issue(
                    issue_code="schema_error",
                    category=category,
                    file_path=file_path,
                    location=location,
                    message=f"Schema Error at {location}: {schema_error.message}",
                    context={"validator": schema_error.validator},
                )
            )
    except Exception as e:
        issues.append(
            _make_issue(
                issue_code="validation_error",
                category=category,
                file_path=file_path,
                message=f"Validation error: {e}",
            )
        )

    localized_text_errors = validate_localized_text_language_constraints(json_item)
    for message in localized_text_errors:
        location = "<root>"
        if message.startswith("Localized text error at ") and ":" in message:
            location = message.split("Localized text error at ", 1)[1].split(":", 1)[0]
        issues.append(
            _make_issue(
                issue_code="localized_text_language_error",
                category=category,
                file_path=file_path,
                location=location,
                message=message,
            )
        )

    issues.extend(_collect_classification_issues(json_item, category, file_path))
    return issues


def retrieve_schema(uri):
    """Custom retrieval function for schema references"""
    # Handle both local and remote references
    if not uri.startswith(("http://", "https://")):
        # This is a local reference, load from package
        try:
            # Try finding the file directly in the schemas package
            schema_path = pkg_resources.files(schemas) / uri
            with open(schema_path, "r") as f:
                schema_data = json.load(f)
                # Create a proper resource object
                return DRAFT7.create_resource(schema_data)
        except Exception as e:
            logging.error(f"Failed to resolve: {uri}, error: {e}")
            raise
    # For remote references, return None to indicate the reference couldn't be resolved
    return None


def category_validate(json_file_path: str, category: str, emit_logs: bool = True):
    schema_file_path = pkg_resources.files(schemas) / f"tidas_{category.lower()}.json"
    schema_uri = f"file://{schema_file_path}"
    issues = []

    with schema_file_path.open() as f:
        schema = json.load(f)

        # Create registry with our retrieve function
        registry = Registry(retrieve=retrieve_schema)
        # Add the main schema to the registry
        registry = registry.with_resource(schema_uri, DRAFT7.create_resource(schema))
        # Instantiate validator once per category for efficiency
        validator = Draft7Validator(schema, registry=registry)

        for filename in sorted(os.listdir(json_file_path)):
            if filename.endswith(".json"):
                full_path = os.path.join(json_file_path, filename)
                try:
                    with open(full_path, "r") as json_file:
                        json_item = json.load(json_file)
                except Exception as e:
                    issues.append(
                        _make_issue(
                            issue_code="invalid_json",
                            category=category,
                            file_path=full_path,
                            message=f"Invalid JSON: {type(e).__name__}: {e}",
                        )
                    )
                    continue

                item_issues = _collect_item_issues(validator, json_item, category, full_path)
                issues.extend(item_issues)

                if emit_logs:
                    if item_issues:
                        for issue in item_issues:
                            logging.error(f"ERROR: {full_path} {issue.message}")
                    else:
                        logging.info(f"INFO: {full_path} PASSED.")

    return build_category_report(category, issues)


def validate_package_dir(input_dir: str, emit_logs: bool = False):
    schemas_root = pkg_resources.files(schemas)
    category_reports = []
    for category in SUPPORTED_CATEGORIES:
        category_dir = os.path.join(input_dir, category)
        if not os.path.isdir(category_dir):
            if emit_logs:
                logging.debug("Skipping missing directory %s", category_dir)
            continue

        schema_filename = f"tidas_{category.lower()}.json"
        schema_path = schemas_root / schema_filename
        if not schema_path.is_file():
            if emit_logs:
                logging.debug(
                    "Skipping directory %s — no schema file %s",
                    category_dir,
                    schema_filename,
                )
            continue

        category_report = category_validate(category_dir, category, emit_logs=emit_logs)
        category_reports.append(category_report)

    return build_package_report(input_dir, category_reports)


def main():
    parser = argparse.ArgumentParser(description="TIDAS format validator.")
    parser.add_argument(
        "--input-dir",
        "-i",
        type=str,
        help="Input directory containing files to validate",
    )
    parser.add_argument(
        "--verbose", "-v", action="store_true", help="Enable verbose logging"
    )
    parser.add_argument(
        "--format",
        choices=["text", "json"],
        default="text",
        help="Output format. Defaults to text logging.",
    )
    try:
        args = parser.parse_args()

        if args.format == "text":
            setup_logging(args.verbose, "validate")
            report = validate_package_dir(args.input_dir, emit_logs=True)
        else:
            report = validate_package_dir(args.input_dir, emit_logs=False)
            print(json.dumps(report, indent=2, ensure_ascii=False))

        if not report["ok"]:
            sys.exit(1)
    except Exception as e:
        error_msg = f"Error validating: {e}"
        logging.error(error_msg)
        sys.exit(1)


if __name__ == "__main__":
    main()
