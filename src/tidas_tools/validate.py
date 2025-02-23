import importlib.resources as pkg_resources
import json

from jsonschema import validate
from jsonschema.exceptions import ValidationError

import tidas_tools.schemas as schemas


def category_validate(json_data: list[dict], category: str):
    with pkg_resources.open_text(schemas, f"tidas_{category.lower()}.json") as f:
        schema = json.load(f)

    for json_item in json_data:
        # Use "filename" key if available, otherwise use a default label.
        file_name = json_item.get("filename", "unknown_file")
        try:
            validate(instance=json_item, schema=schema)
        except ValidationError as e:
            log_entry = {"filename": file_name, "status": "Error", "message": e.message}
            print(json.dumps(log_entry, indent=4, ensure_ascii=False))
        else:
            log_entry = {"filename": file_name, "status": "Success"}
            print(json.dumps(log_entry, indent=4, ensure_ascii=False))
