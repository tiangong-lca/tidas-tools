"""Deterministic TIDAS/ILCD release package operations.

This module owns format conversion, exact reference closure, semantic round-trip,
and byte-stable ZIP construction for the two TianGong release profiles. It does
not assign dataset UUIDs or versions and it performs no database or object-store
mutation.
"""

from __future__ import annotations

import argparse
from dataclasses import dataclass
from functools import lru_cache
import hashlib
import json
from pathlib import Path
import shutil
import sys
import tempfile
from typing import Any, Iterable
import zipfile

import xmltodict

from .validate import validate_ilcd_package_dir, validate_package_dir

SCHEMA_VERSION = "tiangong.tidas-tools.release-result.v1"
UNIT_PROFILE = "unit-process-full-closure.v1"
RESULT_PROFILE = "standalone-lifecyclemodel-result-full-closure.v1"
PROFILES = (UNIT_PROFILE, RESULT_PROFILE)
TIDAS_SCHEMA_ROOT = Path(__file__).resolve().parent / "tidas" / "schemas"

REFERENCE_TYPE_TO_DATASET_TYPE = {
    "contact data set": "contact",
    "flow data set": "flow",
    "flow property data set": "flowproperty",
    "unit group data set": "unitgroup",
    "process data set": "process",
    "source data set": "source",
    "lcia method data set": "lciamethod",
    "life cycle model data set": "lifecyclemodel",
}


class ReleaseToolError(RuntimeError):
    """Fail-closed release tooling error with a stable machine code."""

    def __init__(self, code: str, message: str):
        super().__init__(message)
        self.code = code


@dataclass(frozen=True)
class DatasetEntry:
    dataset_type: str
    role: str
    uuid: str
    version: str
    relative_path: str
    sha256: str
    canonical_content_hash: str

    @property
    def key(self) -> str:
        return f"{self.dataset_type}:{self.uuid.lower()}:{self.version}"


def _sha256_bytes(value: bytes) -> str:
    return hashlib.sha256(value).hexdigest()


def sha256_file(file_path: str | Path) -> str:
    digest = hashlib.sha256()
    with Path(file_path).open("rb") as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def _canonical_strings(value: Any) -> bytes:
    return json.dumps(
        value, ensure_ascii=False, sort_keys=True, separators=(",", ":")
    ).encode("utf-8")


def _contained(root: Path, relative_path: str) -> Path:
    resolved_root = root.resolve()
    resolved = (resolved_root / relative_path).resolve()
    if resolved != resolved_root and resolved_root not in resolved.parents:
        raise ReleaseToolError(
            "release_path_outside_root", f"Path escapes release root: {relative_path}"
        )
    return resolved


def _write_json(file_path: Path, value: Any) -> None:
    file_path.parent.mkdir(parents=True, exist_ok=True)
    file_path.write_text(
        json.dumps(value, ensure_ascii=False, sort_keys=True, indent=2) + "\n",
        encoding="utf-8",
        newline="\n",
    )


@lru_cache(maxsize=None)
def _load_ordering_schema(schema_path: str) -> dict[str, Any]:
    path = Path(schema_path)
    try:
        document = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as exc:
        raise ReleaseToolError("tidas_ordering_schema_invalid", str(path)) from exc
    if not isinstance(document, dict):
        raise ReleaseToolError("tidas_ordering_schema_invalid", str(path))
    return document


def _resolve_ordering_schema(
    schema: dict[str, Any], schema_path: Path
) -> tuple[dict[str, Any], Path]:
    current = schema
    current_path = schema_path
    seen: set[tuple[str, str]] = set()
    while isinstance(current.get("$ref"), str):
        reference = str(current["$ref"])
        file_name, separator, fragment = reference.partition("#")
        target_path = (
            current_path
            if not file_name
            else (current_path.parent / file_name).resolve()
        )
        marker = (str(target_path), fragment)
        if marker in seen:
            raise ReleaseToolError("tidas_ordering_schema_cycle", reference)
        seen.add(marker)
        target: Any = _load_ordering_schema(str(target_path))
        if separator and fragment:
            if not fragment.startswith("/"):
                raise ReleaseToolError("tidas_ordering_schema_ref_invalid", reference)
            for raw_token in fragment[1:].split("/"):
                token = raw_token.replace("~1", "/").replace("~0", "~")
                if not isinstance(target, dict) or token not in target:
                    raise ReleaseToolError(
                        "tidas_ordering_schema_ref_invalid", reference
                    )
                target = target[token]
        if not isinstance(target, dict):
            raise ReleaseToolError("tidas_ordering_schema_ref_invalid", reference)
        current = target
        current_path = target_path
    return current, current_path


def _schema_matches_value(
    schema: dict[str, Any], schema_path: Path, value: Any
) -> bool:
    resolved, resolved_path = _resolve_ordering_schema(schema, schema_path)
    alternatives = resolved.get("oneOf") or resolved.get("anyOf")
    if isinstance(alternatives, list):
        return any(
            isinstance(candidate, dict)
            and _schema_matches_value(candidate, resolved_path, value)
            for candidate in alternatives
        )
    schema_type = resolved.get("type")
    allowed = {schema_type} if isinstance(schema_type, str) else set(schema_type or [])
    if isinstance(value, dict):
        return not allowed or "object" in allowed or "properties" in resolved
    if isinstance(value, list):
        return not allowed or "array" in allowed or "items" in resolved
    if value is None:
        return not allowed or "null" in allowed
    if isinstance(value, bool):
        return not allowed or "boolean" in allowed
    if isinstance(value, (int, float)):
        return not allowed or bool({"integer", "number"} & allowed)
    return not allowed or "string" in allowed


def _select_ordering_schema(
    schema: dict[str, Any], schema_path: Path, value: Any
) -> tuple[dict[str, Any], Path]:
    resolved, resolved_path = _resolve_ordering_schema(schema, schema_path)
    alternatives = resolved.get("oneOf") or resolved.get("anyOf")
    if isinstance(alternatives, list):
        for candidate in alternatives:
            if isinstance(candidate, dict) and _schema_matches_value(
                candidate, resolved_path, value
            ):
                return _select_ordering_schema(candidate, resolved_path, value)
    return resolved, resolved_path


def _ordered_property_schemas(
    schema: dict[str, Any], schema_path: Path, value: dict[str, Any]
) -> dict[str, tuple[dict[str, Any], Path]]:
    resolved, resolved_path = _select_ordering_schema(schema, schema_path, value)
    ordered: dict[str, tuple[dict[str, Any], Path]] = {}
    all_of = resolved.get("allOf")
    if isinstance(all_of, list):
        for component in all_of:
            if isinstance(component, dict):
                ordered.update(
                    _ordered_property_schemas(component, resolved_path, value)
                )
    properties = resolved.get("properties")
    if isinstance(properties, dict):
        for name, child_schema in properties.items():
            if isinstance(child_schema, dict):
                ordered[name] = (child_schema, resolved_path)
    return ordered


def _array_item_schema(
    schema: dict[str, Any], schema_path: Path, value: list[Any]
) -> tuple[dict[str, Any], Path] | None:
    resolved, resolved_path = _select_ordering_schema(schema, schema_path, value)
    items = resolved.get("items")
    if isinstance(items, dict):
        return items, resolved_path
    all_of = resolved.get("allOf")
    if isinstance(all_of, list):
        for component in all_of:
            if isinstance(component, dict):
                selected = _array_item_schema(component, resolved_path, value)
                if selected is not None:
                    return selected
    return None


def _order_value_for_xml(value: Any, schema: dict[str, Any], schema_path: Path) -> Any:
    if isinstance(value, list):
        item_schema = _array_item_schema(schema, schema_path, value)
        if item_schema is None:
            return value
        child_schema, child_path = item_schema
        return [_order_value_for_xml(item, child_schema, child_path) for item in value]
    if not isinstance(value, dict):
        return value
    properties = _ordered_property_schemas(schema, schema_path, value)
    ordered: dict[str, Any] = {}
    for name, (child_schema, child_path) in properties.items():
        if name in value:
            ordered[name] = _order_value_for_xml(value[name], child_schema, child_path)
    for name in sorted(set(value) - set(ordered)):
        ordered[name] = value[name]
    return ordered


def order_tidas_document_for_xml(
    document: dict[str, Any], category: str
) -> dict[str, Any]:
    """Restore schema element order before serializing canonical TIDAS JSON as ILCD XML."""

    schema_path = (TIDAS_SCHEMA_ROOT / f"tidas_{category.lower()}.json").resolve()
    if not schema_path.is_file():
        raise ReleaseToolError("tidas_ordering_schema_missing", category)
    schema = _load_ordering_schema(str(schema_path))
    ordered = _order_value_for_xml(document, schema, schema_path)
    if not isinstance(ordered, dict):
        raise ReleaseToolError("tidas_document_invalid", category)
    return ordered


def _convert_reference_uris_for_ilcd(value: Any) -> Any:
    if isinstance(value, list):
        return [_convert_reference_uris_for_ilcd(item) for item in value]
    if not isinstance(value, dict):
        return value
    converted: dict[str, Any] = {}
    for name, child in value.items():
        if name == "@uri" and isinstance(child, str) and child.endswith(".json"):
            converted[name] = f"{child[:-5]}.xml"
        else:
            converted[name] = _convert_reference_uris_for_ilcd(child)
    return converted


def load_dataset_index(
    index_path: str | Path, input_dir: str | Path
) -> list[DatasetEntry]:
    index_file = Path(index_path).resolve()
    root = Path(input_dir).resolve()
    try:
        document = json.loads(index_file.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as exc:
        raise ReleaseToolError("dataset_index_invalid", str(exc)) from exc
    if document.get("schemaVersion") != "tiangong.release.canonical-dataset-index.v1":
        raise ReleaseToolError("dataset_index_schema_unsupported", str(index_file))
    raw_entries = document.get("datasets")
    if not isinstance(raw_entries, list) or not raw_entries:
        raise ReleaseToolError("dataset_index_empty", str(index_file))

    entries: list[DatasetEntry] = []
    seen_keys: set[str] = set()
    seen_paths: set[str] = set()
    for raw in raw_entries:
        if not isinstance(raw, dict):
            raise ReleaseToolError("dataset_index_entry_invalid", repr(raw))
        try:
            entry = DatasetEntry(
                dataset_type=str(raw["datasetType"]),
                role=str(raw["role"]),
                uuid=str(raw["uuid"]).lower(),
                version=str(raw["version"]),
                relative_path=str(raw["path"]),
                sha256=str(raw["sha256"]),
                canonical_content_hash=str(raw["canonicalContentHash"]),
            )
        except KeyError as exc:
            raise ReleaseToolError(
                "dataset_index_entry_invalid", f"Missing {exc.args[0]}"
            ) from exc
        if entry.key in seen_keys:
            raise ReleaseToolError("dataset_identity_duplicate", entry.key)
        if entry.relative_path in seen_paths:
            raise ReleaseToolError("dataset_path_duplicate", entry.relative_path)
        file_path = _contained(root, entry.relative_path)
        if not file_path.is_file():
            raise ReleaseToolError("dataset_file_missing", entry.relative_path)
        if sha256_file(file_path) != entry.sha256:
            raise ReleaseToolError("dataset_file_hash_mismatch", entry.relative_path)
        seen_keys.add(entry.key)
        seen_paths.add(entry.relative_path)
        entries.append(entry)
    return sorted(entries, key=lambda item: item.key)


def _walk_references(value: Any, location: str = "$") -> Iterable[tuple[str, str]]:
    if isinstance(value, dict):
        reference_id = value.get("@refObjectId")
        reference_type = value.get("@type")
        if isinstance(reference_id, str) and isinstance(reference_type, str):
            dataset_type = REFERENCE_TYPE_TO_DATASET_TYPE.get(reference_type.lower())
            if dataset_type:
                version = value.get("@version")
                if not isinstance(version, str) or not version:
                    raise ReleaseToolError(
                        "reference_version_missing",
                        f"{location} references {reference_type} {reference_id} without @version",
                    )
                yield f"{dataset_type}:{reference_id.lower()}:{version}", location
        for key, child in value.items():
            yield from _walk_references(child, f"{location}/{key}")
    elif isinstance(value, list):
        for index, child in enumerate(value):
            yield from _walk_references(child, f"{location}/{index}")


def resolve_profile_closure(
    *,
    input_dir: str | Path,
    entries: list[DatasetEntry],
    profile_id: str,
) -> tuple[list[DatasetEntry], dict[str, Any]]:
    if profile_id not in PROFILES:
        raise ReleaseToolError("release_profile_unsupported", profile_id)
    root = Path(input_dir).resolve()
    index = {entry.key: entry for entry in entries}
    if profile_id == UNIT_PROFILE:
        root_keys = sorted(
            entry.key for entry in entries if entry.role == "unit_process"
        )
    else:
        root_keys = sorted(
            entry.key
            for entry in entries
            if entry.role in {"lifecycle_model", "result_process"}
        )
    if not root_keys:
        raise ReleaseToolError("release_profile_roots_missing", profile_id)

    selected: dict[str, DatasetEntry] = {}
    pending = list(reversed(root_keys))
    reference_count = 0
    while pending:
        key = pending.pop()
        if key in selected:
            continue
        entry = index.get(key)
        if entry is None:
            raise ReleaseToolError("reference_closure_missing", key)
        selected[key] = entry
        document_path = _contained(root, entry.relative_path)
        try:
            document = json.loads(document_path.read_text(encoding="utf-8"))
        except (OSError, json.JSONDecodeError) as exc:
            raise ReleaseToolError("dataset_json_invalid", entry.relative_path) from exc
        references = sorted(set(_walk_references(document)))
        reference_count += len(references)
        for referenced_key, location in reversed(references):
            if referenced_key not in index:
                raise ReleaseToolError(
                    "reference_closure_missing",
                    f"{entry.key} {location} -> {referenced_key}",
                )
            if referenced_key not in selected:
                pending.append(referenced_key)

    ordered = [selected[key] for key in sorted(selected)]
    closure_hash = _sha256_bytes(_canonical_strings([entry.key for entry in ordered]))
    return ordered, {
        "schemaVersion": "tiangong.tidas-tools.reference-closure-report.v1",
        "status": "passed",
        "profileId": profile_id,
        "rootCount": len(root_keys),
        "datasetCount": len(ordered),
        "referenceCount": reference_count,
        "closureHash": closure_hash,
        "datasetKeys": [entry.key for entry in ordered],
    }


def convert_tidas_to_ilcd(
    input_dir: str | Path, output_dir: str | Path
) -> dict[str, Any]:
    source_root = Path(input_dir).resolve()
    target_root = Path(output_dir).resolve()
    target_root.parent.mkdir(parents=True, exist_ok=True)
    staging = Path(
        tempfile.mkdtemp(prefix=f".{target_root.name}-", dir=str(target_root.parent))
    )
    converted: list[dict[str, Any]] = []
    try:
        for category_dir in sorted(
            path for path in source_root.iterdir() if path.is_dir()
        ):
            for source_file in sorted(category_dir.glob("*.json")):
                relative = source_file.relative_to(source_root)
                document = json.loads(source_file.read_text(encoding="utf-8"))
                ordered_document = order_tidas_document_for_xml(
                    document, category_dir.name
                )
                ilcd_document = _convert_reference_uris_for_ilcd(ordered_document)
                xml = xmltodict.unparse(ilcd_document, pretty=True)
                output_relative = Path("data") / relative.with_suffix(".xml")
                target_file = staging / output_relative
                target_file.parent.mkdir(parents=True, exist_ok=True)
                target_file.write_text(xml, encoding="utf-8", newline="\n")
                converted.append(
                    {
                        "sourcePath": relative.as_posix(),
                        "outputPath": output_relative.as_posix(),
                        "sourceSha256": sha256_file(source_file),
                        "outputSha256": sha256_file(target_file),
                    }
                )

        assets_root = Path(__file__).resolve().parent / "eilcd"
        for asset in sorted(assets_root.iterdir()):
            if asset.name.startswith("__"):
                continue
            destination = staging / asset.name
            if asset.is_dir():
                shutil.copytree(asset, destination)
            elif asset.is_file():
                shutil.copy2(asset, destination)
        if target_root.exists():
            shutil.rmtree(target_root)
        staging.replace(target_root)
    except Exception:
        shutil.rmtree(staging, ignore_errors=True)
        raise

    return {
        "schemaVersion": "tiangong.tidas-tools.ilcd-conversion-report.v1",
        "status": "passed",
        "datasetCount": len(converted),
        "conversionSetHash": _sha256_bytes(_canonical_strings(converted)),
        "datasets": converted,
    }


def _normalize_roundtrip(value: Any, field_name: str | None = None) -> Any:
    if isinstance(value, dict):
        normalized = {
            key: _normalize_roundtrip(child, key)
            for key, child in sorted(value.items(), key=lambda item: item[0])
        }
        return normalized or None
    if isinstance(value, list):
        normalized = [_normalize_roundtrip(child, field_name) for child in value]
        return normalized[0] if len(normalized) == 1 else normalized
    if value is None:
        return None
    if isinstance(value, bool):
        return "true" if value else "false"
    if isinstance(value, (int, float)):
        return str(value)
    if field_name == "@uri" and isinstance(value, str) and value.endswith(".xml"):
        return f"{value[:-4]}.json"
    return value


def _first_difference(left: Any, right: Any, location: str = "$") -> str | None:
    if type(left) is not type(right):
        return location
    if isinstance(left, dict):
        if left.keys() != right.keys():
            return location
        for key in left:
            difference = _first_difference(left[key], right[key], f"{location}/{key}")
            if difference:
                return difference
        return None
    if isinstance(left, list):
        if len(left) != len(right):
            return location
        for index, (left_item, right_item) in enumerate(zip(left, right)):
            difference = _first_difference(left_item, right_item, f"{location}/{index}")
            if difference:
                return difference
        return None
    return None if left == right else location


def semantic_roundtrip_report(
    tidas_dir: str | Path, ilcd_dir: str | Path
) -> dict[str, Any]:
    tidas_root = Path(tidas_dir).resolve()
    ilcd_root = Path(ilcd_dir).resolve()
    datasets: list[dict[str, Any]] = []
    mismatches: list[dict[str, str]] = []
    for source_file in sorted(tidas_root.glob("*/*.json")):
        relative = source_file.relative_to(tidas_root)
        xml_file = ilcd_root / "data" / relative.with_suffix(".xml")
        if not xml_file.is_file():
            mismatches.append(
                {"path": relative.as_posix(), "location": "$", "code": "xml_missing"}
            )
            continue
        source = _normalize_roundtrip(
            json.loads(source_file.read_text(encoding="utf-8"))
        )
        converted = _normalize_roundtrip(
            xmltodict.parse(xml_file.read_text(encoding="utf-8"))
        )
        difference = _first_difference(source, converted)
        record = {
            "path": relative.as_posix(),
            "normalizedSourceHash": _sha256_bytes(_canonical_strings(source)),
            "normalizedRoundtripHash": _sha256_bytes(_canonical_strings(converted)),
        }
        datasets.append(record)
        if difference:
            mismatches.append(
                {
                    "path": relative.as_posix(),
                    "location": difference,
                    "code": "semantic_roundtrip_mismatch",
                }
            )
    return {
        "schemaVersion": "tiangong.tidas-tools.semantic-roundtrip-report.v1",
        "status": "passed" if not mismatches else "failed",
        "datasetCount": len(datasets),
        "mismatchCount": len(mismatches),
        "datasets": datasets,
        "mismatches": mismatches,
    }


def _normalized_validation_report(report: dict[str, Any], root: Path) -> dict[str, Any]:
    normalized = json.loads(json.dumps(report))
    normalized["input_dir"] = "."
    for issue in normalized.get("issues", []):
        file_path = Path(issue.get("file_path", ""))
        if file_path.is_absolute():
            issue["file_path"] = file_path.relative_to(root).as_posix()
    for category in normalized.get("categories", []):
        for issue in category.get("issues", []):
            file_path = Path(issue.get("file_path", ""))
            if file_path.is_absolute():
                issue["file_path"] = file_path.relative_to(root).as_posix()
    return normalized


def validate_release_tree(input_dir: str | Path, data_format: str) -> dict[str, Any]:
    root = Path(input_dir).resolve()
    if data_format == "tidas":
        report = validate_package_dir(str(root), emit_logs=False)
    elif data_format == "ilcd":
        report = validate_ilcd_package_dir(str(root), emit_logs=False)
    else:
        raise ReleaseToolError("validation_format_unsupported", data_format)
    return {
        "schemaVersion": "tiangong.tidas-tools.release-validation-report.v1",
        "status": "passed" if report["ok"] else "failed",
        "format": data_format,
        "report": _normalized_validation_report(report, root),
    }


def deterministic_zip(
    output_path: str | Path, members: Iterable[tuple[Path, str]]
) -> dict[str, Any]:
    output = Path(output_path).resolve()
    output.parent.mkdir(parents=True, exist_ok=True)
    normalized: list[tuple[Path, str]] = []
    for source, raw_name in members:
        name = raw_name.replace("\\", "/")
        if (
            not name
            or name.startswith("/")
            or any(part in {"", ".", ".."} for part in name.split("/"))
        ):
            raise ReleaseToolError("zip_member_path_invalid", raw_name)
        normalized.append((source.resolve(), name))
    ordered = sorted(normalized, key=lambda item: item[1])
    names = [name for _source, name in ordered]
    if len(names) != len(set(names)):
        raise ReleaseToolError("zip_member_duplicate", str(output))
    with tempfile.NamedTemporaryFile(
        prefix=f".{output.name}.", suffix=".tmp", dir=output.parent, delete=False
    ) as temporary_file:
        temporary = Path(temporary_file.name)
    try:
        with zipfile.ZipFile(temporary, "w", compression=zipfile.ZIP_STORED) as archive:
            for source, name in ordered:
                if not source.is_file():
                    raise ReleaseToolError("zip_member_missing", str(source))
                info = zipfile.ZipInfo(name, date_time=(1980, 1, 1, 0, 0, 0))
                info.compress_type = zipfile.ZIP_STORED
                info.create_system = 3
                info.external_attr = (0o100644 & 0xFFFF) << 16
                info.flag_bits |= 0x800
                archive.writestr(info, source.read_bytes())
        temporary.replace(output)
    finally:
        temporary.unlink(missing_ok=True)
    return {
        "path": str(output),
        "sha256": sha256_file(output),
        "byteSize": output.stat().st_size,
        "mediaType": "application/zip",
        "memberCount": len(ordered),
    }


def _package_members(
    *,
    entries: list[DatasetEntry],
    format_name: str,
    tidas_root: Path,
    ilcd_root: Path | None,
) -> list[tuple[Path, str]]:
    if format_name == "tidas":
        return [
            (_contained(tidas_root, entry.relative_path), entry.relative_path)
            for entry in entries
        ]
    if format_name != "ilcd" or ilcd_root is None:
        raise ReleaseToolError("release_package_format_unavailable", format_name)
    members = [
        (
            _contained(
                ilcd_root,
                f"data/{Path(entry.relative_path).with_suffix('.xml').as_posix()}",
            ),
            f"data/{Path(entry.relative_path).with_suffix('.xml').as_posix()}",
        )
        for entry in entries
    ]
    for asset in sorted(ilcd_root.rglob("*")):
        if not asset.is_file() or (ilcd_root / "data") in asset.parents:
            continue
        members.append((asset, asset.relative_to(ilcd_root).as_posix()))
    return members


def build_release_packages(
    *,
    tidas_dir: str | Path,
    dataset_index: str | Path,
    output_dir: str | Path,
    ilcd_dir: str | Path | None = None,
) -> dict[str, Any]:
    tidas_root = Path(tidas_dir).resolve()
    ilcd_root = Path(ilcd_dir).resolve() if ilcd_dir else None
    entries = load_dataset_index(dataset_index, tidas_root)
    output_root = Path(output_dir).resolve()
    output_root.mkdir(parents=True, exist_ok=True)

    closures: dict[str, tuple[list[DatasetEntry], dict[str, Any]]] = {
        profile: resolve_profile_closure(
            input_dir=tidas_root, entries=entries, profile_id=profile
        )
        for profile in PROFILES
    }
    unit_keys = {entry.key for entry in closures[UNIT_PROFILE][0]}
    result_keys = {entry.key for entry in closures[RESULT_PROFILE][0]}
    if not unit_keys.issubset(result_keys):
        missing = sorted(unit_keys - result_keys)
        raise ReleaseToolError(
            "standalone_profile_missing_unit_closure", ",".join(missing)
        )

    formats = ["tidas"] + (["ilcd"] if ilcd_root else [])
    packages: list[dict[str, Any]] = []
    for profile in PROFILES:
        selected, closure_report = closures[profile]
        for format_name in formats:
            output_path = output_root / f"{profile}.{format_name}.zip"
            artifact = deterministic_zip(
                output_path,
                _package_members(
                    entries=selected,
                    format_name=format_name,
                    tidas_root=tidas_root,
                    ilcd_root=ilcd_root,
                ),
            )
            packages.append(
                {
                    "profileId": profile,
                    "format": format_name,
                    "selfContained": True,
                    "closureHash": closure_report["closureHash"],
                    "datasetCount": closure_report["datasetCount"],
                    "artifact": artifact,
                }
            )

    return {
        "schemaVersion": "tiangong.tidas-tools.release-packages-report.v1",
        "status": "passed",
        "profiles": [closures[profile][1] for profile in PROFILES],
        "packages": packages,
        "artifactSetHash": _sha256_bytes(
            _canonical_strings(
                [
                    {
                        "profileId": item["profileId"],
                        "format": item["format"],
                        "sha256": item["artifact"]["sha256"],
                    }
                    for item in packages
                ]
            )
        ),
    }


def _parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        description="Deterministic TIDAS/ILCD release conversion and package tool."
    )
    subparsers = parser.add_subparsers(dest="command", required=True)

    convert = subparsers.add_parser("convert-ilcd")
    convert.add_argument("--input-dir", required=True)
    convert.add_argument("--output-dir", required=True)

    for name, data_format in (("validate-tidas", "tidas"), ("validate-ilcd", "ilcd")):
        validate = subparsers.add_parser(name)
        validate.add_argument("--input-dir", required=True)
        validate.set_defaults(data_format=data_format)

    roundtrip = subparsers.add_parser("semantic-roundtrip")
    roundtrip.add_argument("--tidas-dir", required=True)
    roundtrip.add_argument("--ilcd-dir", required=True)

    package = subparsers.add_parser("build-packages")
    package.add_argument("--tidas-dir", required=True)
    package.add_argument("--ilcd-dir")
    package.add_argument("--dataset-index", required=True)
    package.add_argument("--output-dir", required=True)

    closure = subparsers.add_parser("validate-closure")
    closure.add_argument("--input-dir", required=True)
    closure.add_argument("--dataset-index", required=True)
    closure.add_argument("--profile", choices=PROFILES, required=True)

    for command in subparsers.choices.values():
        command.add_argument("--report")
    return parser


def execute(args: argparse.Namespace) -> dict[str, Any]:
    if args.command == "convert-ilcd":
        return convert_tidas_to_ilcd(args.input_dir, args.output_dir)
    if args.command in {"validate-tidas", "validate-ilcd"}:
        return validate_release_tree(args.input_dir, args.data_format)
    if args.command == "semantic-roundtrip":
        return semantic_roundtrip_report(args.tidas_dir, args.ilcd_dir)
    if args.command == "build-packages":
        return build_release_packages(
            tidas_dir=args.tidas_dir,
            ilcd_dir=args.ilcd_dir,
            dataset_index=args.dataset_index,
            output_dir=args.output_dir,
        )
    if args.command == "validate-closure":
        entries = load_dataset_index(args.dataset_index, args.input_dir)
        _selected, report = resolve_profile_closure(
            input_dir=args.input_dir, entries=entries, profile_id=args.profile
        )
        return report
    raise ReleaseToolError("release_command_unknown", str(args.command))


def main(argv: list[str] | None = None) -> int:
    parser = _parser()
    args = parser.parse_args(argv)
    try:
        result = execute(args)
        exit_code = 0 if result.get("status") == "passed" else 1
    except ReleaseToolError as exc:
        result = {
            "schemaVersion": SCHEMA_VERSION,
            "status": "failed",
            "command": args.command,
            "error": {"code": exc.code, "message": str(exc)},
        }
        exit_code = 1
    except Exception as exc:  # fail closed without leaking a traceback to JSON stdout
        result = {
            "schemaVersion": SCHEMA_VERSION,
            "status": "failed",
            "command": args.command,
            "error": {"code": "release_tool_internal_error", "message": str(exc)},
        }
        exit_code = 1
    report_path = getattr(args, "report", None)
    if report_path:
        _write_json(Path(report_path), result)
    print(json.dumps(result, ensure_ascii=False, separators=(",", ":")))
    return exit_code


if __name__ == "__main__":
    sys.exit(main())
