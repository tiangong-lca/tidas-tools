"""Materialize per-process TIDAS dependency bundles."""

from __future__ import annotations

import json
from pathlib import Path, PurePosixPath
from shutil import copy2
from typing import Any

from .writers import TIDAS_CATEGORIES, ensure_tidas_package_dirs

PROCESS_BUNDLE_CATEGORIES = (
    "contacts",
    "sources",
    "unitgroups",
    "flowproperties",
    "flows",
    "processes",
)


def write_process_bundles(
    tidas_dir: str | Path, output_dir: str | Path
) -> dict[str, Any]:
    """Copy each process and its referenced TIDAS datasets into its own folder."""

    package_root = Path(tidas_dir)
    bundle_root = Path(output_dir)
    bundle_root.mkdir(parents=True, exist_ok=True)

    package_index = _index_package(package_root)
    process_ids = sorted(package_index["processes"])
    bundles: list[dict[str, Any]] = []
    unresolved_references: list[dict[str, Any]] = []

    for process_id in process_ids:
        dependencies, unresolved = _collect_dependencies(process_id, package_index)
        bundle_dir = bundle_root / process_id
        tidas_bundle_dir = ensure_tidas_package_dirs(bundle_dir / "tidas")
        files = _copy_dependencies(package_root, tidas_bundle_dir, dependencies)
        dependency_counts = {
            category: len(files.get(category, []))
            for category in PROCESS_BUNDLE_CATEGORIES
        }
        manifest = {
            "schema_version": 1,
            "process_id": process_id,
            "source_tidas_dir": str(package_root),
            "bundle_tidas_dir": "tidas",
            "dependency_counts": dependency_counts,
            "files": files,
            "unresolved_references": unresolved,
        }
        _write_json(bundle_dir / "manifest.json", manifest)
        bundles.append(
            {
                "process_id": process_id,
                "manifest": f"{process_id}/manifest.json",
                "tidas_dir": f"{process_id}/tidas",
                "dependency_counts": dependency_counts,
            }
        )
        unresolved_references.extend(
            {"process_id": process_id, **item} for item in unresolved
        )

    index_payload = {
        "schema_version": 1,
        "source_tidas_dir": str(package_root),
        "process_count": len(bundles),
        "bundles": bundles,
        "unresolved_references": unresolved_references,
    }
    _write_json(bundle_root / "index.json", index_payload)
    return {
        "process_count": len(bundles),
        "output_dir": str(bundle_root),
        "index_file": str(bundle_root / "index.json"),
        "bundles": bundles,
        "unresolved_references": unresolved_references,
    }


def _index_package(tidas_dir: Path) -> dict[str, dict[str, Path]]:
    index: dict[str, dict[str, Path]] = {}
    for category in TIDAS_CATEGORIES:
        category_dir = tidas_dir / category
        index[category] = {
            path.stem: path for path in sorted(category_dir.glob("*.json"))
        }
    return index


def _collect_dependencies(
    process_id: str, package_index: dict[str, dict[str, Path]]
) -> tuple[dict[str, set[str]], list[dict[str, Any]]]:
    dependencies: dict[str, set[str]] = {
        category: set() for category in PROCESS_BUNDLE_CATEGORIES
    }
    unresolved: list[dict[str, Any]] = []
    visited: set[tuple[str, str]] = set()
    queue: list[tuple[str, str, str]] = [("processes", process_id, "$")]

    while queue:
        category, dataset_id, ref_path = queue.pop(0)
        if category not in dependencies:
            continue
        if (category, dataset_id) in visited:
            continue
        visited.add((category, dataset_id))

        dataset_path = package_index.get(category, {}).get(dataset_id)
        if dataset_path is None:
            unresolved.append(
                {
                    "category": category,
                    "ref_id": dataset_id,
                    "path": ref_path,
                }
            )
            continue

        dependencies[category].add(dataset_id)
        payload = _read_json(dataset_path)
        for reference in _iter_references(payload):
            ref_category = reference.get("category")
            ref_id = reference.get("ref_id")
            if not isinstance(ref_category, str) or not isinstance(ref_id, str):
                continue
            if ref_category not in dependencies:
                continue
            queue.append((ref_category, ref_id, str(reference["path"])))

    return dependencies, unresolved


def _iter_references(payload: Any, path: str = "$"):
    if isinstance(payload, dict):
        ref_id = payload.get("@refObjectId")
        if isinstance(ref_id, str):
            category = _category_for_reference(payload)
            if category:
                yield {
                    "category": category,
                    "ref_id": ref_id,
                    "path": path,
                }
        for key, value in payload.items():
            yield from _iter_references(value, f"{path}.{key}")
    elif isinstance(payload, list):
        for index, item in enumerate(payload):
            yield from _iter_references(item, f"{path}[{index}]")


def _category_for_reference(reference: dict[str, Any]) -> str | None:
    ref_type = str(reference.get("@type") or "").lower()
    if "flow property" in ref_type:
        return "flowproperties"
    if "flow" in ref_type:
        return "flows"
    if "unit group" in ref_type:
        return "unitgroups"
    if "contact" in ref_type:
        return "contacts"
    if "source" in ref_type:
        return "sources"
    if "process" in ref_type:
        return "processes"

    uri = str(reference.get("@uri") or "")
    uri_parts = {part for part in uri.split("/") if part}
    for category in PROCESS_BUNDLE_CATEGORIES:
        if category in uri_parts:
            return category
    return None


def _copy_dependencies(
    package_root: Path, bundle_tidas_dir: Path, dependencies: dict[str, set[str]]
) -> dict[str, list[str]]:
    files: dict[str, list[str]] = {}
    for category in PROCESS_BUNDLE_CATEGORIES:
        files[category] = []
        for dataset_id in sorted(dependencies[category]):
            source = package_root / category / f"{dataset_id}.json"
            destination = bundle_tidas_dir / category / source.name
            destination.parent.mkdir(parents=True, exist_ok=True)
            copy2(source, destination)
            files[category].append(str(PurePosixPath("tidas") / category / source.name))
    return files


def _read_json(path: Path) -> Any:
    return json.loads(path.read_text(encoding="utf-8"))


def _write_json(path: Path, payload: dict[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(payload, indent=2, ensure_ascii=False) + "\n")
