import importlib.util
import json
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[1]
SCHEMA_LOCK_SCRIPT = REPO_ROOT / "scripts" / "schema_lock.py"


def load_schema_lock_module():
    spec = importlib.util.spec_from_file_location("schema_lock", SCHEMA_LOCK_SCRIPT)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def test_contract_node_ignores_description_only():
    schema_lock = load_schema_lock_module()

    left = {
        "description": "English text",
        "properties": {
            "name": {
                "description": "Name",
                "type": "string",
            }
        },
    }
    right = {
        "description": "Chinese text",
        "properties": {
            "name": {
                "description": "名称",
                "type": "string",
            }
        },
    }

    assert schema_lock.contract_node(
        left, {"description"}
    ) == schema_lock.contract_node(
        right,
        {"description"},
    )


def test_schema_lock_matches_current_assets():
    schema_lock = load_schema_lock_module()

    generated = schema_lock.build_lock(schema_lock.DEFAULT_SCHEMA_ROOT)
    committed = json.loads(
        (schema_lock.DEFAULT_SCHEMA_ROOT / schema_lock.LOCK_FILENAME).read_text(
            encoding="utf-8"
        )
    )

    assert generated == committed
    assert generated["schemaSets"]["en"]["fileCount"] == 18
    assert generated["schemaSets"]["zh"]["fileCount"] == 18
    assert generated["translationPairs"]["fileCount"] == 18
