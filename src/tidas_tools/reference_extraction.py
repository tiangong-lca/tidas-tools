"""Versioned, side-effect-free extraction of TIDAS document references.

This module intentionally does not resolve references.  It preserves the
constraint written in the source document so a caller can apply its own
visibility, exact-version, and closure policy.
"""

from __future__ import annotations

from dataclasses import asdict, dataclass
import re
from typing import Any
from uuid import UUID

REFERENCE_EXTRACTION_SCHEMA_VERSION = "tidas.reference-extraction-result.v1"
REFERENCE_EDGE_SCHEMA_VERSION = "tidas.reference-edge.v1"
REFERENCE_ISSUE_SCHEMA_VERSION = "tidas.reference-extraction-issue.v1"

REFERENCE_ROLE_PROCESS_EXCHANGE_FLOW = "process_exchange_flow"
REFERENCE_ROLE_LCIA_FACTOR_FLOW = "lcia_factor_flow"
REFERENCE_ROLE_LIFECYCLE_MODEL_PROCESS = "lifecycle_model_process"
REFERENCE_ROLE_SUPPORT_DOCUMENT = "support_document"

REFERENCE_ROLES = (
    REFERENCE_ROLE_LCIA_FACTOR_FLOW,
    REFERENCE_ROLE_LIFECYCLE_MODEL_PROCESS,
    REFERENCE_ROLE_PROCESS_EXCHANGE_FLOW,
    REFERENCE_ROLE_SUPPORT_DOCUMENT,
)

_VERSION_RE = re.compile(r"^\d{2}\.\d{2}(?:\.\d{3})?$")

_TYPE_TO_CATEGORY = {
    "contact": "contacts",
    "contact data set": "contacts",
    "flow": "flows",
    "flow data set": "flows",
    "flow property": "flowproperties",
    "flow property data set": "flowproperties",
    "lcia method": "lciamethods",
    "lcia method data set": "lciamethods",
    "life cycle model": "lifecyclemodels",
    "life cycle model data set": "lifecyclemodels",
    "lifecycle model": "lifecyclemodels",
    "lifecycle model data set": "lifecyclemodels",
    "process": "processes",
    "process data set": "processes",
    "source": "sources",
    "source data set": "sources",
    "unit group": "unitgroups",
    "unit group data set": "unitgroups",
}

_URI_CATEGORY_ALIASES = {
    "contacts": "contacts",
    "contact": "contacts",
    "flows": "flows",
    "flow": "flows",
    "flowproperties": "flowproperties",
    "flowproperty": "flowproperties",
    "flow-properties": "flowproperties",
    "lciamethods": "lciamethods",
    "lciamethod": "lciamethods",
    "lcia-methods": "lciamethods",
    "lifecyclemodels": "lifecyclemodels",
    "lifecyclemodel": "lifecyclemodels",
    "life-cycle-models": "lifecyclemodels",
    "processes": "processes",
    "process": "processes",
    "sources": "sources",
    "source": "sources",
    "unitgroups": "unitgroups",
    "unitgroup": "unitgroups",
    "unit-groups": "unitgroups",
}


@dataclass(frozen=True)
class ReferenceEdgeV1:
    schema_version: str
    document_key: str
    source_category: str
    target_category: str
    target_uuid: str
    requested_version_state: str
    requested_version: str | None
    requested_version_raw: Any
    reference_role: str
    json_path: str
    raw_type: Any
    uri: Any

    def to_dict(self) -> dict[str, Any]:
        return asdict(self)


@dataclass(frozen=True)
class ReferenceExtractionIssueV1:
    schema_version: str
    issue_code: str
    severity: str
    document_key: str
    source_category: str
    json_path: str
    reference_role: str
    message: str
    details: dict[str, Any]

    def to_dict(self) -> dict[str, Any]:
        return asdict(self)


@dataclass(frozen=True)
class ReferenceExtractionResultV1:
    schema_version: str
    document_key: str
    source_category: str
    edges: tuple[ReferenceEdgeV1, ...]
    issues: tuple[ReferenceExtractionIssueV1, ...]

    def to_dict(self) -> dict[str, Any]:
        return {
            "schema_version": self.schema_version,
            "document_key": self.document_key,
            "source_category": self.source_category,
            "edges": [edge.to_dict() for edge in self.edges],
            "issues": [issue.to_dict() for issue in self.issues],
        }


def extract_references(
    document_key: str, category: str, payload: Any
) -> ReferenceExtractionResultV1:
    """Extract all recognizable references and all extraction defects.

    The output order is document order and is deterministic for a given JSON
    value.  Missing targets are deliberately not reported here because target
    existence is a resolver concern, not a document-extraction concern.
    """

    if not document_key:
        raise ValueError("document_key must be non-empty")
    if not category:
        raise ValueError("category must be non-empty")

    edges: list[ReferenceEdgeV1] = []
    issues: list[ReferenceExtractionIssueV1] = []
    _walk_references(
        payload,
        document_key=document_key,
        source_category=category,
        path="$",
        parent_key=None,
        edges=edges,
        issues=issues,
    )
    return ReferenceExtractionResultV1(
        schema_version=REFERENCE_EXTRACTION_SCHEMA_VERSION,
        document_key=document_key,
        source_category=category,
        edges=tuple(edges),
        issues=tuple(issues),
    )


def _walk_references(
    node: Any,
    *,
    document_key: str,
    source_category: str,
    path: str,
    parent_key: str | None,
    edges: list[ReferenceEdgeV1],
    issues: list[ReferenceExtractionIssueV1],
) -> None:
    if isinstance(node, dict):
        if _looks_like_reference(node, parent_key):
            _extract_reference(
                node,
                document_key=document_key,
                source_category=source_category,
                path=path,
                parent_key=parent_key,
                edges=edges,
                issues=issues,
            )
        for key, value in node.items():
            _walk_references(
                value,
                document_key=document_key,
                source_category=source_category,
                path=f"{path}.{key}",
                parent_key=key,
                edges=edges,
                issues=issues,
            )
    elif isinstance(node, list):
        for index, item in enumerate(node):
            _walk_references(
                item,
                document_key=document_key,
                source_category=source_category,
                path=f"{path}[{index}]",
                parent_key=parent_key,
                edges=edges,
                issues=issues,
            )


def _looks_like_reference(node: dict[str, Any], parent_key: str | None) -> bool:
    if "@refObjectId" in node or "@uri" in node:
        return True
    if parent_key and "referenceto" in parent_key.lower():
        return True
    return False


def _extract_reference(
    reference: dict[str, Any],
    *,
    document_key: str,
    source_category: str,
    path: str,
    parent_key: str | None,
    edges: list[ReferenceEdgeV1],
    issues: list[ReferenceExtractionIssueV1],
) -> None:
    raw_type = reference.get("@type")
    uri = reference.get("@uri")
    target_category = _category_from_type(raw_type) or _category_from_uri(uri)
    role = _reference_role(source_category, path, parent_key, target_category)

    if target_category is None:
        issues.append(
            _issue(
                "reference_type_unresolved",
                document_key,
                source_category,
                path,
                role,
                "Reference target type cannot be resolved from @type or @uri.",
                {"raw_type": raw_type, "uri": uri},
            )
        )

    raw_id = reference.get("@refObjectId")
    if not isinstance(raw_id, str) or not raw_id.strip():
        issues.append(
            _issue(
                "reference_object_id_missing",
                document_key,
                source_category,
                path,
                role,
                "Recognized reference is missing a non-empty @refObjectId.",
                {"raw_ref_object_id": raw_id, "raw_type": raw_type, "uri": uri},
            )
        )
        return

    target_uuid = raw_id.strip()
    if not _is_canonical_uuid(target_uuid):
        issues.append(
            _issue(
                "reference_uuid_invalid",
                document_key,
                source_category,
                path,
                role,
                "Reference @refObjectId is not a canonical lowercase UUID.",
                {"raw_ref_object_id": raw_id},
            )
        )

    raw_version = reference.get("@version")
    if raw_version is None:
        version_state = "omitted"
        requested_version = None
    elif isinstance(raw_version, str) and _VERSION_RE.fullmatch(raw_version):
        version_state = "explicit"
        requested_version = raw_version
    else:
        version_state = "invalid"
        requested_version = raw_version if isinstance(raw_version, str) else None
        issues.append(
            _issue(
                "reference_version_invalid",
                document_key,
                source_category,
                path,
                role,
                "Reference @version must match NN.NN or NN.NN.NNN.",
                {"requested_version_raw": raw_version},
            )
        )

    if target_category is not None:
        edges.append(
            ReferenceEdgeV1(
                schema_version=REFERENCE_EDGE_SCHEMA_VERSION,
                document_key=document_key,
                source_category=source_category,
                target_category=target_category,
                target_uuid=target_uuid,
                requested_version_state=version_state,
                requested_version=requested_version,
                requested_version_raw=raw_version,
                reference_role=role,
                json_path=path,
                raw_type=raw_type,
                uri=uri,
            )
        )


def _issue(
    issue_code: str,
    document_key: str,
    source_category: str,
    json_path: str,
    reference_role: str,
    message: str,
    details: dict[str, Any],
) -> ReferenceExtractionIssueV1:
    return ReferenceExtractionIssueV1(
        schema_version=REFERENCE_ISSUE_SCHEMA_VERSION,
        issue_code=issue_code,
        severity="error",
        document_key=document_key,
        source_category=source_category,
        json_path=json_path,
        reference_role=reference_role,
        message=message,
        details=details,
    )


def _category_from_type(raw_type: Any) -> str | None:
    if not isinstance(raw_type, str):
        return None
    return _TYPE_TO_CATEGORY.get(" ".join(raw_type.strip().lower().split()))


def _category_from_uri(uri: Any) -> str | None:
    if not isinstance(uri, str):
        return None
    parts = [part.lower() for part in re.split(r"[/\\]", uri) if part]
    for part in parts:
        category = _URI_CATEGORY_ALIASES.get(part)
        if category:
            return category
    return None


def _reference_role(
    source_category: str,
    path: str,
    parent_key: str | None,
    target_category: str | None,
) -> str:
    normalized_path = path.lower()
    normalized_key = (parent_key or "").lower()
    if (
        source_category == "processes"
        and target_category == "flows"
        and "exchange" in normalized_path
        and normalized_key == "referencetoflowdataset"
    ):
        return REFERENCE_ROLE_PROCESS_EXCHANGE_FLOW
    if (
        source_category == "lciamethods"
        and target_category == "flows"
        and (
            "characterisation" in normalized_path
            or "characterization" in normalized_path
        )
    ):
        return REFERENCE_ROLE_LCIA_FACTOR_FLOW
    if source_category == "lifecyclemodels" and target_category == "processes":
        return REFERENCE_ROLE_LIFECYCLE_MODEL_PROCESS
    return REFERENCE_ROLE_SUPPORT_DOCUMENT


def _is_canonical_uuid(value: str) -> bool:
    try:
        return str(UUID(value)) == value
    except (ValueError, AttributeError):
        return False
