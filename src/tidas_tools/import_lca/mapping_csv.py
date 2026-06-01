"""Mapping CSV writer for expert review of import results."""

from __future__ import annotations

import csv
from dataclasses import dataclass
from decimal import Decimal
import json
from pathlib import Path
from typing import Any, Iterable

from .model import CanonicalEntity
from .store import MemoryCanonicalStore

PLACEHOLDER_PREFIX = "TIDAS_IMPORT_PLACEHOLDER:"
SOURCE_GAP_MARKERS = {
    "not declared in source package": "SOURCE_VALUE_NOT_DECLARED",
    "source package metadata not declared": "SOURCE_METADATA_NOT_DECLARED",
    "compliance system not declared in source package": "COMPLIANCE_NOT_DECLARED",
}
TRACE_MARKER = "TIDAS_IMPORT_TRACE_V1"

MAPPING_CSV_COLUMNS = [
    "row_id",
    "source_format",
    "source_object",
    "source_entity_type",
    "source_entity_id",
    "source_entity_name",
    "source_exchange_id",
    "source_field_path",
    "source_field_name",
    "source_value",
    "target_dataset_type",
    "target_dataset_id",
    "target_dataset_name",
    "target_file",
    "target_scope",
    "target_field_path",
    "target_field_name",
    "target_value",
    "mapping_status",
    "mapping_category",
    "placeholder_kind",
    "trace_marker",
    "needs_review",
    "reviewer_notes",
]

ROOT_KEYS = {
    "contacts": "contactDataSet",
    "flowproperties": "flowPropertyDataSet",
    "flows": "flowDataSet",
    "lifecyclemodels": "lifeCycleModelDataSet",
    "processes": "processDataSet",
    "sources": "sourceDataSet",
    "unitgroups": "unitGroupDataSet",
}


@dataclass(frozen=True)
class _SourceMatch:
    path: str
    value: str


def write_mapping_csv(
    store: MemoryCanonicalStore,
    tidas_dir: str | Path,
    output_path: str | Path,
    *,
    source_format: str | None = None,
) -> int:
    """Write a stable mapping CSV for generated TIDAS JSON datasets."""

    root = Path(tidas_dir)
    path = Path(output_path)
    path.parent.mkdir(parents=True, exist_ok=True)
    entities = _entities_by_id(store)

    row_count = 0
    with path.open("w", newline="", encoding="utf-8") as stream:
        writer = csv.DictWriter(stream, fieldnames=MAPPING_CSV_COLUMNS)
        writer.writeheader()
        for index, row in enumerate(
            _mapping_rows(root, entities, source_format=source_format), start=1
        ):
            row_count = index
            writer.writerow(
                {
                    column: str(row.get(column, "") or "")
                    for column in MAPPING_CSV_COLUMNS
                    if column != "row_id"
                }
                | {"row_id": str(index)}
            )
    return row_count


def _mapping_rows(
    root: Path,
    entities: dict[tuple[str, str], CanonicalEntity],
    *,
    source_format: str | None,
) -> Iterable[dict[str, str]]:
    for dataset_dir in sorted(path for path in root.iterdir() if path.is_dir()):
        dataset_type = dataset_dir.name
        root_key = ROOT_KEYS.get(dataset_type)
        if root_key is None:
            continue
        for file_path in sorted(dataset_dir.glob("*.json")):
            payload = json.loads(file_path.read_text(encoding="utf-8"))
            dataset = payload.get(root_key)
            if not isinstance(dataset, dict):
                continue
            dataset_id = file_path.stem
            entity = entities.get((dataset_type, dataset_id))
            source_index = _source_value_index(entity)
            source_trace = entity.raw.get("sourceTrace") if entity else {}
            source_format_value = (
                _source_format(entity)
                or source_format
                or _trace_value(source_trace, "format")
            )
            source_object = _trace_value(source_trace, "sourceObject")
            context = {
                "source_format": source_format_value,
                "source_object": source_object,
                "source_entity_type": dataset_type,
                "source_entity_id": (
                    entity.external_id if entity and entity.external_id else dataset_id
                ),
                "source_entity_name": entity.name if entity and entity.name else "",
                "target_dataset_type": dataset_type,
                "target_dataset_id": dataset_id,
                "target_dataset_name": _dataset_name(dataset),
                "target_file": str(file_path.relative_to(root.parent)),
            }
            for target_path, value in _flatten(dataset):
                if _skip_target_path(target_path):
                    continue
                value_text = _value_text(value)
                status = _mapping_status(target_path, value_text, entity)
                source_match = _source_match(
                    status=status,
                    target_path=target_path,
                    target_value=value_text,
                    source_index=source_index,
                )
                placeholder_kind = _placeholder_kind(value_text)
                yield {
                    **context,
                    "source_exchange_id": _source_exchange_id(entity, target_path),
                    "source_field_path": source_match.path if source_match else "",
                    "source_field_name": _field_name(
                        source_match.path if source_match else ""
                    ),
                    "source_value": source_match.value if source_match else "",
                    "target_scope": _target_scope(target_path),
                    "target_field_path": target_path,
                    "target_field_name": _field_name(target_path),
                    "target_value": value_text,
                    "mapping_status": status,
                    "mapping_category": _mapping_category(
                        target_path, source_match.path if source_match else ""
                    ),
                    "placeholder_kind": placeholder_kind,
                    "trace_marker": TRACE_MARKER if _is_trace_path(target_path) else "",
                    "needs_review": _needs_review(status, placeholder_kind),
                    "reviewer_notes": "",
                }


def _entities_by_id(
    store: MemoryCanonicalStore,
) -> dict[tuple[str, str], CanonicalEntity]:
    entities: dict[tuple[str, str], CanonicalEntity] = {}
    for entity_type in ROOT_KEYS:
        for entity in store.iter_type(entity_type):
            entities[(entity_type, entity.internal_id)] = entity
    return entities


def _flatten(value: Any, prefix: str = "") -> Iterable[tuple[str, Any]]:
    if isinstance(value, dict):
        for key, item in value.items():
            path = f"{prefix}.{key}" if prefix else str(key)
            yield from _flatten(item, path)
        return
    if isinstance(value, list):
        for index, item in enumerate(value):
            yield from _flatten(item, f"{prefix}[{index}]")
        return
    yield prefix, value


def _skip_target_path(path: str) -> bool:
    parts = path.split(".")
    return any(
        part.startswith("@xmlns")
        or part in {"@xsi:schemaLocation", "@locations", "@version"}
        for part in parts
    )


def _mapping_status(path: str, value: str, entity: CanonicalEntity | None) -> str:
    if _placeholder_kind(value):
        return "placeholder"
    if _is_trace_path(path):
        return "trace_only"
    if entity is None:
        return "generated"
    if _is_generated_path(path):
        return "generated"
    return "formal"


def _source_match(
    *,
    status: str,
    target_path: str,
    target_value: str,
    source_index: dict[str, list[_SourceMatch]],
) -> _SourceMatch | None:
    if status == "trace_only":
        return _SourceMatch(
            path=_trace_source_path(target_path),
            value=target_value,
        )
    if status == "generated":
        return None
    matches = source_index.get(target_value)
    if matches:
        return matches[0]
    return None


def _source_value_index(
    entity: CanonicalEntity | None,
) -> dict[str, list[_SourceMatch]]:
    if entity is None:
        return {}
    index: dict[str, list[_SourceMatch]] = {}
    for path, value in _flatten(entity.raw):
        value_text = _value_text(value)
        if not value_text:
            continue
        index.setdefault(value_text, []).append(
            _SourceMatch(path=path, value=value_text)
        )
    for matches in index.values():
        matches.sort(key=lambda match: (path_priority(match.path), match.path))
    return index


def path_priority(path: str) -> int:
    if path.startswith("sourceTrace."):
        return 3
    if "sourceTrace." in path:
        return 2
    if path.startswith("exchanges["):
        return 1
    return 0


def _trace_source_path(target_path: str) -> str:
    marker = ".tidasimport:sourceTrace.payload."
    if marker in target_path:
        return target_path.split(marker, 1)[1]
    marker = ".tidasimport:sourceTrace."
    if marker in target_path:
        return target_path.split(marker, 1)[1]
    return target_path


def _is_trace_path(path: str) -> bool:
    return ".common:other.tidasimport:sourceTrace" in f".{path}"


def _is_generated_path(path: str) -> bool:
    generated_markers = (
        ".administrativeInformation.",
        ".common:referenceToDataSetFormat.",
        ".common:permanentDataSetURI",
        ".common:referenceToComplianceSystem.",
        ".common:referenceToCommissioner.",
        ".common:referenceToPersonOrEntity",
    )
    return any(marker in f".{path}" for marker in generated_markers)


def _target_scope(path: str) -> str:
    if ".exchanges.exchange[" in path or path.startswith("exchanges.exchange["):
        return "exchange"
    if _is_trace_path(path):
        return "trace"
    if ".administrativeInformation." in f".{path}":
        return "administrative"
    return "dataset"


def _mapping_category(target_path: str, source_path: str) -> str:
    path = f"{target_path}.{source_path}".casefold()
    categories = (
        ("provider", "provider_link"),
        ("activitylinkid", "provider_link"),
        ("classification", "classification"),
        ("category", "classification"),
        ("casnumber", "cas"),
        ("sumformula", "chemical_identity"),
        ("location", "geography"),
        ("geography", "geography"),
        ("time", "time"),
        ("referenceyear", "time"),
        ("validuntil", "time"),
        ("technology", "technology"),
        ("review", "review"),
        ("source", "source"),
        ("amount", "amount"),
        ("exchange", "exchange"),
        ("flow", "flow"),
        ("name", "name"),
        ("uuid", "identity"),
    )
    for token, category in categories:
        if token in path:
            return category
    return "metadata"


def _source_exchange_id(entity: CanonicalEntity | None, target_path: str) -> str:
    if entity is None:
        return ""
    exchange_index = _exchange_index(target_path)
    if exchange_index is None:
        return ""
    exchanges = entity.raw.get("exchanges")
    if not isinstance(exchanges, list) or exchange_index >= len(exchanges):
        return str(exchange_index + 1)
    exchange = exchanges[exchange_index]
    if not isinstance(exchange, dict):
        return str(exchange_index + 1)
    for key in ("sourceExchangeNumber", "sourceExchangeId", "internalId"):
        if exchange.get(key) is not None:
            return str(exchange[key])
    return str(exchange_index + 1)


def _exchange_index(path: str) -> int | None:
    marker = "exchanges.exchange["
    if marker not in path:
        return None
    tail = path.split(marker, 1)[1]
    index_text = tail.split("]", 1)[0]
    try:
        return int(index_text)
    except ValueError:
        return None


def _needs_review(status: str, placeholder_kind: str) -> str:
    if status in {"trace_only", "placeholder"} or placeholder_kind:
        return "TRUE"
    if status == "generated":
        return "TRUE"
    return "FALSE"


def _placeholder_kind(value: str) -> str:
    if PLACEHOLDER_PREFIX not in value:
        normalized = value.casefold()
        for marker, kind in SOURCE_GAP_MARKERS.items():
            if marker in normalized:
                return kind
        return ""
    return value.split(PLACEHOLDER_PREFIX, 1)[1].split()[0].strip(".,;:)")


def _source_format(entity: CanonicalEntity | None) -> str:
    if entity is None:
        return ""
    source_trace = entity.raw.get("sourceTrace")
    if isinstance(source_trace, dict) and source_trace.get("format"):
        return str(source_trace["format"])
    if entity.raw.get("sourceFormat"):
        return str(entity.raw["sourceFormat"])
    return ""


def _trace_value(trace: Any, key: str) -> str:
    if isinstance(trace, dict) and trace.get(key) is not None:
        return str(trace[key])
    return ""


def _dataset_name(dataset: dict[str, Any]) -> str:
    for path, value in _flatten(dataset):
        if path.endswith(".name.baseName.#text") or path.endswith(".shortName.#text"):
            text = _value_text(value)
            if text:
                return text
    return ""


def _field_name(path: str) -> str:
    if not path:
        return ""
    field = path.rsplit(".", 1)[-1]
    if "]" in field:
        field = field.rsplit("]", 1)[-1].lstrip(".")
    return field


def _value_text(value: Any) -> str:
    if value is None:
        return ""
    if isinstance(value, bool):
        return "true" if value else "false"
    if isinstance(value, (str, int, float, Decimal)):
        return str(value)
    return json.dumps(value, ensure_ascii=False, sort_keys=True)
