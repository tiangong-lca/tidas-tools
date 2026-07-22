"""Materialize per-process TIDAS dependency bundles."""

from __future__ import annotations

from collections import deque
import json
from pathlib import Path, PurePosixPath
from shutil import copy2
from typing import Any

from tidas_tools.reference_extraction import extract_references

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
    queue = deque([("processes", process_id, "$")])

    while queue:
        category, dataset_id, ref_path = queue.popleft()
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
        extraction = extract_references(
            document_key=f"{category}:{dataset_id}",
            category=category,
            payload=payload,
        )
        unresolved.extend(
            {
                "category": None,
                "ref_id": None,
                "path": issue.json_path,
                "issue_code": issue.issue_code,
                "reference_role": issue.reference_role,
                "details": issue.details,
            }
            for issue in extraction.issues
        )
        for edge in extraction.edges:
            ref_category = edge.target_category
            ref_id = edge.target_uuid
            if ref_category not in dependencies:
                continue
            queue.append((ref_category, ref_id, edge.json_path))

    return dependencies, unresolved


def _iter_references(payload: Any, path: str = "$"):
    """Compatibility iterator backed by the versioned extraction contract."""

    result = extract_references("process-bundle-adapter", "processes", payload)
    for edge in result.edges:
        edge_path = edge.json_path
        if path != "$" and edge_path.startswith("$"):
            edge_path = f"{path}{edge_path[1:]}"
        yield {
            "category": edge.target_category,
            "ref_id": edge.target_uuid,
            "version": edge.requested_version,
            "version_state": edge.requested_version_state,
            "reference_role": edge.reference_role,
            "path": edge_path,
        }


def _category_for_reference(reference: dict[str, Any]) -> str | None:
    result = extract_references("process-bundle-adapter", "processes", reference)
    return result.edges[0].target_category if result.edges else None


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
