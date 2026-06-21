#!/usr/bin/env python3
"""Validate and lock paired English/Chinese TIDAS schema assets."""

from __future__ import annotations

import argparse
import hashlib
import json
import sys
from pathlib import Path
from typing import Any
from urllib.parse import unquote

from jsonschema import Draft7Validator, SchemaError

REPO_ROOT = Path(__file__).resolve().parents[1]
DEFAULT_SCHEMA_ROOT = REPO_ROOT / "src" / "tidas_tools" / "tidas"
LOCK_FILENAME = "schema.lock.json"
SCHEMA_SETS = {
    "en": "schemas",
    "zh": "schemas_zh",
}
DEFAULT_LOCALIZED_KEYS = ("description",)


class SchemaLockError(Exception):
    def __init__(self, messages: list[str]):
        self.messages = messages
        super().__init__("\n".join(messages))


def canonical_bytes(value: Any) -> bytes:
    return json.dumps(
        value,
        ensure_ascii=False,
        sort_keys=True,
        separators=(",", ":"),
    ).encode("utf-8")


def sha256_bytes(value: bytes) -> str:
    return hashlib.sha256(value).hexdigest()


def sha256_json(value: Any) -> str:
    return sha256_bytes(canonical_bytes(value))


def repo_relative(path: Path) -> str:
    try:
        return path.resolve().relative_to(REPO_ROOT).as_posix()
    except ValueError:
        return path.as_posix()


def read_json_file(path: Path) -> Any:
    return json.loads(path.read_text(encoding="utf-8"))


def contract_node(node: Any, localized_keys: set[str]) -> Any:
    if isinstance(node, dict):
        return {
            key: contract_node(value, localized_keys)
            for key, value in node.items()
            if key not in localized_keys
        }
    if isinstance(node, list):
        return [contract_node(value, localized_keys) for value in node]
    return node


def iter_json_refs(node: Any, path: str = "$"):
    if isinstance(node, dict):
        for key, value in node.items():
            child_path = f"{path}/{key}"
            if key == "$ref" and isinstance(value, str):
                yield child_path, value
            else:
                yield from iter_json_refs(value, child_path)
    elif isinstance(node, list):
        for index, value in enumerate(node):
            yield from iter_json_refs(value, f"{path}/{index}")


def decode_json_pointer(pointer: str) -> list[str]:
    if pointer == "":
        return []
    if not pointer.startswith("/"):
        raise ValueError(f"JSON pointer must start with '/': {pointer}")
    return [
        unquote(token).replace("~1", "/").replace("~0", "~")
        for token in pointer[1:].split("/")
    ]


def pointer_exists(document: Any, pointer: str) -> bool:
    current = document
    for token in decode_json_pointer(pointer):
        if isinstance(current, dict):
            if token not in current:
                return False
            current = current[token]
        elif isinstance(current, list):
            try:
                index = int(token)
            except ValueError:
                return False
            if index < 0 or index >= len(current):
                return False
            current = current[index]
        else:
            return False
    return True


def validate_local_refs(
    docs: dict[str, Any],
    schema_dir: Path,
    lang: str,
    errors: list[str],
) -> None:
    for current_name, document in sorted(docs.items()):
        for ref_path, ref in iter_json_refs(document):
            if ref.startswith(("http://", "https://")):
                continue

            if "#" in ref:
                file_part, pointer = ref.split("#", 1)
            else:
                file_part, pointer = ref, ""

            target_ref = Path(file_part) if file_part else Path(current_name)
            if target_ref.is_absolute() or ".." in target_ref.parts:
                errors.append(
                    f"{lang}:{current_name}:{ref_path}: unsupported local $ref {ref!r}"
                )
                continue

            target_name = target_ref.name
            target_path = schema_dir / target_ref
            if target_name not in docs:
                errors.append(
                    f"{lang}:{current_name}:{ref_path}: missing $ref target "
                    f"{repo_relative(target_path)}"
                )
                continue

            try:
                if not pointer_exists(docs[target_name], pointer):
                    errors.append(
                        f"{lang}:{current_name}:{ref_path}: missing JSON pointer "
                        f"#{pointer} in {target_name}"
                    )
            except ValueError as exc:
                errors.append(f"{lang}:{current_name}:{ref_path}: {exc}")


def collect_schema_set(
    schema_root: Path,
    lang: str,
    subdir: str,
    errors: list[str],
) -> dict[str, Any]:
    schema_dir = schema_root / subdir
    docs: dict[str, Any] = {}
    files: dict[str, dict[str, str]] = {}

    if not schema_dir.is_dir():
        errors.append(f"{lang}: missing schema directory {repo_relative(schema_dir)}")
        return {"docs": docs, "files": files}

    for path in sorted(schema_dir.glob("*.json")):
        try:
            document = read_json_file(path)
        except Exception as exc:  # noqa: BLE001 - include parse details in CI output.
            errors.append(f"{repo_relative(path)}: invalid JSON: {exc}")
            continue

        try:
            Draft7Validator.check_schema(document)
        except SchemaError as exc:
            errors.append(
                f"{repo_relative(path)}: invalid Draft 7 schema: {exc.message}"
            )

        docs[path.name] = document
        files[path.name] = {
            "contentSha256": sha256_bytes(path.read_bytes()),
        }

    return {"docs": docs, "files": files, "path": schema_dir}


def first_diffs(left: Any, right: Any, path: str = "$", limit: int = 12) -> list[str]:
    diffs: list[str] = []

    def walk(a: Any, b: Any, current_path: str) -> None:
        if len(diffs) >= limit:
            return
        if type(a) is not type(b):
            diffs.append(
                f"{current_path}: type {type(a).__name__} != {type(b).__name__}"
            )
            return
        if isinstance(a, dict):
            left_keys = set(a)
            right_keys = set(b)
            for key in sorted(left_keys - right_keys):
                if len(diffs) >= limit:
                    return
                diffs.append(f"{current_path}/{key}: only in en")
            for key in sorted(right_keys - left_keys):
                if len(diffs) >= limit:
                    return
                diffs.append(f"{current_path}/{key}: only in zh")
            for key in sorted(left_keys & right_keys):
                walk(a[key], b[key], f"{current_path}/{key}")
            return
        if isinstance(a, list):
            if len(a) != len(b):
                diffs.append(f"{current_path}: list length {len(a)} != {len(b)}")
            for index, (left_item, right_item) in enumerate(zip(a, b)):
                walk(left_item, right_item, f"{current_path}/{index}")
            return
        if a != b:
            diffs.append(f"{current_path}: {a!r} != {b!r}")

    walk(left, right, path)
    return diffs


def schema_root_label(schema_root: Path) -> str:
    try:
        return schema_root.resolve().relative_to(REPO_ROOT).as_posix()
    except ValueError:
        return schema_root.as_posix()


def aggregate_file_hash(files: dict[str, dict[str, str]], key: str) -> str:
    return sha256_json({name: values[key] for name, values in sorted(files.items())})


def build_lock(
    schema_root: Path = DEFAULT_SCHEMA_ROOT,
    localized_keys: tuple[str, ...] = DEFAULT_LOCALIZED_KEYS,
) -> dict[str, Any]:
    schema_root = schema_root.resolve()
    errors: list[str] = []
    localized_key_set = set(localized_keys)
    collected: dict[str, dict[str, Any]] = {}

    for lang, subdir in SCHEMA_SETS.items():
        collected[lang] = collect_schema_set(schema_root, lang, subdir, errors)

    en_names = set(collected["en"]["docs"])
    zh_names = set(collected["zh"]["docs"])
    for name in sorted(en_names - zh_names):
        errors.append(f"schema file missing from zh set: {name}")
    for name in sorted(zh_names - en_names):
        errors.append(f"schema file missing from en set: {name}")

    for lang, result in collected.items():
        validate_local_refs(result["docs"], result["path"], lang, errors)

    shared_names = sorted(en_names & zh_names)
    pair_files: dict[str, dict[str, str]] = {}

    for name in shared_names:
        en_contract = contract_node(collected["en"]["docs"][name], localized_key_set)
        zh_contract = contract_node(collected["zh"]["docs"][name], localized_key_set)
        en_contract_hash = sha256_json(en_contract)
        zh_contract_hash = sha256_json(zh_contract)

        collected["en"]["files"][name]["contractSha256"] = en_contract_hash
        collected["zh"]["files"][name]["contractSha256"] = zh_contract_hash

        if en_contract != zh_contract:
            diffs = "\n    ".join(first_diffs(en_contract, zh_contract))
            errors.append(
                f"{name}: en/zh contract differs after removing localized keys "
                f"{sorted(localized_keys)}:\n    {diffs}"
            )

        pair_files[name] = {
            "contractSha256": en_contract_hash,
            "enContentSha256": collected["en"]["files"][name]["contentSha256"],
            "zhContentSha256": collected["zh"]["files"][name]["contentSha256"],
        }

    if errors:
        raise SchemaLockError(errors)

    schema_sets = {}
    for lang, subdir in SCHEMA_SETS.items():
        files = dict(sorted(collected[lang]["files"].items()))
        schema_sets[lang] = {
            "path": subdir,
            "fileCount": len(files),
            "contentAggregateSha256": aggregate_file_hash(files, "contentSha256"),
            "contractAggregateSha256": aggregate_file_hash(files, "contractSha256"),
            "files": files,
        }

    return {
        "version": 1,
        "schemaRoot": schema_root_label(schema_root),
        "allowedLocalizedKeys": list(localized_keys),
        "generation": {
            "tool": "scripts/schema_lock.py",
            "mode": "deterministic-v1",
        },
        "schemaSets": schema_sets,
        "translationPairs": {
            "fileCount": len(pair_files),
            "contractAggregateSha256": sha256_json(
                {
                    name: values["contractSha256"]
                    for name, values in sorted(pair_files.items())
                }
            ),
            "files": dict(sorted(pair_files.items())),
        },
    }


def lock_path(schema_root: Path) -> Path:
    return schema_root / LOCK_FILENAME


def dump_lock(lock: dict[str, Any]) -> str:
    return json.dumps(lock, ensure_ascii=False, indent=2, sort_keys=True) + "\n"


def update_lock(schema_root: Path) -> int:
    lock = build_lock(schema_root)
    target = lock_path(schema_root)
    target.write_text(dump_lock(lock), encoding="utf-8")
    print(f"Updated {repo_relative(target)}")
    return 0


def check_lock(schema_root: Path) -> int:
    expected = build_lock(schema_root)
    target = lock_path(schema_root)

    if not target.is_file():
        print(
            f"Missing {repo_relative(target)}. "
            "Run: uv run python scripts/schema_lock.py update",
            file=sys.stderr,
        )
        return 1

    try:
        actual = read_json_file(target)
    except Exception as exc:  # noqa: BLE001 - include parse details in CI output.
        print(f"{repo_relative(target)} is not valid JSON: {exc}", file=sys.stderr)
        return 1

    if actual != expected:
        print(
            f"{repo_relative(target)} is stale. "
            "Run: uv run python scripts/schema_lock.py update",
            file=sys.stderr,
        )
        for diff in first_diffs(actual, expected, limit=20):
            print(f"  {diff}", file=sys.stderr)
        return 1

    pair_count = expected["translationPairs"]["fileCount"]
    aggregate = expected["translationPairs"]["contractAggregateSha256"]
    print(f"Schema lock OK: {pair_count} schema pairs, contract {aggregate}")
    return 0


def parse_args(argv: list[str]) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "command",
        choices=("check", "update"),
        help="check the committed lock or update it from current schema assets",
    )
    parser.add_argument(
        "--schema-root",
        type=Path,
        default=DEFAULT_SCHEMA_ROOT,
        help="directory containing schemas/, schemas_zh/, and schema.lock.json",
    )
    return parser.parse_args(argv)


def main(argv: list[str] | None = None) -> int:
    args = parse_args(sys.argv[1:] if argv is None else argv)
    try:
        if args.command == "update":
            return update_lock(args.schema_root)
        return check_lock(args.schema_root)
    except SchemaLockError as exc:
        print("Schema lock validation failed:", file=sys.stderr)
        for message in exc.messages:
            print(f"  - {message}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
