"""Deterministic streaming facade for certificate-grade document validation."""

from __future__ import annotations

from dataclasses import dataclass
from hashlib import sha256
from importlib.metadata import PackageNotFoundError, version
import importlib.resources as pkg_resources
import json
from pathlib import Path, PurePosixPath
import stat
from typing import Any, Callable

import fastjsonschema

import tidas_tools.tidas as tidas_assets

DOCUMENT_VALIDATION_BATCH_PROTOCOL = "document-validation-batch.v1"
DOCUMENT_VALIDATION_PROFILE = "tidas-document-conformance.v1"
VALIDATION_REPORT_SCHEMA_VERSION = "tidas.validation-report.v1"
VALIDATION_ISSUE_EVENT_SCHEMA_VERSION = "tidas.validation-issue-event.v1"
VALIDATION_FINAL_EVENT_SCHEMA_VERSION = "tidas.validation-final-event.v1"


class BatchProtocolError(ValueError):
    """The caller violated the batch input or streaming protocol."""


@dataclass(frozen=True)
class BatchDocument:
    document_key: str
    category: str
    relative_path: str
    content_sha256: str
    dataset_type: str | None
    dataset_id: str | None
    dataset_version: str | None
    path: Path


def describe_document_validation() -> dict[str, Any]:
    """Return the reproducibility handshake consumed before a batch run."""

    return {
        "schema_version": "tidas.validation-describe.v1",
        "package": {"name": "tidas-tools", "version": _package_version()},
        "protocols": [DOCUMENT_VALIDATION_BATCH_PROTOCOL],
        "profiles": [DOCUMENT_VALIDATION_PROFILE],
        "report_schema_versions": [VALIDATION_REPORT_SCHEMA_VERSION],
        "event_schema_versions": [
            VALIDATION_ISSUE_EVENT_SCHEMA_VERSION,
            VALIDATION_FINAL_EVENT_SCHEMA_VERSION,
        ],
        "engines": {
            "python": _python_version(),
            "fastjsonschema": _distribution_version("fastjsonschema"),
            "jsonschema": _distribution_version("jsonschema"),
        },
        "tidas_schema_lock_sha256": _schema_lock_sha256(),
    }


def run_document_validation_batch(
    *,
    input_dir: str | Path,
    input_manifest: str | Path,
    profile: str = DOCUMENT_VALIDATION_PROFILE,
    emit_event: Callable[[dict[str, Any]], None],
) -> dict[str, Any]:
    """Validate exactly the manifest documents and stream issue/final events.

    Data issues are a successful protocol run.  Invalid manifests, unsafe
    paths, content drift, and validator failures raise ``BatchProtocolError``
    or the underlying system exception and therefore never emit a final event.
    """

    if profile != DOCUMENT_VALIDATION_PROFILE:
        raise BatchProtocolError(f"unsupported document validation profile: {profile}")

    from .validate import _build_tidas_validator, _collect_tidas_file_issues

    root = Path(input_dir).resolve(strict=True)
    if not root.is_dir():
        raise BatchProtocolError("--input-dir must be a directory")
    documents = load_batch_manifest(root, input_manifest)
    validators = {}
    logical_hasher = sha256()
    issue_count = 0
    error_count = 0
    warning_count = 0
    info_count = 0

    for document_ordinal, document in enumerate(documents):
        validator = validators.get(document.category)
        if validator is None:
            validator = _build_tidas_validator(document.category)
            validators[document.category] = validator
        issues = _collect_tidas_file_issues(
            str(document.path), document.category, validator
        )
        for issue_ordinal, issue in enumerate(issues):
            issue_payload = issue.to_dict()
            issue_payload["file_path"] = document.relative_path
            event = {
                "type": "issue",
                "schema_version": VALIDATION_ISSUE_EVENT_SCHEMA_VERSION,
                "protocol": DOCUMENT_VALIDATION_BATCH_PROTOCOL,
                "profile": profile,
                "document_key": document.document_key,
                "document_ordinal": document_ordinal,
                "issue_ordinal": issue_ordinal,
                "identity": {
                    "dataset_type": document.dataset_type,
                    "dataset_id": document.dataset_id,
                    "dataset_version": document.dataset_version,
                    "content_sha256": document.content_sha256,
                },
                "issue": issue_payload,
            }
            encoded = canonical_json_line(event)
            logical_hasher.update(encoded)
            emit_event(event)
            issue_count += 1
            if issue.severity == "error":
                error_count += 1
            elif issue.severity == "warning":
                warning_count += 1
            else:
                info_count += 1

    describe = describe_document_validation()
    final_event = {
        "type": "final",
        "schema_version": VALIDATION_FINAL_EVENT_SCHEMA_VERSION,
        "protocol": DOCUMENT_VALIDATION_BATCH_PROTOCOL,
        "profile": profile,
        "completed": True,
        "summary": {
            "document_count": len(documents),
            "issue_count": issue_count,
            "error_count": error_count,
            "warning_count": warning_count,
            "info_count": info_count,
        },
        "logical_issue_stream_sha256": logical_hasher.hexdigest(),
        "fingerprints": describe,
    }
    emit_event(final_event)
    return final_event


def load_batch_manifest(root: Path, input_manifest: str | Path) -> list[BatchDocument]:
    manifest_path = Path(input_manifest)
    seen_keys: set[str] = set()
    seen_paths: set[str] = set()
    documents: list[BatchDocument] = []

    with manifest_path.open("r", encoding="utf-8") as manifest_file:
        for line_number, raw_line in enumerate(manifest_file, start=1):
            if not raw_line.strip():
                continue
            try:
                item = json.loads(raw_line)
            except json.JSONDecodeError as error:
                raise BatchProtocolError(
                    f"manifest line {line_number} is not valid JSON: {error}"
                ) from error
            if not isinstance(item, dict):
                raise BatchProtocolError(
                    f"manifest line {line_number} must be a JSON object"
                )

            document_key = _required_string(item, "document_key", line_number)
            category = _required_string(item, "category", line_number).lower()
            relative_path = _required_string(item, "relative_path", line_number)
            content_sha256 = _required_string(
                item, "content_sha256", line_number
            ).lower()
            if category not in _supported_categories():
                raise BatchProtocolError(
                    f"manifest line {line_number} has unsupported category: {category}"
                )
            if not _is_sha256(content_sha256):
                raise BatchProtocolError(
                    f"manifest line {line_number} content_sha256 is invalid"
                )
            if document_key in seen_keys:
                raise BatchProtocolError(f"duplicate document_key: {document_key}")
            normalized_relative = _normalize_relative_path(relative_path, line_number)
            if normalized_relative in seen_paths:
                raise BatchProtocolError(
                    f"duplicate relative_path: {normalized_relative}"
                )

            path = _safe_regular_file(root, normalized_relative, line_number)
            actual_sha256 = sha256(path.read_bytes()).hexdigest()
            if actual_sha256 != content_sha256:
                raise BatchProtocolError(
                    f"content hash mismatch for {normalized_relative}: "
                    f"expected {content_sha256}, got {actual_sha256}"
                )

            seen_keys.add(document_key)
            seen_paths.add(normalized_relative)
            identity = item.get("identity") or {}
            if not isinstance(identity, dict):
                raise BatchProtocolError(
                    f"manifest line {line_number} identity must be an object"
                )
            documents.append(
                BatchDocument(
                    document_key=document_key,
                    category=category,
                    relative_path=normalized_relative,
                    content_sha256=content_sha256,
                    dataset_type=_optional_string(
                        identity, "dataset_type", line_number
                    ),
                    dataset_id=_optional_string(identity, "dataset_id", line_number),
                    dataset_version=_optional_string(
                        identity, "dataset_version", line_number
                    ),
                    path=path,
                )
            )

    return documents


def canonical_json_line(payload: dict[str, Any]) -> bytes:
    return (
        json.dumps(
            payload,
            ensure_ascii=False,
            sort_keys=True,
            separators=(",", ":"),
        )
        + "\n"
    ).encode("utf-8")


def _required_string(item: dict[str, Any], key: str, line_number: int) -> str:
    value = item.get(key)
    if not isinstance(value, str) or not value:
        raise BatchProtocolError(
            f"manifest line {line_number} requires non-empty string {key}"
        )
    return value


def _optional_string(item: dict[str, Any], key: str, line_number: int) -> str | None:
    value = item.get(key)
    if value is None:
        return None
    if not isinstance(value, str) or not value:
        raise BatchProtocolError(
            f"manifest line {line_number} {key} must be a non-empty string or null"
        )
    return value


def _normalize_relative_path(relative_path: str, line_number: int) -> str:
    posix_path = PurePosixPath(relative_path)
    if posix_path.is_absolute() or not posix_path.parts:
        raise BatchProtocolError(
            f"manifest line {line_number} relative_path must be relative"
        )
    if any(part in {"", ".", ".."} for part in posix_path.parts):
        raise BatchProtocolError(
            f"manifest line {line_number} relative_path escapes the batch root"
        )
    return str(posix_path)


def _safe_regular_file(root: Path, relative_path: str, line_number: int) -> Path:
    current = root
    for part in PurePosixPath(relative_path).parts:
        current = current / part
        if current.is_symlink():
            raise BatchProtocolError(
                f"manifest line {line_number} references a symlink: {relative_path}"
            )
    try:
        metadata = current.stat()
    except FileNotFoundError as error:
        raise BatchProtocolError(
            f"manifest line {line_number} file does not exist: {relative_path}"
        ) from error
    if not stat.S_ISREG(metadata.st_mode):
        raise BatchProtocolError(
            f"manifest line {line_number} path is not a regular file: {relative_path}"
        )
    resolved = current.resolve(strict=True)
    try:
        resolved.relative_to(root)
    except ValueError as error:
        raise BatchProtocolError(
            f"manifest line {line_number} path escapes the batch root: {relative_path}"
        ) from error
    return resolved


def _supported_categories() -> frozenset[str]:
    from .validate import SUPPORTED_CATEGORIES

    return frozenset(SUPPORTED_CATEGORIES)


def _schema_lock_sha256() -> str:
    schema_lock = pkg_resources.files(tidas_assets) / "schema.lock.json"
    return sha256(schema_lock.read_bytes()).hexdigest()


def _package_version() -> str:
    return _distribution_version("tidas-tools")


def _distribution_version(distribution: str) -> str:
    try:
        return version(distribution)
    except PackageNotFoundError:
        return "0+unknown"


def _python_version() -> str:
    import platform

    return platform.python_version()


def _is_sha256(value: str) -> bool:
    return len(value) == 64 and all(
        character in "0123456789abcdef" for character in value
    )
