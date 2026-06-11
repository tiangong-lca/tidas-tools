import argparse
from collections.abc import Callable
from concurrent.futures import ProcessPoolExecutor
from dataclasses import dataclass
from functools import lru_cache
import importlib.resources as pkg_resources
import json
import logging
import os
import re
import sys
from collections import defaultdict
from pathlib import Path

import fastjsonschema
from jsonschema import Draft7Validator, FormatChecker
from lxml import etree
from referencing import Registry
from referencing.jsonschema import DRAFT7

if __package__:
    from .tidas_log import setup_logging
    from .validation_report import (
        ValidationIssue,
        build_category_report,
        build_package_report,
    )
else:
    from tidas_tools.tidas_log import setup_logging
    from tidas_tools.validation_report import (
        ValidationIssue,
        build_category_report,
        build_package_report,
    )

import tidas_tools.tidas.schemas as schemas
import tidas_tools.eilcd.schemas as eilcd_schemas

CHINESE_CHARACTER_RE = re.compile(r"[\u3400-\u4DBF\u4E00-\u9FFF\uF900-\uFAFF]")
CAS_NUMBER_RE = re.compile(r"^[0-9]{2,7}-[0-9]{2}-[0-9]$")
FORMAT_CHECKER = FormatChecker()
DEFAULT_VALIDATION_JOBS = 1
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


@lru_cache
def tidas_language_codes() -> frozenset[str]:
    schema_file_path = pkg_resources.files(schemas) / "tidas_data_types.json"
    with schema_file_path.open() as schema_file:
        root_schema = json.load(schema_file)
    values = root_schema["$defs"]["Languages"]["enum"]
    return frozenset(str(value) for value in values)


ILCD_SCHEMA_BY_ROOT = {
    ("http://lca.jrc.it/ILCD/Wrapper", "ILCD"): ("wrapper", "ILCD_ILCD.xsd"),
    ("http://lca.jrc.it/ILCD/Contact", "contactDataSet"): (
        "contacts",
        "ILCD_ContactDataSet.xsd",
    ),
    ("http://lca.jrc.it/ILCD/FlowProperty", "flowPropertyDataSet"): (
        "flowproperties",
        "ILCD_FlowPropertyDataSet.xsd",
    ),
    ("http://lca.jrc.it/ILCD/Flow", "flowDataSet"): (
        "flows",
        "ILCD_FlowDataSet.xsd",
    ),
    ("http://lca.jrc.it/ILCD/LCIAMethod", "LCIAMethodDataSet"): (
        "lciamethods",
        "ILCD_LCIAMethodDataSet.xsd",
    ),
    (
        "http://eplca.jrc.ec.europa.eu/ILCD/LifeCycleModel/2017",
        "lifeCycleModelDataSet",
    ): ("lifecyclemodels", "ILCD_LifeCycleModelDataSet.xsd"),
    ("http://lca.jrc.it/ILCD/Process", "processDataSet"): (
        "processes",
        "ILCD_ProcessDataSet.xsd",
    ),
    ("http://lca.jrc.it/ILCD/Source", "sourceDataSet"): (
        "sources",
        "ILCD_SourceDataSet.xsd",
    ),
    ("http://lca.jrc.it/ILCD/UnitGroup", "unitGroupDataSet"): (
        "unitgroups",
        "ILCD_UnitGroupDataSet.xsd",
    ),
    ("http://lca.jrc.it/ILCD/Categories", "CategorySystem"): (
        "categories",
        "ILCD_Categories.xsd",
    ),
    ("http://lca.jrc.it/ILCD/Locations", "ILCDLocations"): (
        "locations",
        "ILCD_Locations.xsd",
    ),
    ("http://lca.jrc.it/ILCD/LCIAMethodologies", "ILCDLCIAMethodologies"): (
        "lciamethodologies",
        "ILCD_LCIAMethodologies.xsd",
    ),
}
_TIDAS_WORKER_CATEGORY: str | None = None
_TIDAS_WORKER_VALIDATOR: "TidasSchemaValidator | None" = None
_ILCD_WORKER_SCHEMA_CACHE: dict[str, etree.XMLSchema] | None = None


@dataclass(frozen=True)
class TidasSchemaValidator:
    strict_validator: Draft7Validator
    fast_validator: Callable[[dict], dict] | None


def is_valid_cas_number(value: str) -> bool:
    """Return True when a CAS number has valid structure and check digit."""
    if not isinstance(value, str) or not CAS_NUMBER_RE.fullmatch(value):
        return False

    body, check_digit = value.rsplit("-", 1)
    digits = body.replace("-", "")
    checksum = sum(
        int(digit) * weight for weight, digit in enumerate(reversed(digits), start=1)
    )
    return checksum % 10 == int(check_digit)


@FORMAT_CHECKER.checks("cas-number")
def _check_cas_number_format(value):
    if not isinstance(value, str):
        return True
    return is_valid_cas_number(value)


def _fast_check_cas_number_format(value):
    if not isinstance(value, str):
        return True
    return is_valid_cas_number(value)


def validate_elementary_flows_classification_hierarchy(class_items):

    errors = []

    for i, item in enumerate(class_items):
        level = int(item["@level"])
        if level != i:
            errors.append(
                f"Elementary flow classification level sorting error: at index {i}, expected level {i}, got {level}"
            )

    for i in range(1, len(class_items)):
        parent_id = class_items[i - 1]["@catId"]
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

        if isinstance(language, str):
            if language not in tidas_language_codes():
                errors.append(
                    f"Localized text error at {location}: @xml:lang '{language}' is not a TIDAS Languages enumeration value"
                )

        if isinstance(language, str) and isinstance(text, str):
            has_chinese = bool(CHINESE_CHARACTER_RE.search(text))

            if language == "zh":
                if not has_chinese:
                    errors.append(
                        f"Localized text error at {location}: @xml:lang '{language}' must include at least one Chinese character"
                    )
            elif language == "en":
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


def _localized_text_issue_code(message: str) -> str:
    if "is not a TIDAS Languages enumeration value" in message:
        return "localized_text_language_not_in_tidas_enum"
    return "localized_text_language_error"


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
            data_set_type = json_item["flowDataSet"]["modellingAndValidation"][
                "LCIMethod"
            ]["typeOfDataSet"]
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


def _collect_strict_schema_issues(
    validator: Draft7Validator, json_item: dict, category: str, file_path: str
) -> list[ValidationIssue]:
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

    return issues


def _collect_schema_issues(
    validator: TidasSchemaValidator, json_item: dict, category: str, file_path: str
) -> list[ValidationIssue]:
    if validator.fast_validator is not None:
        try:
            validator.fast_validator(json_item)
            return []
        except fastjsonschema.JsonSchemaValueException:
            pass
        except fastjsonschema.JsonSchemaException as e:
            logging.debug(
                "fastjsonschema skipped for %s after validation engine error: %s",
                file_path,
                e,
            )
        except Exception as e:
            logging.debug(
                "fastjsonschema skipped for %s after unexpected error: %s",
                file_path,
                e,
            )

    return _collect_strict_schema_issues(
        validator.strict_validator, json_item, category, file_path
    )


def _collect_item_issues(
    validator: TidasSchemaValidator, json_item: dict, category: str, file_path: str
):
    issues = _collect_schema_issues(validator, json_item, category, file_path)

    localized_text_errors = validate_localized_text_language_constraints(json_item)
    for message in localized_text_errors:
        location = "<root>"
        if message.startswith("Localized text error at ") and ":" in message:
            location = message.split("Localized text error at ", 1)[1].split(":", 1)[0]
        issues.append(
            _make_issue(
                issue_code=_localized_text_issue_code(message),
                category=category,
                file_path=file_path,
                location=location,
                message=message,
            )
        )

    issues.extend(_collect_classification_issues(json_item, category, file_path))
    return issues


@lru_cache(maxsize=None)
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


@lru_cache(maxsize=None)
def _load_tidas_schema(schema_filename: str) -> dict:
    schema_file_path = pkg_resources.files(schemas) / schema_filename
    with schema_file_path.open() as schema_file:
        return json.load(schema_file)


def _fastjsonschema_schema_handler(uri: str) -> dict:
    schema_name = Path(uri.split("#", 1)[0]).name
    if not schema_name:
        raise fastjsonschema.JsonSchemaDefinitionException(
            f"Unsupported empty schema reference: {uri}"
        )
    schema = _load_tidas_schema(schema_name)
    return _normalize_fastjsonschema_schema({**schema, "$id": schema_name})


def _normalize_fastjsonschema_schema(node):
    if isinstance(node, dict):
        normalized = {
            key: _normalize_fastjsonschema_schema(value) for key, value in node.items()
        }
        if normalized.get("then") == {}:
            normalized.pop("then")
        if normalized.get("else") == {}:
            normalized.pop("else")
        return normalized
    if isinstance(node, list):
        return [_normalize_fastjsonschema_schema(value) for value in node]
    return node


@lru_cache(maxsize=1)
def _fastjsonschema_handlers() -> dict[str, Callable[[str], dict]]:
    schemas_root = pkg_resources.files(schemas)
    handlers: dict[str, Callable[[str], dict]] = {"": _fastjsonschema_schema_handler}
    for schema_path in schemas_root.iterdir():
        if not schema_path.name.startswith("tidas_") or not schema_path.name.endswith(
            ".json"
        ):
            continue
        handlers[schema_path.name] = _fastjsonschema_schema_handler
        handlers[f"./{schema_path.name}"] = _fastjsonschema_schema_handler
    return handlers


def _compile_fast_tidas_schema_definition(
    schema_id: str, schema_definition: dict
) -> Callable[[dict], dict]:
    schema = _normalize_fastjsonschema_schema({**schema_definition, "$id": schema_id})
    return fastjsonschema.compile(
        schema,
        handlers=_fastjsonschema_handlers(),
        formats={"cas-number": _fast_check_cas_number_format},
        use_default=False,
    )


def _compile_fast_tidas_schema(schema_filename: str) -> Callable[[dict], dict]:
    return _compile_fast_tidas_schema_definition(
        schema_filename, _load_tidas_schema(schema_filename)
    )


@lru_cache(maxsize=1)
def _tidas_schema_registry() -> Registry:
    registry = Registry(retrieve=retrieve_schema)
    schemas_root = pkg_resources.files(schemas)
    schema_files = sorted(
        schema_path
        for schema_path in schemas_root.iterdir()
        if schema_path.name.startswith("tidas_") and schema_path.name.endswith(".json")
    )
    for schema_path in schema_files:
        registry = registry.with_resource(
            f"file://{schema_path}",
            DRAFT7.create_resource(_load_tidas_schema(schema_path.name)),
        )
    return registry.crawl()


def _build_tidas_validator(category: str) -> TidasSchemaValidator:
    schema_file_path = pkg_resources.files(schemas) / f"tidas_{category.lower()}.json"
    schema_uri = f"file://{schema_file_path}"
    strict_schema = {**_load_tidas_schema(schema_file_path.name), "$id": schema_uri}
    strict_validator = Draft7Validator(
        strict_schema,
        registry=_tidas_schema_registry(),
        format_checker=FORMAT_CHECKER,
    )
    try:
        fast_validator = _compile_fast_tidas_schema(schema_file_path.name)
    except Exception as e:
        logging.debug(
            "fastjsonschema compilation skipped for %s: %s",
            schema_file_path.name,
            e,
        )
        fast_validator = None
    return TidasSchemaValidator(strict_validator, fast_validator)


def _collect_tidas_file_issues(
    full_path: str,
    category: str,
    validator: TidasSchemaValidator,
) -> list[ValidationIssue]:
    try:
        with open(full_path, "r") as json_file:
            json_item = json.load(json_file)
    except Exception as e:
        return [
            _make_issue(
                issue_code="invalid_json",
                category=category,
                file_path=full_path,
                message=f"Invalid JSON: {type(e).__name__}: {e}",
            )
        ]

    return _collect_item_issues(validator, json_item, category, full_path)


def _init_tidas_validation_worker(category: str) -> None:
    global _TIDAS_WORKER_CATEGORY, _TIDAS_WORKER_VALIDATOR
    _TIDAS_WORKER_CATEGORY = category
    _TIDAS_WORKER_VALIDATOR = _build_tidas_validator(category)


def _collect_tidas_file_issues_worker(full_path: str) -> list[ValidationIssue]:
    if _TIDAS_WORKER_CATEGORY is None or _TIDAS_WORKER_VALIDATOR is None:
        raise RuntimeError("TIDAS validation worker was not initialized")
    return _collect_tidas_file_issues(
        full_path, _TIDAS_WORKER_CATEGORY, _TIDAS_WORKER_VALIDATOR
    )


def _normalize_jobs(jobs: int | None) -> int:
    if jobs is None:
        return DEFAULT_VALIDATION_JOBS
    if jobs <= 0:
        return os.cpu_count() or 1
    return jobs


def _map_chunksize(item_count: int, jobs: int) -> int:
    if item_count <= 0 or jobs <= 1:
        return 1
    return max(1, min(64, item_count // (jobs * 16) or 1))


def category_validate(
    json_file_path: str,
    category: str,
    emit_logs: bool = True,
    jobs: int | None = DEFAULT_VALIDATION_JOBS,
):
    issues = []
    json_files = [
        os.path.join(json_file_path, filename)
        for filename in sorted(os.listdir(json_file_path))
        if filename.endswith(".json")
    ]
    worker_count = min(_normalize_jobs(jobs), len(json_files)) if json_files else 1

    if worker_count <= 1:
        validator = _build_tidas_validator(category)
        results = (
            (full_path, _collect_tidas_file_issues(full_path, category, validator))
            for full_path in json_files
        )
    else:
        with ProcessPoolExecutor(
            max_workers=worker_count,
            initializer=_init_tidas_validation_worker,
            initargs=(category,),
        ) as executor:
            item_results = executor.map(
                _collect_tidas_file_issues_worker,
                json_files,
                chunksize=_map_chunksize(len(json_files), worker_count),
            )
            results = zip(json_files, item_results)

    for full_path, item_issues in results:
        issues.extend(item_issues)
        if emit_logs:
            if item_issues:
                for issue in item_issues:
                    logging.error(f"ERROR: {full_path} {issue.message}")
            else:
                logging.info(f"INFO: {full_path} PASSED.")

    return build_category_report(category, issues)


def validate_package_dir(
    input_dir: str,
    emit_logs: bool = False,
    jobs: int | None = DEFAULT_VALIDATION_JOBS,
):
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

        category_report = category_validate(
            category_dir, category, emit_logs=emit_logs, jobs=jobs
        )
        category_reports.append(category_report)

    return build_package_report(input_dir, category_reports)


def _split_xml_tag(tag: str) -> tuple[str, str]:
    if tag.startswith("{") and "}" in tag:
        namespace, local_name = tag[1:].split("}", 1)
        return namespace, local_name
    return "", tag


def _iter_ilcd_xml_files(input_dir: str):
    root = Path(input_dir)
    data_root = root / "data"
    search_root = data_root if data_root.is_dir() else root

    for xml_path in sorted(search_root.rglob("*.xml")):
        if not xml_path.is_file():
            continue
        if xml_path.name == "reference_types.xml":
            continue
        if search_root == root:
            relative_parts = xml_path.relative_to(root).parts[:-1]
            if "schemas" in relative_parts or "stylesheets" in relative_parts:
                continue
        yield xml_path


def _build_ilcd_schema(schema_filename: str):
    schema_path = pkg_resources.files(eilcd_schemas) / schema_filename
    schema_doc = etree.parse(str(schema_path))
    return etree.XMLSchema(schema_doc)


def _lxml_error_to_issue(
    *,
    issue_code: str,
    category: str,
    file_path: str,
    error,
) -> ValidationIssue:
    line = getattr(error, "line", None)
    column = getattr(error, "column", None)
    path = getattr(error, "path", None)
    location = path or "<root>"
    if line:
        location = f"{location}:line {line}"
        if column:
            location = f"{location}:column {column}"

    context = {
        "domain": getattr(error, "domain_name", None),
        "level": getattr(error, "level_name", None),
        "type": getattr(error, "type_name", None),
        "line": line,
        "column": column,
    }
    return _make_issue(
        issue_code=issue_code,
        category=category,
        file_path=file_path,
        location=location,
        message=getattr(error, "message", str(error)),
        context={key: value for key, value in context.items() if value is not None},
    )


def _collect_ilcd_cas_number_issues(
    document: etree._ElementTree,
    category: str,
    file_path: str,
) -> list[ValidationIssue]:
    if category != "flows":
        return []

    namespaces = {"flow": "http://lca.jrc.it/ILCD/Flow"}
    issues = []
    for cas_number in document.xpath("//flow:CASNumber", namespaces=namespaces):
        value = cas_number.text or ""
        if not CAS_NUMBER_RE.fullmatch(value) or is_valid_cas_number(value):
            continue

        issues.append(
            _make_issue(
                issue_code="cas_number_checksum_error",
                category=category,
                file_path=file_path,
                location=document.getpath(cas_number),
                message=f"CASNumber '{value}' has an invalid check digit.",
                context={"validator": "cas-number"},
            )
        )

    return issues


def validate_ilcd_file(
    xml_file_path: str | Path,
    schema_cache: dict[str, etree.XMLSchema] | None = None,
):
    schema_cache = schema_cache if schema_cache is not None else {}
    file_path = str(xml_file_path)
    parser = etree.XMLParser(resolve_entities=False, no_network=True)

    try:
        document = etree.parse(file_path, parser)
    except etree.XMLSyntaxError as e:
        issues = [
            _lxml_error_to_issue(
                issue_code="invalid_xml",
                category="ilcd",
                file_path=file_path,
                error=error,
            )
            for error in e.error_log
        ]
        if not issues:
            issues.append(
                _make_issue(
                    issue_code="invalid_xml",
                    category="ilcd",
                    file_path=file_path,
                    message=str(e),
                )
            )
        return "ilcd", issues

    namespace, local_name = _split_xml_tag(document.getroot().tag)
    schema_info = ILCD_SCHEMA_BY_ROOT.get((namespace, local_name))
    if schema_info is None:
        return (
            "ilcd",
            [
                _make_issue(
                    issue_code="unsupported_ilcd_root",
                    category="ilcd",
                    file_path=file_path,
                    location=f"/{local_name}",
                    message=(
                        "Unsupported ILCD XML root element "
                        f"{{{namespace}}}{local_name}"
                    ),
                    context={"namespace": namespace, "local_name": local_name},
                )
            ],
        )

    category, schema_filename = schema_info
    try:
        schema = schema_cache.get(schema_filename)
        if schema is None:
            schema = _build_ilcd_schema(schema_filename)
            schema_cache[schema_filename] = schema
    except etree.XMLSchemaParseError as e:
        return (
            category,
            [
                _lxml_error_to_issue(
                    issue_code="ilcd_schema_load_error",
                    category=category,
                    file_path=file_path,
                    error=error,
                )
                for error in e.error_log
            ],
        )

    schema_issues = []
    if not schema.validate(document):
        schema_issues = [
            _lxml_error_to_issue(
                issue_code="ilcd_schema_error",
                category=category,
                file_path=file_path,
                error=error,
            )
            for error in schema.error_log
        ]

    schema_issues.extend(_collect_ilcd_cas_number_issues(document, category, file_path))
    return category, schema_issues


def _init_ilcd_validation_worker() -> None:
    global _ILCD_WORKER_SCHEMA_CACHE
    _ILCD_WORKER_SCHEMA_CACHE = {}


def _validate_ilcd_file_worker(
    xml_file_path: Path,
) -> tuple[str, list[ValidationIssue]]:
    if _ILCD_WORKER_SCHEMA_CACHE is None:
        raise RuntimeError("ILCD validation worker was not initialized")
    return validate_ilcd_file(xml_file_path, schema_cache=_ILCD_WORKER_SCHEMA_CACHE)


def validate_ilcd_package_dir(
    input_dir: str,
    emit_logs: bool = False,
    jobs: int | None = DEFAULT_VALIDATION_JOBS,
):
    schema_cache: dict[str, etree.XMLSchema] = {}
    issues_by_category: dict[str, list[ValidationIssue]] = defaultdict(list)
    seen_categories = set()
    xml_files = list(_iter_ilcd_xml_files(input_dir))
    worker_count = min(_normalize_jobs(jobs), len(xml_files)) if xml_files else 1

    if worker_count <= 1:
        results = (
            (
                xml_file_path,
                validate_ilcd_file(xml_file_path, schema_cache=schema_cache),
            )
            for xml_file_path in xml_files
        )
    else:
        with ProcessPoolExecutor(
            max_workers=worker_count,
            initializer=_init_ilcd_validation_worker,
        ) as executor:
            item_results = executor.map(
                _validate_ilcd_file_worker,
                xml_files,
                chunksize=_map_chunksize(len(xml_files), worker_count),
            )
            results = zip(xml_files, item_results)

    for xml_file_path, (category, issues) in results:
        seen_categories.add(category)
        issues_by_category[category].extend(issues)

        if emit_logs:
            if issues:
                for issue in issues:
                    logging.error(f"ERROR: {xml_file_path} {issue.message}")
            else:
                logging.info(f"INFO: {xml_file_path} PASSED.")

    category_reports = [
        build_category_report(category, issues_by_category[category])
        for category in sorted(seen_categories)
    ]
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
        "--report-format",
        choices=["text", "json"],
        default="text",
        help=(
            "Validation report output format. This does not change the input "
            "data format. Defaults to text logging."
        ),
    )
    parser.add_argument(
        "--data-format",
        choices=["tidas", "ilcd", "eilcd"],
        default="tidas",
        help="Input data format to validate. Defaults to TIDAS JSON.",
    )
    parser.add_argument(
        "--jobs",
        type=int,
        default=DEFAULT_VALIDATION_JOBS,
        help=(
            "Number of parallel validation worker processes. "
            "Use 0 to use all CPU cores. Defaults to 1."
        ),
    )
    try:
        args = parser.parse_args()

        if not args.input_dir:
            parser.error("the following arguments are required: --input-dir/-i")

        data_format = "ilcd" if args.data_format == "eilcd" else args.data_format
        if args.report_format == "text":
            setup_logging(args.verbose, "validate")
            if data_format == "ilcd":
                report = validate_ilcd_package_dir(
                    args.input_dir, emit_logs=True, jobs=args.jobs
                )
            else:
                report = validate_package_dir(
                    args.input_dir, emit_logs=True, jobs=args.jobs
                )
        else:
            if data_format == "ilcd":
                report = validate_ilcd_package_dir(
                    args.input_dir, emit_logs=False, jobs=args.jobs
                )
            else:
                report = validate_package_dir(
                    args.input_dir, emit_logs=False, jobs=args.jobs
                )
            print(json.dumps(report, indent=2, ensure_ascii=False))

        if not report["ok"]:
            sys.exit(1)
    except Exception as e:
        error_msg = f"Error validating: {e}"
        logging.error(error_msg)
        sys.exit(1)


if __name__ == "__main__":
    main()
