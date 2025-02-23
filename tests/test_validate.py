import glob
import json
import os
from src.tidas_tools.validate import category_validate


def test_category_validate():
    # Setup: Create a schema file with an empty schema (allows any data)
    schema_filename = os.path.join(os.getcwd(), "tidas_sources.json")
    with open(schema_filename, "w", encoding="utf-8") as f:
        json.dump({}, f)

    try:
        # Collect all JSON test files from tests/test_data/sources
        test_data = []
        sources_path = os.path.join(
            os.path.dirname(__file__), "test_data", "sources", "*.json"
        )
        for filepath in glob.glob(sources_path):
            with open(filepath, "r", encoding="utf-8") as f:
                test_data.append(json.load(f))

        # Invoke the validate function; it is expected to print messages without throwing exceptions.
        category_validate(test_data, "sources")
    finally:
        # Clean up: Remove the temporary schema file.
        if os.path.exists(schema_filename):
            os.remove(schema_filename)
