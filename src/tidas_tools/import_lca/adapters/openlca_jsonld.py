"""openLCA JSON-LD source adapter."""

from __future__ import annotations

import json
from decimal import Decimal, InvalidOperation
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
        entries = [
            (label, item)
            for label, payload in _iter_json_payloads(Path(source))
            for item in _iter_items(payload)
            if isinstance(item, dict)
        ]
        location_index = _location_index(entries)
        unsupported_items = []
        for label, item in entries:
            entity = _to_entity(item, label, location_index)
            if entity is None:
                unsupported_items.append(_unsupported_item(label, item))
                continue
            store.add(entity)
            count += 1

        if unsupported_items:
            store.add(_auxiliary_source_entity(unsupported_items))
            count += 1

        _normalize_exchange_amounts(store, report)

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
    return json.loads(data.decode("utf-8-sig"), parse_float=Decimal)


def _iter_items(payload: Any) -> Iterable[dict[str, Any]]:
    if isinstance(payload, list):
        yield from payload
    elif isinstance(payload, dict):
        if "@graph" in payload and isinstance(payload["@graph"], list):
            yield from payload["@graph"]
        else:
            yield payload


def _to_entity(
    item: dict[str, Any], label: str, location_index: dict[str, dict[str, Any]]
) -> CanonicalEntity | None:
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
        raw.update(_process_metadata(item, location_index))
        raw["exchanges"] = _process_exchanges(item, label, location_index)

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
    category_path = _category_path(item)
    if category_path:
        metadata["sourceCategoryPath"] = category_path
    if _text(item.get("refUnit")):
        metadata["unitName"] = str(item["refUnit"]).strip()
    cas_number = item.get("CASNumber") or item.get("cas")
    if _text(cas_number):
        metadata["CASNumber"] = str(cas_number).strip()
    formula = item.get("formula")
    if _looks_like_chemical_formula(formula):
        metadata["sumFormula"] = str(formula).strip()
    return metadata


def _process_metadata(
    item: dict[str, Any], location_index: dict[str, dict[str, Any]]
) -> dict[str, Any]:
    documentation = item.get("processDocumentation")
    if not isinstance(documentation, dict):
        documentation = {}

    location = _location_metadata(item.get("location"), location_index)
    category_path = _category_path(item)
    metadata: dict[str, Any] = {
        "sourceFormat": "openlca-jsonld",
        "sourceProcessType": item.get("processType"),
        "sourceDefaultAllocationMethod": item.get("defaultAllocationMethod"),
        "sourceCategoryPath": category_path,
        "version": item.get("version"),
        "lastChange": item.get("lastChange"),
        "referenceYear": _year_from(documentation.get("validFrom")),
        "dataSetValidUntil": _year_from(documentation.get("validUntil")),
        "timeDescription": documentation.get("timeDescription"),
        "location": location.get("code"),
        "sourceLocation": location,
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
        "sourceReviews": _review_list(documentation.get("reviews")),
    }
    return {key: value for key, value in metadata.items() if value not in (None, [])}


def _process_exchanges(
    item: dict[str, Any], label: str, location_index: dict[str, dict[str, Any]]
) -> list[dict[str, Any]]:
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
        location = _location_metadata(exchange.get("location"), location_index)
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
            "location": location.get("code"),
            "sourceLocation": location,
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


_UNRESOLVED_SAMPLE_LIMIT = 20
_REF_UNIT_MISMATCH_SAMPLE_LIMIT = 5


def _normalize_exchange_amounts(
    store: MemoryCanonicalStore, report: ConversionReport
) -> None:
    """Rescale exchange amounts to each flow's reference flow property unit.

    openLCA exchanges may be stated in any unit of any flow property defined
    for the flow (e.g. g for a kg-referenced flow, or MJ for a mass-referenced
    fuel). TIDAS amounts are interpreted in the flow's reference flow property
    reference unit, so amounts are converted with
    ``amount * unit_cf * f_ref / f_property`` where ``unit_cf`` is the
    exchange unit's conversion factor inside its unit group and ``f_*`` are
    the flow's flow property conversion factors. All arithmetic stays in
    :class:`~decimal.Decimal` without rounding.
    """
    unit_index, group_ref, group_alias = _unit_group_indexes(store)
    fp_group, group_properties = _flow_property_indexes(store, group_alias)
    flow_index = _flow_factor_index(store)

    scanned = 0
    counts = {
        "normalized_same_property": 0,
        "normalized_cross_property": 0,
        "already_reference": 0,
        "no_unit_info": 0,
        "formula_with_scaled_amount": 0,
        "ref_unit_name_mismatch": 0,
    }
    unresolved: dict[str, int] = {}
    unresolved_samples: list[dict[str, Any]] = []
    mismatch_samples: list[dict[str, Any]] = []

    for entity in store.iter_type("processes"):
        for exchange in entity.raw.get("exchanges") or []:
            if not isinstance(exchange, dict):
                continue
            scanned += 1
            result = _normalize_exchange(
                exchange,
                unit_index=unit_index,
                group_ref=group_ref,
                fp_group=fp_group,
                group_properties=group_properties,
                flow_index=flow_index,
            )
            status = result["status"]
            if status == "unresolved":
                reason = result["reason"]
                unresolved[reason] = unresolved.get(reason, 0) + 1
                if len(unresolved_samples) < _UNRESOLVED_SAMPLE_LIMIT:
                    unresolved_samples.append(
                        {
                            "process_id": entity.internal_id,
                            "internalId": exchange.get("internalId"),
                            "unitId": exchange.get("unitId"),
                            "unitName": exchange.get("unitName"),
                            "reason": reason,
                        }
                    )
                continue
            if status == "normalized":
                counts[
                    (
                        "normalized_cross_property"
                        if result["cross_property"]
                        else "normalized_same_property"
                    )
                ] += 1
                if result["has_formula"]:
                    counts["formula_with_scaled_amount"] += 1
                if result["ref_unit_mismatch"] is not None:
                    counts["ref_unit_name_mismatch"] += 1
                    if len(mismatch_samples) < _REF_UNIT_MISMATCH_SAMPLE_LIMIT:
                        mismatch_samples.append(
                            {
                                "process_id": entity.internal_id,
                                "internalId": exchange.get("internalId"),
                                "flowRefUnit": result["ref_unit_mismatch"],
                                "targetUnit": exchange.get("unitName"),
                            }
                        )
                continue
            counts[status] += 1

    mutated = counts["normalized_same_property"] + counts["normalized_cross_property"]
    unresolved_total = sum(unresolved.values())
    if scanned == 0 or (mutated == 0 and unresolved_total == 0):
        return

    context: dict[str, Any] = {
        "scanned": scanned,
        "normalized_same_property": counts["normalized_same_property"],
        "normalized_cross_property": counts["normalized_cross_property"],
        "already_reference": counts["already_reference"],
        "no_unit_info": counts["no_unit_info"],
        "unresolved": dict(sorted(unresolved.items())),
        "unresolved_samples": unresolved_samples,
        "formula_with_scaled_amount": counts["formula_with_scaled_amount"],
        "ref_unit_name_mismatch": counts["ref_unit_name_mismatch"],
    }
    if mismatch_samples:
        context["ref_unit_name_mismatch_samples"] = mismatch_samples
    report.add_issue(
        severity="warning",
        code="exchange_amounts_normalized_to_reference_units",
        message=(
            "Normalized exchange amounts stated in non-reference units to "
            "each flow's reference flow property reference unit. Original "
            "values are preserved on the exchange as sourceAmount/sourceUnit* "
            "fields and in sourceTrace."
        ),
        context=context,
    )
    if unresolved_total:
        report.add_issue(
            severity="warning",
            code="exchange_unit_normalization_unresolved",
            message=(
                "Some exchange amounts could not be normalized to reference "
                "units and were left unchanged."
            ),
            context={
                "scanned": scanned,
                "unresolved_total": unresolved_total,
                "unresolved": dict(sorted(unresolved.items())),
                "unresolved_samples": unresolved_samples,
            },
        )


def _normalize_exchange(
    exchange: dict[str, Any],
    *,
    unit_index: dict[str, dict[str, Any]],
    group_ref: dict[str, dict[str, Any]],
    fp_group: dict[str, str],
    group_properties: dict[str, list[str]],
    flow_index: dict[str, dict[str, Any]],
) -> dict[str, Any]:
    unit_id = exchange.get("unitId")
    if not unit_id:
        return {"status": "no_unit_info"}
    unit_record = unit_index.get(str(unit_id))
    if unit_record is None:
        return {"status": "unresolved", "reason": "unknown_unit"}
    unit_cf = unit_record["cf"]
    unit_group_id = unit_record["group_id"]

    flow_ref_id = exchange.get("flowRefId")
    flow_record = flow_index.get(str(flow_ref_id)) if flow_ref_id else None

    property_id = exchange.get("flowPropertyRefId")
    if property_id:
        property_id = str(property_id)
        property_group_id = fp_group.get(property_id)
        if property_group_id is None:
            return {"status": "unresolved", "reason": "unknown_flow_property"}
    else:
        candidates = group_properties.get(unit_group_id, [])
        if flow_record is not None:
            candidates = [
                candidate
                for candidate in candidates
                if candidate in flow_record["factors"]
            ]
        if len(candidates) != 1:
            return {"status": "unresolved", "reason": "unknown_flow_property"}
        property_id = candidates[0]
        property_group_id = unit_group_id
    if property_group_id != unit_group_id:
        return {"status": "unresolved", "reason": "unit_not_in_property_group"}

    flow_ref = exchange.get("flow") if isinstance(exchange.get("flow"), dict) else {}
    declared_ref_unit = flow_ref.get("refUnit")
    declared_ref_unit = (
        str(declared_ref_unit).strip() if _text(declared_ref_unit) else None
    )

    target_property: tuple[str, str | None] | None = None
    if flow_record is None:
        # Dangling flow reference: only the same-property case is safe, and
        # only when the exchange's own flow Ref declares the same reference
        # unit as the property's unit group reference unit.
        group_info = group_ref.get(unit_group_id) or {}
        group_ref_unit_name = group_info.get("ref_unit_name")
        if (
            not declared_ref_unit
            or not group_ref_unit_name
            or declared_ref_unit != str(group_ref_unit_name).strip()
        ):
            return {"status": "unresolved", "reason": "missing_flow_factors"}
        factor_property = factor_ref = Decimal(1)
        target_group_id = unit_group_id
        cross_property = False
    else:
        factor_property = flow_record["factors"].get(property_id)
        if factor_property is None:
            return {"status": "unresolved", "reason": "missing_flow_factors"}
        ref_fp_id = flow_record["ref_fp_id"]
        factor_ref = flow_record["factors"].get(ref_fp_id)
        if factor_ref is None:
            return {"status": "unresolved", "reason": "missing_flow_factors"}
        if factor_property == 0 or factor_ref == 0:
            return {"status": "unresolved", "reason": "zero_factor"}
        cross_property = ref_fp_id != property_id
        if cross_property:
            target_group_id = fp_group.get(ref_fp_id)
            if target_group_id is None:
                return {"status": "unresolved", "reason": "unknown_flow_property"}
            target_property = (ref_fp_id, flow_record["ref_fp_name"])
        else:
            target_group_id = unit_group_id

    total = (unit_cf * factor_ref) / factor_property
    if total == 1:
        return {"status": "already_reference"}

    group_info = group_ref.get(target_group_id) or {}
    target_unit_name = group_info.get("ref_unit_name")
    if not target_unit_name:
        return {"status": "unresolved", "reason": "missing_reference_unit"}

    amount = _decimal_value(exchange.get("amount", 1))
    if amount is None:
        return {"status": "unresolved", "reason": "non_numeric_amount"}

    source_unit_name = exchange.get("unitName") or unit_record.get("unit_name")
    for key, source_key in (
        ("amount", "sourceAmount"),
        ("unitId", "sourceUnitId"),
        ("unitName", "sourceUnitName"),
        ("flowPropertyRefId", "sourceFlowPropertyRefId"),
        ("flowPropertyName", "sourceFlowPropertyName"),
    ):
        if key in exchange:
            exchange[source_key] = exchange[key]

    exchange["amount"] = (amount * unit_cf * factor_ref) / factor_property
    target_unit_id = group_info.get("ref_unit_id")
    if target_unit_id:
        exchange["unitId"] = target_unit_id
    else:
        exchange.pop("unitId", None)
    exchange["unitName"] = target_unit_name
    if target_property is not None:
        target_fp_id, target_fp_name = target_property
        exchange["flowPropertyRefId"] = target_fp_id
        if target_fp_name:
            exchange["flowPropertyName"] = target_fp_name
        else:
            exchange.pop("flowPropertyName", None)
    for bound_key in ("minimumAmount", "maximumAmount"):
        bound = exchange.get(bound_key)
        if isinstance(bound, bool) or not isinstance(bound, (int, float, Decimal)):
            continue
        bound_value = _decimal_value(bound)
        if bound_value is None:
            continue
        exchange[bound_key] = (bound_value * unit_cf * factor_ref) / factor_property

    normalization: dict[str, Any] = {
        "factor": str(total),
        "sourceUnit": source_unit_name,
        "targetUnit": target_unit_name,
        "crossProperty": cross_property,
    }
    has_formula = exchange.get("amountFormula") is not None
    if has_formula:
        normalization["amountFormulaNotRescaled"] = True
    exchange["amountNormalization"] = normalization

    mismatch = bool(
        declared_ref_unit and declared_ref_unit != str(target_unit_name).strip()
    )
    return {
        "status": "normalized",
        "cross_property": cross_property,
        "has_formula": has_formula,
        "ref_unit_mismatch": declared_ref_unit if mismatch else None,
    }


def _unit_group_indexes(
    store: MemoryCanonicalStore,
) -> tuple[dict[str, dict[str, Any]], dict[str, dict[str, Any]], dict[str, str]]:
    unit_index: dict[str, dict[str, Any]] = {}
    group_ref: dict[str, dict[str, Any]] = {}
    group_alias: dict[str, str] = {}
    for entity in store.iter_type("unitgroups"):
        group_id = entity.internal_id
        for key in _alias_ids(entity.raw, entity.internal_id):
            group_alias.setdefault(key, group_id)
        units = entity.raw.get("units")
        if not isinstance(units, list):
            continue
        reference_unit = next(
            (
                unit
                for unit in units
                if isinstance(unit, dict) and unit.get("referenceUnit")
            ),
            None,
        )
        if reference_unit is None:
            reference_unit = next(
                (
                    unit
                    for unit in units
                    if isinstance(unit, dict)
                    and _decimal_value(unit.get("conversionFactor")) == 1
                ),
                None,
            )
        if reference_unit is not None:
            group_ref[group_id] = {
                "ref_unit_id": _ref_id(reference_unit),
                "ref_unit_name": _name(reference_unit),
            }
        for unit in units:
            if not isinstance(unit, dict):
                continue
            conversion_factor = _decimal_value(unit.get("conversionFactor"))
            if conversion_factor is None:
                continue
            record = {
                "cf": conversion_factor,
                "group_id": group_id,
                "unit_name": _name(unit),
            }
            for key in _alias_ids(unit):
                unit_index.setdefault(key, record)
    return unit_index, group_ref, group_alias


def _flow_property_indexes(
    store: MemoryCanonicalStore, group_alias: dict[str, str]
) -> tuple[dict[str, str], dict[str, list[str]]]:
    fp_group: dict[str, str] = {}
    group_properties: dict[str, list[str]] = {}
    for entity in store.iter_type("flowproperties"):
        unit_group = entity.raw.get("unitGroup")
        group_key = _ref_id(unit_group) if isinstance(unit_group, dict) else None
        if group_key is None:
            group_key = entity.raw.get("unitGroupRefId")
        if not group_key:
            continue
        group_id = group_alias.get(str(group_key))
        if group_id is None:
            continue
        canonical_id = _ref_id(entity.raw) or entity.internal_id
        for key in _alias_ids(entity.raw, entity.internal_id):
            fp_group.setdefault(key, group_id)
        members = group_properties.setdefault(group_id, [])
        if canonical_id not in members:
            members.append(canonical_id)
    return fp_group, group_properties


def _flow_factor_index(store: MemoryCanonicalStore) -> dict[str, dict[str, Any]]:
    flow_index: dict[str, dict[str, Any]] = {}
    for entity in store.iter_type("flows"):
        entries = entity.raw.get("flowProperties")
        if not isinstance(entries, list) or not entries:
            continue
        factors: dict[str, Decimal] = {}
        first_entry: tuple[str, str | None] | None = None
        reference_entry: tuple[str, str | None] | None = None
        for entry in entries:
            if not isinstance(entry, dict):
                continue
            flow_property = entry.get("flowProperty")
            if not isinstance(flow_property, dict):
                continue
            fp_id = _ref_id(flow_property)
            if not fp_id:
                continue
            # openLCA omits conversionFactor when it is the default 1.0.
            factor = _decimal_value(entry.get("conversionFactor", 1))
            if factor is None:
                continue
            factors[fp_id] = factor
            if first_entry is None:
                first_entry = (fp_id, _name(flow_property))
            if reference_entry is None and entry.get("isRefFlowProperty"):
                reference_entry = (fp_id, _name(flow_property))
        if not factors:
            continue
        ref_fp_id, ref_fp_name = reference_entry or first_entry
        record = {
            "factors": factors,
            "ref_fp_id": ref_fp_id,
            "ref_fp_name": ref_fp_name,
        }
        for key in _alias_ids(entity.raw, entity.internal_id):
            flow_index.setdefault(key, record)
    return flow_index


def _alias_ids(item: Any, internal_id: str | None = None) -> list[str]:
    keys: list[str] = []
    if internal_id:
        keys.append(str(internal_id))
    if isinstance(item, dict):
        raw_id = _external_id(item)
        if raw_id and raw_id not in keys:
            keys.append(raw_id)
        ref = _ref_id(item)
        if ref and ref not in keys:
            keys.append(ref)
    return keys


def _decimal_value(value: Any) -> Decimal | None:
    if isinstance(value, bool):
        return None
    if isinstance(value, Decimal):
        return value
    if isinstance(value, int):
        return Decimal(value)
    if isinstance(value, float):
        return Decimal(str(value))
    if isinstance(value, str):
        try:
            return Decimal(value.strip())
        except InvalidOperation:
            return None
    return None


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
                "Candidate lifecycle model derived from openLCA JSON-LD "
                "exchange defaultProvider hints."
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
                "derivedEntity": "default-provider-candidate-lifecyclemodel",
                "linkingSemantics": {
                    "sourceField": "Exchange.defaultProvider",
                    "rule": "Only link resolvable default providers on input exchanges.",
                    "caveat": (
                        "openLCA defaultProvider is an exchange-level default "
                        "hint. It is not the same as ProductSystem.processLinks "
                        "unless a product system was explicitly built with the "
                        "same linking rule."
                    ),
                },
                "providerConnectionCount": len(connections),
                "skippedProviderLinks": skipped,
            },
        },
    )


def _location_index(
    entries: list[tuple[str, dict[str, Any]]],
) -> dict[str, dict[str, Any]]:
    by_id: dict[str, dict[str, Any]] = {}
    by_name: dict[str, dict[str, Any]] = {}
    for label, item in entries:
        if _type_name(item.get("@type")) != "location":
            continue
        payload = _location_payload(item, label)
        loc_id = payload.get("id")
        name = payload.get("name")
        if loc_id:
            by_id[str(loc_id)] = payload
        if name:
            by_name[_norm_location_name(str(name))] = payload
    return {"by_id": by_id, "by_name": by_name}


def _location_payload(item: dict[str, Any], label: str | None = None) -> dict[str, Any]:
    payload = {
        "id": _external_id(item),
        "name": _name(item),
        "code": item.get("code"),
        "category": item.get("category"),
        "latitude": item.get("latitude"),
        "longitude": item.get("longitude"),
    }
    if label:
        payload["sourceObject"] = label
    return {key: value for key, value in payload.items() if value not in (None, "")}


def _location_metadata(
    value: Any, location_index: dict[str, dict[str, Any]]
) -> dict[str, Any]:
    if not isinstance(value, dict):
        if not _text(value):
            return {}
        name = str(value).strip()
        return {
            key: val
            for key, val in {
                "name": name,
                "code": _location_code_from_name(name),
                "sourceValue": name,
            }.items()
            if val
        }

    ref_id = _external_id(value)
    name = _name(value)
    resolved = None
    if ref_id:
        resolved = location_index.get("by_id", {}).get(str(ref_id))
    if resolved is None and name:
        resolved = location_index.get("by_name", {}).get(_norm_location_name(name))
    code = (
        value.get("code")
        or (resolved or {}).get("code")
        or (_location_code_from_name(name) if name else None)
    )
    payload = {
        "id": ref_id,
        "name": name,
        "code": code,
        "category": value.get("category") or (resolved or {}).get("category"),
        "resolvedLocation": resolved,
        "sourceRef": value,
    }
    return {key: val for key, val in payload.items() if val not in (None, {}, "")}


def _location_code_from_name(value: str) -> str | None:
    aliases = {
        "global": "GLO",
        "row": "GLO",
        "rest of world": "GLO",
        "northern america": "RNA",
        "north america": "RNA",
        "europe": "RER",
        "united states": "US",
        "united states of america": "US",
        "united states of america (the)": "US",
        "china": "CN",
        "canada": "CA",
    }
    return aliases.get(_norm_location_name(value))


def _norm_location_name(value: str) -> str:
    return " ".join(value.casefold().strip().split())


def _type_name(value: Any) -> str:
    if isinstance(value, str):
        return value.replace(" ", "").lower()
    return ""


def _entity_id(item: dict[str, Any], entity_type: str) -> str:
    candidate = _external_id(item)
    if _is_uuid(candidate):
        return candidate
    seed = (
        f"openlca-jsonld/{entity_type}/"
        f"{candidate or _name(item) or json.dumps(item, sort_keys=True, default=str)}"
    )
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


def _review_list(value: Any) -> list[dict[str, Any]]:
    if not isinstance(value, list):
        return []
    reviews = []
    for item in value:
        if not isinstance(item, dict):
            continue
        report = _reference_payload(item.get("report"))
        reviews.append(
            {
                key: entry
                for key, entry in {
                    "reviewType": item.get("reviewType"),
                    "details": item.get("details"),
                    "reportRef": report,
                    "sourceReview": item,
                }.items()
                if entry not in (None, {})
            }
        )
    return reviews


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
