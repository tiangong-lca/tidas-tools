import importlib.resources as pkg_resources
import json
import os
import logging

from jsonschema import validate
from jsonschema.exceptions import ValidationError

import tidas_tools.schemas as schemas

logging.basicConfig(
    filename="tidas_validate.log",
    level=logging.INFO,
    format="%(asctime)s - %(message)s",
    datefmt="%d-%b-%y %H:%M:%S",
)

GREEN = "\033[92m"
RED = "\033[91m"
RESET = "\033[0m"


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
        "U": ["99"]
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
        
        parent_id = parent["@catId"]
        child_id = child["@catId"]
        
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

def category_validate(json_file_path: str, category: str):

    with pkg_resources.open_text(schemas, f"tidas_{category.lower()}.json") as f:
        schema = json.load(f)

        for filename in os.listdir(json_file_path):
            if filename.endswith(".json"):
                full_path = os.path.join(json_file_path, filename)
                with open(full_path, "r") as json_file:
                    json_item = json.load(json_file)

                    errors = []

                    try:
                        validate(instance=json_item, schema=schema)
                    except ValidationError as e:
                        errors.append(f"Schema Error: {e.message}")

                    # 如果是 flows 分类，则继续进行分类层级验证
                    if category == "flows":
                        if json_item["flowDataSet"]["modellingAndValidation"]["LCIMethod"]["typeOfDataSet"] == "Product flow":
                            validation_result = (
                                validate_product_flows_classification_hierarchy(
                                    json_item["flowDataSet"]["flowInformation"]["dataSetInformation"]["classificationInformation"][
                                        "common:classification"
                                    ]["common:class"]
                                )
                            )
                            if not validation_result["valid"]:
                                errors.extend(validation_result["errors"])

                    # 输出所有错误信息
                    if errors:
                        for err in errors:
                            print(f"{RED}ERROR: {full_path} {err}{RESET}")
                            logging.error(f"ERROR: {full_path} {err}")
                    else:
                        print(f"{GREEN}INFO: {full_path} PASSED.{RESET}")
                        logging.info(f"INFO: {full_path} PASSED.")
