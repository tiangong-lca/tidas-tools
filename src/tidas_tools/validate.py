import importlib.resources as pkg_resources
import json
import os

from jsonschema import validate
from jsonschema.exceptions import ValidationError

import tidas_tools.schemas as schemas

GREEN = "\033[92m"
RED = "\033[91m"
RESET = "\033[0m"


def category_validate(json_file_path: str, category: str):

    with pkg_resources.open_text(schemas, f"tidas_{category.lower()}.json") as f:
        schema = json.load(f)

        for filename in os.listdir(json_file_path):
            if filename.endswith(".json"):
                full_path = os.path.join(json_file_path, filename)
                with open(full_path, "r") as json_file:
                    json_item = json.load(json_file)
                    try:
                        validate(instance=json_item, schema=schema)

                    except ValidationError as e:
                        print(f"{RED}Error: {full_path} {e.message}{RESET}")
                    else:
                        print(f"{GREEN}Passed: {full_path}{RESET}")
