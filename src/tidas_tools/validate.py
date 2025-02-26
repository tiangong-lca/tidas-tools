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
                f"Product flow classification level error: at index {i}, expected level {i}, got {level}"
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
