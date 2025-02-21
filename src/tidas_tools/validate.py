import importlib.resources as pkg_resources
import json

from jsonschema import validate

import tidas_tools.schemas as schemas


def category_validate(json_data: list[dict], category: str):
    with pkg_resources.open_text(schemas, f"tidas_{category.lower()}.json") as f:
            schema = json.load(f)

    for json_item in json_data:
        validate(json_item, schema)
