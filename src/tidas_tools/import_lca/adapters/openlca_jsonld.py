"""openLCA JSON-LD source adapter."""

from __future__ import annotations

import json
from pathlib import Path
from typing import Any, Iterable
import uuid
import zipfile

from .base import SourceAdapter
from ..model import CanonicalEntity
from ..report import ConversionReport
from ..store import MemoryCanonicalStore


class OpenLcaJsonLdAdapter(SourceAdapter):
    """Read openLCA JSON-LD packages into the canonical store."""

    format_id = "openlca-jsonld"

    def read(
        self,
        source: str | Path,
        store: MemoryCanonicalStore,
        report: ConversionReport,
    ) -> None:
        count = 0
        for label, payload in _iter_json_payloads(Path(source)):
            for item in _iter_items(payload):
                if not isinstance(item, dict):
                    continue
                entity = _to_entity(item)
                if entity is None:
                    continue
                store.add(entity)
                count += 1
                report.add_issue(
                    severity="warning",
                    code="jsonld_minimal_mapping",
                    message=(
                        f"Mapped {entity.entity_type} from openLCA JSON-LD with "
                        "the current minimal adapter."
                    ),
                    source_object=label,
                    context={"entity_id": entity.internal_id},
                )

        if count == 0:
            report.add_issue(
                severity="error",
                code="no_supported_jsonld_entities",
                message="No supported openLCA JSON-LD entities were found.",
            )


def _iter_json_payloads(source: Path) -> Iterable[tuple[str, Any]]:
    if source.is_dir():
        for path in sorted(source.rglob("*.json")) + sorted(source.rglob("*.jsonld")):
            if path.name == "context.jsonld":
                continue
            yield str(path), _read_json_bytes(path.read_bytes())
        return

    if source.suffix.lower() == ".zip":
        with zipfile.ZipFile(source) as archive:
            for name in sorted(archive.namelist()):
                lower_name = name.lower()
                if name.endswith("/") or lower_name.endswith("context.jsonld"):
                    continue
                if not lower_name.endswith((".json", ".jsonld")):
                    continue
                with archive.open(name) as stream:
                    yield f"{source}:{name}", _read_json_bytes(stream.read())
        return

    yield str(source), _read_json_bytes(source.read_bytes())


def _read_json_bytes(data: bytes) -> Any:
    return json.loads(data.decode("utf-8-sig"))


def _iter_items(payload: Any) -> Iterable[dict[str, Any]]:
    if isinstance(payload, list):
        yield from payload
    elif isinstance(payload, dict):
        if "@graph" in payload and isinstance(payload["@graph"], list):
            yield from payload["@graph"]
        else:
            yield payload


def _to_entity(item: dict[str, Any]) -> CanonicalEntity | None:
    type_name = _type_name(item.get("@type"))
    entity_type = {
        "actor": "contacts",
        "source": "sources",
        "unitgroup": "unitgroups",
        "flowproperty": "flowproperties",
        "flow": "flows",
        "process": "processes",
    }.get(type_name)
    if entity_type is None:
        return None

    entity_id = _entity_id(item, entity_type)
    raw = dict(item)
    if entity_type == "flowproperties":
        raw.update(_flow_property_refs(item))
    elif entity_type == "flows":
        raw.update(_flow_refs(item))
    elif entity_type == "processes":
        raw["exchanges"] = _process_exchanges(item)

    return CanonicalEntity(
        entity_type=entity_type,
        internal_id=entity_id,
        external_id=_external_id(item),
        name=_name(item) or entity_type.rstrip("s").title(),
        category_path=_category_path(item),
        raw=raw,
    )


def _flow_property_refs(item: dict[str, Any]) -> dict[str, Any]:
    unit_group = item.get("unitGroup")
    if isinstance(unit_group, dict):
        return {
            "unitGroupRefId": _ref_id(unit_group),
            "unitGroupName": _name(unit_group),
        }
    return {}


def _flow_refs(item: dict[str, Any]) -> dict[str, Any]:
    factors = item.get("flowProperties")
    if not isinstance(factors, list) or not factors:
        return {}
    selected = next(
        (
            factor
            for factor in factors
            if isinstance(factor, dict) and factor.get("isRefFlowProperty")
        ),
        factors[0],
    )
    if not isinstance(selected, dict):
        return {}
    flow_property = selected.get("flowProperty")
    if not isinstance(flow_property, dict):
        return {}
    return {
        "flowPropertyRefId": _ref_id(flow_property),
        "flowPropertyName": _name(flow_property),
    }


def _process_exchanges(item: dict[str, Any]) -> list[dict[str, Any]]:
    exchanges = item.get("exchanges")
    if not isinstance(exchanges, list):
        return []
    result = []
    for exchange in exchanges:
        if not isinstance(exchange, dict):
            continue
        flow = exchange.get("flow")
        result.append(
            {
                "internalId": exchange.get("internalId"),
                "flow": flow if isinstance(flow, dict) else {},
                "flowRefId": _ref_id(flow) if isinstance(flow, dict) else None,
                "flowName": _name(flow) if isinstance(flow, dict) else None,
                "isInput": bool(exchange.get("isInput", False)),
                "amount": exchange.get("amount", 1),
            }
        )
    return result


def _type_name(value: Any) -> str:
    if isinstance(value, str):
        return value.replace(" ", "").lower()
    return ""


def _entity_id(item: dict[str, Any], entity_type: str) -> str:
    candidate = _external_id(item)
    if _is_uuid(candidate):
        return candidate
    seed = f"openlca-jsonld/{entity_type}/{candidate or _name(item) or json.dumps(item, sort_keys=True)}"
    return str(uuid.uuid5(uuid.NAMESPACE_URL, seed))


def _external_id(item: dict[str, Any]) -> str | None:
    for key in ("@id", "refId", "id"):
        value = item.get(key)
        if value:
            return str(value)
    return None


def _ref_id(ref: dict[str, Any]) -> str | None:
    candidate = _external_id(ref)
    if not candidate:
        return None
    if _is_uuid(candidate):
        return candidate
    return str(uuid.uuid5(uuid.NAMESPACE_URL, f"openlca-jsonld/ref/{candidate}"))


def _is_uuid(value: str | None) -> bool:
    if not value:
        return False
    try:
        uuid.UUID(str(value))
    except ValueError:
        return False
    return True


def _name(item: Any) -> str | None:
    if not isinstance(item, dict):
        return None
    return _localized(item.get("name") or item.get("shortDescription"))


def _localized(value: Any) -> str | None:
    if isinstance(value, str):
        return value
    if isinstance(value, dict):
        for key in ("#text", "en", "default", "value"):
            if value.get(key):
                return str(value[key])
    if isinstance(value, list) and value:
        return _localized(value[0])
    return None


def _category_path(item: dict[str, Any]) -> list[str]:
    category = item.get("category")
    if isinstance(category, str):
        return [part for part in category.split("/") if part]
    if isinstance(category, dict):
        name = _name(category)
        return [name] if name else []
    return []
