"""openLCA JSON-LD source adapter."""

from __future__ import annotations

import json
from pathlib import Path
import re
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
        unsupported_items = []
        for label, payload in _iter_json_payloads(Path(source)):
            for item in _iter_items(payload):
                if not isinstance(item, dict):
                    continue
                entity = _to_entity(item, label)
                if entity is None:
                    unsupported_items.append(_unsupported_item(label, item))
                    continue
                store.add(entity)
                count += 1

        if unsupported_items:
            store.add(_auxiliary_source_entity(unsupported_items))
            count += 1

        lifecycle_model = _provider_graph_lifecycle_model(store, source)
        if lifecycle_model is not None:
            store.add(lifecycle_model)

        if count == 0:
            report.add_issue(
                severity="error",
                code="no_supported_jsonld_entities",
                message="No supported openLCA JSON-LD entities were found.",
            )
            return

        counts = store.counts()
        report.add_issue(
            severity="warning",
            code="jsonld_semantic_mapping_with_trace",
            message=(
                "Mapped openLCA JSON-LD entities to TIDAS fields where possible "
                "and preserved source trace metadata in common:other."
            ),
            context={
                "entity_counts": counts,
                "unsupported_json_items": len(unsupported_items),
                "lifecyclemodels": counts.get("lifecyclemodels", 0),
            },
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


def _to_entity(item: dict[str, Any], label: str) -> CanonicalEntity | None:
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
    raw["sourceTrace"] = _source_trace(label, _root_trace_payload(item))
    if entity_type == "flowproperties":
        raw.update(_flow_property_refs(item))
    elif entity_type == "flows":
        raw.update(_flow_refs(item))
        raw.update(_flow_metadata(item))
    elif entity_type == "processes":
        raw.update(_process_metadata(item))
        raw["exchanges"] = _process_exchanges(item, label)

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


def _flow_metadata(item: dict[str, Any]) -> dict[str, Any]:
    metadata: dict[str, Any] = {}
    if _text(item.get("refUnit")):
        metadata["unitName"] = str(item["refUnit"]).strip()
    formula = item.get("formula")
    if _looks_like_chemical_formula(formula):
        metadata["sumFormula"] = str(formula).strip()
    return metadata


def _process_metadata(item: dict[str, Any]) -> dict[str, Any]:
    documentation = item.get("processDocumentation")
    if not isinstance(documentation, dict):
        documentation = {}

    metadata: dict[str, Any] = {
        "sourceFormat": "openlca-jsonld",
        "sourceProcessType": item.get("processType"),
        "sourceDefaultAllocationMethod": item.get("defaultAllocationMethod"),
        "version": item.get("version"),
        "lastChange": item.get("lastChange"),
        "referenceYear": _year_from(documentation.get("validFrom")),
        "dataSetValidUntil": _year_from(documentation.get("validUntil")),
        "timeDescription": documentation.get("timeDescription"),
        "location": _location_code(item.get("location")),
        "locationDescription": documentation.get("geographyDescription"),
        "technologyDescription": documentation.get("technologyDescription"),
        "samplingProcedure": documentation.get("samplingDescription"),
        "dataCollectionPeriod": documentation.get("dataCollectionDescription"),
        "dataSelectionAndCombinationPrinciples": documentation.get(
            "dataSelectionDescription"
        ),
        "dataTreatmentAndExtrapolationsPrinciples": documentation.get(
            "dataTreatmentDescription"
        ),
        "dataCutOffAndCompletenessPrinciples": documentation.get(
            "completenessDescription"
        ),
        "uncertaintyAdjustments": documentation.get("uncertaintyAdjustments"),
        "useAdviceForDataSet": documentation.get("useAdvice"),
        "intendedApplications": documentation.get("intendedApplication"),
        "project": documentation.get("projectDescription"),
        "modellingConstants": documentation.get("modelingConstantsDescription"),
        "deviationsFromLCIMethodPrinciple": documentation.get(
            "inventoryMethodDescription"
        ),
        "accessRestrictions": documentation.get("restrictionsDescription"),
        "copyright": documentation.get("isCopyrightProtected"),
        "creationDate": documentation.get("creationDate"),
        "dataGeneratorRef": _reference_payload(documentation.get("dataGenerator")),
        "dataEntryByRef": _reference_payload(documentation.get("dataDocumentor")),
        "dataSetOwnerRef": _reference_payload(documentation.get("dataSetOwner")),
        "publicationRef": _reference_payload(documentation.get("publication")),
        "sourceRefs": _reference_list(documentation.get("sources")),
    }
    return {key: value for key, value in metadata.items() if value not in (None, [])}


def _process_exchanges(item: dict[str, Any], label: str) -> list[dict[str, Any]]:
    exchanges = item.get("exchanges")
    if not isinstance(exchanges, list):
        return []
    result = []
    for exchange in exchanges:
        if not isinstance(exchange, dict):
            continue
        flow = exchange.get("flow")
        unit = exchange.get("unit")
        flow_property = exchange.get("flowProperty")
        provider = exchange.get("defaultProvider")
        uncertainty = exchange.get("uncertainty")
        exchange_metadata = {
            "internalId": exchange.get("internalId"),
            "flow": flow if isinstance(flow, dict) else {},
            "flowRefId": _ref_id(flow) if isinstance(flow, dict) else None,
            "flowName": _name(flow) if isinstance(flow, dict) else None,
            "isInput": bool(exchange.get("isInput", False)),
            "amount": exchange.get("amount", 1),
            "amountFormula": exchange.get("amountFormula"),
            "isQuantitativeReference": bool(
                exchange.get("isQuantitativeReference", False)
            ),
            "isAvoidedProduct": exchange.get("isAvoidedProduct"),
            "unitId": _ref_id(unit) if isinstance(unit, dict) else None,
            "unitName": _name(unit) if isinstance(unit, dict) else None,
            "flowPropertyRefId": (
                _ref_id(flow_property) if isinstance(flow_property, dict) else None
            ),
            "flowPropertyName": (
                _name(flow_property) if isinstance(flow_property, dict) else None
            ),
            "providerRefId": _ref_id(provider) if isinstance(provider, dict) else None,
            "providerName": _name(provider) if isinstance(provider, dict) else None,
            "provider": provider if isinstance(provider, dict) else None,
            "location": _location_code(exchange.get("location")),
            "dqEntry": exchange.get("dqEntry"),
            "uncertaintyDistributionType": _uncertainty_distribution_type(uncertainty),
            "minimumAmount": _uncertainty_bound(uncertainty, "minimum"),
            "maximumAmount": _uncertainty_bound(uncertainty, "maximum"),
            "generalComment": exchange.get("description"),
            "sourceTrace": _source_trace(label, {"exchange": exchange}),
        }
        result.append(
            {
                key: value
                for key, value in exchange_metadata.items()
                if value not in (None, {})
            }
        )
    return result


def _provider_graph_lifecycle_model(
    store: MemoryCanonicalStore, source: str | Path
) -> CanonicalEntity | None:
    processes = sorted(
        store.iter_type("processes"), key=lambda entity: entity.internal_id
    )
    process_ids = {entity.internal_id for entity in processes}
    if len(processes) < 2:
        return None

    connections = []
    skipped = []
    for consumer in processes:
        for exchange in consumer.raw.get("exchanges") or []:
            provider_id = exchange.get("providerRefId")
            if not provider_id:
                continue
            if not exchange.get("isInput", False):
                skipped.append(_provider_skip_payload(consumer, exchange, "non_input"))
                continue
            if provider_id not in process_ids:
                skipped.append(
                    _provider_skip_payload(consumer, exchange, "missing_provider")
                )
                continue
            flow_id = exchange.get("flowRefId")
            if not _is_uuid(flow_id):
                skipped.append(
                    _provider_skip_payload(consumer, exchange, "missing_flow")
                )
                continue
            connections.append(
                {
                    "providerProcessId": provider_id,
                    "consumerProcessId": consumer.internal_id,
                    "flowRefId": flow_id,
                    "flowName": exchange.get("flowName"),
                    "consumerExchangeInternalId": exchange.get("internalId"),
                    "amount": exchange.get("amount"),
                    "location": exchange.get("location"),
                }
            )

    if not connections:
        return None

    reference_process = _reference_process(processes)
    model_id = str(
        uuid.uuid5(uuid.NAMESPACE_URL, f"openlca-jsonld/lifecyclemodel/{source}")
    )
    return CanonicalEntity(
        entity_type="lifecyclemodels",
        internal_id=model_id,
        name="openLCA JSON-LD provider graph",
        raw={
            "description": (
                "Derived lifecycle model from openLCA JSON-LD exchange "
                "defaultProvider links."
            ),
            "referenceProcessId": reference_process.internal_id,
            "processRefs": [
                {
                    "id": entity.internal_id,
                    "name": entity.name or "Process",
                    "processType": entity.raw.get("sourceProcessType"),
                }
                for entity in processes
            ],
            "connections": connections,
            "sourceTrace": {
                "format": "openlca-jsonld",
                "sourceObject": str(source),
                "derivedEntity": "provider-graph-lifecyclemodel",
                "providerConnectionCount": len(connections),
                "skippedProviderLinks": skipped,
            },
        },
    )


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


def _ref_id(ref: dict[str, Any] | None) -> str | None:
    if not isinstance(ref, dict):
        return None
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


def _reference_process(processes: list[CanonicalEntity]) -> CanonicalEntity:
    for entity in processes:
        if entity.raw.get("sourceProcessType") == "LCI_RESULT":
            return entity
    return processes[0]


def _unsupported_item(label: str, item: dict[str, Any]) -> dict[str, Any]:
    return {
        "sourceObject": label,
        "type": item.get("@type") or item.get("type"),
        "id": _external_id(item),
        "payload": item,
    }


def _auxiliary_source_entity(items: list[dict[str, Any]]) -> CanonicalEntity:
    source_id = str(uuid.uuid5(uuid.NAMESPACE_URL, "openlca-jsonld/auxiliary-metadata"))
    return CanonicalEntity(
        entity_type="sources",
        internal_id=source_id,
        name="openLCA JSON-LD auxiliary metadata",
        raw={
            "shortName": "openLCA JSON-LD auxiliary metadata",
            "sourceCitation": (
                "openLCA JSON-LD auxiliary metadata without a direct TIDAS "
                "root dataset target"
            ),
            "description": (
                "Unsupported or package-level JSON-LD objects preserved for "
                "traceability."
            ),
            "sourceTrace": {
                "format": "openlca-jsonld",
                "sourceObject": "auxiliary-jsonld-metadata",
                "items": items,
            },
        },
    )


def _source_trace(label: str, payload: dict[str, Any]) -> dict[str, Any]:
    return {
        "format": "openlca-jsonld",
        "sourceObject": label,
        "payload": payload,
    }


def _root_trace_payload(item: dict[str, Any]) -> dict[str, Any]:
    if _type_name(item.get("@type")) != "process":
        return {"entity": item}
    payload = dict(item)
    if isinstance(payload.get("exchanges"), list):
        payload["exchanges"] = (
            "TIDAS_IMPORT_TRACE_EXCHANGES_STORED_ON_EXCHANGE_COMMON_OTHER"
        )
    return {"entity": payload}


def _provider_skip_payload(
    process: CanonicalEntity, exchange: dict[str, Any], reason: str
) -> dict[str, Any]:
    return {
        "reason": reason,
        "consumerProcessId": process.internal_id,
        "providerProcessId": exchange.get("providerRefId"),
        "flowRefId": exchange.get("flowRefId"),
        "exchangeInternalId": exchange.get("internalId"),
    }


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


def _reference_payload(item: Any) -> dict[str, str] | None:
    if not isinstance(item, dict):
        return None
    ref_id = _ref_id(item)
    if not ref_id:
        return None
    return {"id": ref_id, "name": _name(item) or ref_id}


def _reference_list(value: Any) -> list[dict[str, str]]:
    if not isinstance(value, list):
        return []
    refs = []
    for item in value:
        ref = _reference_payload(item)
        if ref:
            refs.append(ref)
    return refs


def _category_path(item: dict[str, Any]) -> list[str]:
    category = item.get("category")
    if isinstance(category, str):
        return [part for part in category.split("/") if part]
    if isinstance(category, dict):
        name = _name(category)
        return [name] if name else []
    return []


def _location_code(value: Any) -> str | None:
    if isinstance(value, dict):
        for key in ("code", "name"):
            text = value.get(key)
            if _text(text):
                return str(text).strip()
    if _text(value):
        return str(value).strip()
    return None


def _year_from(value: Any) -> int | None:
    if value is None:
        return None
    text = str(value)
    match = re.search(r"\b(\d{4})\b", text)
    return int(match.group(1)) if match else None


def _uncertainty_distribution_type(value: Any) -> str | None:
    if not isinstance(value, dict):
        return None
    mapping = {
        "LOG_NORMAL_DISTRIBUTION": "log-normal",
        "TRIANGLE_DISTRIBUTION": "triangular",
        "TRIANGULAR_DISTRIBUTION": "triangular",
        "UNIFORM_DISTRIBUTION": "uniform",
        "NORMAL_DISTRIBUTION": "normal",
    }
    return mapping.get(str(value.get("distributionType") or "").strip().upper())


def _uncertainty_bound(value: Any, key: str) -> Any:
    if isinstance(value, dict):
        return value.get(key)
    return None


def _looks_like_chemical_formula(value: Any) -> bool:
    if not _text(value):
        return False
    text = str(value).strip()
    if len(text) > 100 or any(char.isspace() for char in text):
        return False
    return bool(re.fullmatch(r"(?:[A-Z][a-z]?\d*(?:[()]\d*)?)+[+-]?", text))


def _text(value: Any) -> bool:
    return isinstance(value, str) and bool(value.strip())
