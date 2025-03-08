import argparse
import importlib.resources as pkg_resources
import json
import logging
import os
import sys

from jsonschema import validate
from jsonschema.exceptions import ValidationError
from referencing import Registry
from referencing.jsonschema import DRAFT7

import tidas_tools.tidas.schemas as schemas

logging.basicConfig(
    filename="tidas_validate.log",
    level=logging.INFO,
    format="%(asctime)s - %(message)s",
    datefmt="%d-%b-%y %H:%M:%S",
)

GREEN = "\033[92m"
RED = "\033[91m"
RESET = "\033[0m"


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

    level0_to_level1_mapping = {
        "A": ["01", "02", "03"],
        "B": ["05", "06", "07", "08", "09"],
        "C": [f"{i:02d}" for i in range(10, 34)],  # 10 to 33
        "D": ["35"],
        "F": ["41", "42", "43"],
        "G": ["45", "46", "47"],
        "H": ["49", "50", "51", "52", "53"],
        "I": ["55", "56"],
        "J": [f"{i:02d}" for i in range(58, 67)],  # 58 to 66
        "L": ["68"],
        "M": [f"{i:02d}" for i in range(69, 76)],  # 69 to 75
        "N": [f"{i:02d}" for i in range(77, 83)],  # 77 to 82
        "O": ["84"],
        "P": ["85"],
        "Q": ["86", "87", "88"],
        "R": ["90", "91", "92", "93"],
        "S": ["94", "95", "96"],
        "T": ["97", "98"],
        "U": ["99"],
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

    for i, item in enumerate(class_items):
        level = int(item["@level"])
        if level != i:
            errors.append(
                f"Sources classification level sorting error: at index {i}, expected level {i}, got {level}"
            )

    for i in range(1, len(class_items)):
        parent_id = class_items[i - 2]["@classId"]
        child_id = class_items[i]["@classId"]

        if not child_id.startswith(parent_id):
            errors.append(
                f"Sources classification code error: child code '{child_id}' does not start with parent code '{parent_id}'"
            )

    if errors:
        return {"valid": False, "errors": errors}
    else:
        return {"valid": True}


def retrieve_schema(uri):
    """Custom retrieval function for schema references"""
    # Handle both local and remote references
    if not uri.startswith(("http://", "https://")):
        # This is a local reference, load from package
        try:
            # print(f"Trying to resolve local reference: {uri}")
            with pkg_resources.open_text(schemas, uri) as f:
                schema_data = json.load(f)
                # Create a proper resource object
                return DRAFT7.create_resource(schema_data)
        except Exception as e:
            print(f"Failed to resolve: {uri}, error: {e}")
            # Try alternative path resolution methods
            try:
                # Try finding the file directly in the schemas package
                schema_path = pkg_resources.files(schemas) / uri
                with open(schema_path, "r") as f:
                    schema_data = json.load(f)
                    # Create a proper resource object
                    return DRAFT7.create_resource(schema_data)
            except Exception as alt_e:
                print(f"Alternative resolution also failed: {alt_e}")
                raise
    # For remote references, return None to indicate the reference couldn't be resolved
    return None


def category_validate(json_file_path: str, category: str):
    schema_file_path = pkg_resources.files(schemas) / f"tidas_{category.lower()}.json"
    schema_uri = f"file://{schema_file_path}"

    with schema_file_path.open() as f:
        schema = json.load(f)

        # Create registry with our retrieve function
        registry = Registry(retrieve=retrieve_schema)
        # Add the main schema to the registry
        registry = registry.with_resource(schema_uri, DRAFT7.create_resource(schema))

        for filename in os.listdir(json_file_path):
            if filename.endswith(".json"):
                full_path = os.path.join(json_file_path, filename)
                with open(full_path, "r") as json_file:
                    json_item = json.load(json_file)

                    errors = []

                    try:
                        # Add more detailed error handling
                        validate(instance=json_item, schema=schema, registry=registry)
                    except ValidationError as e:
                        errors.append(f"Schema Error: {e.message}")
                    except Exception as e:
                        print(f"Unexpected validation error: {type(e).__name__}: {e}")
                        errors.append(f"Validation error: {e}")

                    if category == "flows":
                        if (
                            json_item["flowDataSet"]["modellingAndValidation"][
                                "LCIMethod"
                            ]["typeOfDataSet"]
                            == "Product flow"
                        ):
                            validation_result = (
                                validate_product_flows_classification_hierarchy(
                                    json_item["flowDataSet"]["flowInformation"][
                                        "dataSetInformation"
                                    ]["classificationInformation"][
                                        "common:classification"
                                    ][
                                        "common:class"
                                    ]
                                )
                            )
                            if not validation_result["valid"]:
                                errors.extend(validation_result["errors"])
                        elif (
                            json_item["flowDataSet"]["modellingAndValidation"][
                                "LCIMethod"
                            ]["typeOfDataSet"]
                            == "Elementary flow"
                        ):
                            validation_result = (
                                validate_elementary_flows_classification_hierarchy(
                                    json_item["flowDataSet"]["flowInformation"][
                                        "dataSetInformation"
                                    ]["classificationInformation"][
                                        "common:elementaryFlowCategorization"
                                    ][
                                        "common:category"
                                    ]
                                )
                            )
                            if not validation_result["valid"]:
                                errors.extend(validation_result["errors"])

                    if category == "processes":
                        validation_result = validate_processes_classification_hierarchy(
                            json_item["processDataSet"]["processInformation"][
                                "dataSetInformation"
                            ]["classificationInformation"]["common:classification"][
                                "common:class"
                            ]
                        )
                        if not validation_result["valid"]:
                            errors.extend(validation_result["errors"])

                    if category == "lifecyclemodels":
                        validation_result = validate_processes_classification_hierarchy(
                            json_item["lifecycleModelDataSet"][
                                "lifecycleModelInformation"
                            ]["dataSetInformation"]["classificationInformation"][
                                "common:classification"
                            ][
                                "common:class"
                            ]
                        )
                        if not validation_result["valid"]:
                            errors.extend(validation_result["errors"])

                    if category == "sources":
                        validation_result = validate_sources_classification_hierarchy(
                            json_item["sourceDataSet"]["sourceInformation"][
                                "dataSetInformation"
                            ]["classificationInformation"]["common:classification"][
                                "common:class"
                            ]
                        )
                        if not validation_result["valid"]:
                            errors.extend(validation_result["errors"])

                    if errors:
                        for err in errors:
                            print(f"{RED}ERROR: {full_path} {err}{RESET}")
                            logging.error(f"ERROR: {full_path} {err}")
                    else:
                        print(f"{GREEN}INFO: {full_path} PASSED.{RESET}")
                        logging.info(f"INFO: {full_path} PASSED.")


def main():
    parser = argparse.ArgumentParser(description="TIDAS format validator.")
    parser.add_argument(
        "--input_dir",
        "-i",
        type=str,
        help="Input directory containing files to process",
    )
    try:
        args = parser.parse_args()
        for category in os.listdir(args.input_dir):
            try:
                if category == "external_docs":
                    continue
                category_dir = os.path.join(args.input_dir, category)
                if os.path.isdir(category_dir):  # Only process directories
                    category_validate(category_dir, category)
            except Exception as e:
                error_msg = f"Error validating category {category}: {e}"
                print(f"{RED}{error_msg}{RESET}", file=sys.stderr)
                logging.error(error_msg)
    except Exception as e:
        error_msg = f"Error validating: {e}"
        print(f"{RED}{error_msg}{RESET}", file=sys.stderr)
        logging.error(error_msg)
        sys.exit(1)


if __name__ == "__main__":
    main()
