from __future__ import annotations

import json
import logging
import re
from dataclasses import dataclass
from pathlib import Path

SUPPORTED_VERSIONED_CATEGORIES = (
    "contacts",
    "flowproperties",
    "flows",
    "lciamethods",
    "lifecyclemodels",
    "processes",
    "sources",
    "unitgroups",
)

PRECEDING_REFERENCE_KEYS = frozenset(
    {
        "common:referenceToPrecedingDataSetVersion",
        "referenceToPrecedingDataSetVersion",
    }
)

TYPE_TO_CATEGORY = {
    "contact data set": "contacts",
    "flow property data set": "flowproperties",
    "flow data set": "flows",
    "lcia method data set": "lciamethods",
    "life cycle model data set": "lifecyclemodels",
    "lifecycle model data set": "lifecyclemodels",
    "process data set": "processes",
    "source data set": "sources",
    "unit group data set": "unitgroups",
    "compliance system": "sources",
    "ilcddataformatreference": "sources",
    "datasetformat": "sources",
    "ilcdformatglobalreference": "sources",
}

VERSIONED_FILE_RE = re.compile(
    r"^(?P<uuid>[0-9a-fA-F-]{36})_(?P<version>\d{2}\.\d{2}\.\d{3})\.json$"
)
VERSIONED_URI_RE = re.compile(
    r"(?P<uuid>[0-9a-fA-F-]{36})_(?P<version>\d{2}\.\d{2}\.\d{3})(?P<suffix>\.(?:xml|json))$"
)
UNVERSIONED_URI_RE = re.compile(
    r"(?P<uuid>[0-9a-fA-F-]{36})(?P<suffix>\.(?:xml|json))$"
)


@dataclass
class VersionedRecord:
    category: str
    uuid: str
    version: str
    path: Path
    payload: dict


def _normalize_reference_type(reference_type: str | None) -> str | None:
    if not reference_type:
        return None
    return TYPE_TO_CATEGORY.get(reference_type.strip().lower())


def _infer_reference_category(reference: dict) -> str | None:
    uri = reference.get("@uri") or ""
    for category in SUPPORTED_VERSIONED_CATEGORIES:
        if f"/{category}/" in uri or f"../{category}/" in uri:
            return category
    return _normalize_reference_type(reference.get("@type"))


def _infer_reference_target(reference: dict) -> tuple[str, str, str | None] | None:
    category = _infer_reference_category(reference)
    ref_uuid = reference.get("@refObjectId")
    version = reference.get("@version")
    uri = reference.get("@uri") or ""

    versioned_match = VERSIONED_URI_RE.search(uri)
    if versioned_match:
        ref_uuid = ref_uuid or versioned_match.group("uuid")
        version = version or versioned_match.group("version")
    else:
        unversioned_match = UNVERSIONED_URI_RE.search(uri)
        if unversioned_match:
            ref_uuid = ref_uuid or unversioned_match.group("uuid")

    if not category or not ref_uuid:
        return None

    return category, ref_uuid, version


def _rewrite_reference_uri(
    uri: str | None,
    *,
    ref_uuid: str,
    latest_version: str,
) -> str | None:
    if not uri:
        return uri

    versioned_match = VERSIONED_URI_RE.search(uri)
    if versioned_match and versioned_match.group("uuid") == ref_uuid:
        return VERSIONED_URI_RE.sub(
            f"{ref_uuid}_{latest_version}\\g<suffix>",
            uri,
            count=1,
        )

    return uri


def _load_versioned_records(package_dir: Path) -> list[VersionedRecord]:
    records: list[VersionedRecord] = []

    for category in SUPPORTED_VERSIONED_CATEGORIES:
        category_dir = package_dir / category
        if not category_dir.is_dir():
            continue

        for path in sorted(category_dir.glob("*.json")):
            match = VERSIONED_FILE_RE.match(path.name)
            if not match:
                continue

            with path.open(encoding="utf-8") as handle:
                payload = json.load(handle)

            records.append(
                VersionedRecord(
                    category=category,
                    uuid=match.group("uuid"),
                    version=match.group("version"),
                    path=path,
                    payload=payload,
                )
            )

    return records


def _build_versions_by_dataset(
    records: list[VersionedRecord],
) -> dict[tuple[str, str], list[str]]:
    versions_by_dataset: dict[tuple[str, str], list[str]] = {}

    for record in records:
        key = (record.category, record.uuid)
        versions_by_dataset.setdefault(key, []).append(record.version)

    for versions in versions_by_dataset.values():
        versions.sort()

    return versions_by_dataset


def _normalize_payload_references(
    node,
    *,
    latest_versions: dict[tuple[str, str], str],
    versions_by_dataset: dict[tuple[str, str], list[str]],
    summary: dict[str, int],
) -> bool:
    changed = False

    if isinstance(node, dict):
        for key in list(node.keys()):
            value = node[key]

            if isinstance(value, dict) and "@refObjectId" in value:
                target = _infer_reference_target(value)
                if target is not None:
                    category, ref_uuid, version = target
                    dataset_key = (category, ref_uuid)
                    latest_version = latest_versions.get(dataset_key)
                    versions = versions_by_dataset.get(dataset_key, [])

                    if key in PRECEDING_REFERENCE_KEYS and len(versions) > 1:
                        del node[key]
                        summary["removed_preceding_references"] += 1
                        changed = True
                        continue

                    if (
                        latest_version is not None
                        and version is not None
                        and version != latest_version
                    ):
                        value["@version"] = latest_version
                        rewritten_uri = _rewrite_reference_uri(
                            value.get("@uri"),
                            ref_uuid=ref_uuid,
                            latest_version=latest_version,
                        )
                        if rewritten_uri is not None:
                            value["@uri"] = rewritten_uri
                        summary["updated_references"] += 1
                        changed = True

            if key in node:
                changed = _normalize_payload_references(
                    node[key],
                    latest_versions=latest_versions,
                    versions_by_dataset=versions_by_dataset,
                    summary=summary,
                ) or changed

    elif isinstance(node, list):
        for item in node:
            changed = _normalize_payload_references(
                item,
                latest_versions=latest_versions,
                versions_by_dataset=versions_by_dataset,
                summary=summary,
            ) or changed

    return changed


def normalize_package_versions(package_dir: str | Path) -> dict[str, int | str]:
    package_path = Path(package_dir)
    records = _load_versioned_records(package_path)
    versions_by_dataset = _build_versions_by_dataset(records)
    latest_versions = {
        dataset_key: versions[-1]
        for dataset_key, versions in versions_by_dataset.items()
    }

    summary: dict[str, int | str] = {
        "package_dir": str(package_path),
        "scanned_records": len(records),
        "dataset_count": len(versions_by_dataset),
        "duplicate_dataset_count": sum(
            1 for versions in versions_by_dataset.values() if len(versions) > 1
        ),
        "kept_records": 0,
        "removed_records": 0,
        "rewritten_files": 0,
        "updated_references": 0,
        "removed_preceding_references": 0,
    }

    kept_records: list[VersionedRecord] = []
    removed_records: list[VersionedRecord] = []

    for record in records:
        if record.version == latest_versions[(record.category, record.uuid)]:
            kept_records.append(record)
        else:
            removed_records.append(record)

    for record in kept_records:
        changed = _normalize_payload_references(
            record.payload,
            latest_versions=latest_versions,
            versions_by_dataset=versions_by_dataset,
            summary=summary,
        )
        if changed:
            with record.path.open("w", encoding="utf-8") as handle:
                json.dump(record.payload, handle, indent=2, ensure_ascii=False)
                handle.write("\n")
            summary["rewritten_files"] += 1

    for record in removed_records:
        record.path.unlink()

    summary["kept_records"] = len(kept_records)
    summary["removed_records"] = len(removed_records)

    logging.info(
        "Normalized package versions in %s: kept=%s removed=%s updated_refs=%s removed_preceding_refs=%s rewritten_files=%s",
        package_path,
        summary["kept_records"],
        summary["removed_records"],
        summary["updated_references"],
        summary["removed_preceding_references"],
        summary["rewritten_files"],
    )

    return summary
