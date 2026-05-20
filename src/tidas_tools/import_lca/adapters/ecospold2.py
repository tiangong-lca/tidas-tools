"""EcoSpold 2 source adapter."""

from __future__ import annotations

from pathlib import Path
from typing import Any, Iterable
import uuid
import zipfile

from lxml import etree

from .base import SourceAdapter
from ..model import CanonicalEntity
from ..report import ConversionReport
from ..store import MemoryCanonicalStore


class EcoSpold2Adapter(SourceAdapter):
    """Read EcoSpold 2 activity datasets into the canonical store."""

    format_id = "ecospold2"

    def read(
        self,
        source: str | Path,
        store: MemoryCanonicalStore,
        report: ConversionReport,
    ) -> None:
        count = 0
        for label, data in _iter_xml_payloads(Path(source)):
            root = _parse_xml(data)
            if root is None:
                report.add_issue(
                    severity="warning",
                    code="invalid_xml_skipped",
                    message="Skipped invalid EcoSpold 2 XML payload.",
                    source_object=label,
                )
                continue
            for index, dataset in enumerate(_datasets(root), start=1):
                process = _process_entity(dataset, label, index)
                exchanges = []
                for exchange_index, element in enumerate(
                    _exchange_elements(dataset), 1
                ):
                    flow = _flow_entity(element, label, exchange_index)
                    store.add(flow)
                    exchanges.append(_exchange(element, flow, exchange_index))
                process.raw["exchanges"] = _reference_first(exchanges)
                store.add(process)
                count += 1
                report.add_issue(
                    severity="warning",
                    code="ecospold2_minimal_mapping",
                    message=(
                        "Mapped EcoSpold 2 activity dataset with the current "
                        "minimal adapter."
                    ),
                    source_object=label,
                    context={
                        "process_id": process.internal_id,
                        "exchange_count": len(exchanges),
                    },
                )

        if count == 0:
            report.add_issue(
                severity="error",
                code="no_ecospold2_datasets",
                message="No EcoSpold 2 activity datasets were found.",
            )


def _iter_xml_payloads(source: Path) -> Iterable[tuple[str, bytes]]:
    if source.is_dir():
        for path in sorted(source.rglob("*")):
            if path.suffix.lower() in {".spold", ".xml"}:
                yield str(path), path.read_bytes()
        return
    if source.suffix.lower() == ".zip":
        with zipfile.ZipFile(source) as archive:
            for name in sorted(archive.namelist()):
                if name.lower().endswith((".spold", ".xml")):
                    with archive.open(name) as stream:
                        yield f"{source}:{name}", stream.read()
        return
    yield str(source), source.read_bytes()


def _parse_xml(data: bytes):
    parser = etree.XMLParser(resolve_entities=False, no_network=True, recover=True)
    try:
        return etree.fromstring(data, parser=parser)
    except etree.XMLSyntaxError:
        return None


def _datasets(root) -> list:
    datasets = root.xpath(".//*[local-name()='activityDataset']")
    if datasets:
        return datasets
    local_name = etree.QName(root).localname
    return [root] if local_name == "activityDataset" else []


def _exchange_elements(dataset) -> list:
    return dataset.xpath(
        ".//*[local-name()='intermediateExchange' or local-name()='elementaryExchange']"
    )


def _process_entity(dataset, label: str, index: int) -> CanonicalEntity:
    activity = _first_element(dataset, ".//*[local-name()='activity']")
    name = _first_text(
        dataset,
        [
            ".//*[local-name()='activity']/*[local-name()='activityName']/text()",
            ".//*[local-name()='activity']/@activityName",
            ".//*[local-name()='activity']/@name",
        ],
    )
    name = name or f"EcoSpold 2 activity {index}"
    activity_id = _attr(activity, "id") if activity is not None else None
    process_id = (
        activity_id
        if _is_uuid(activity_id)
        else _stable_id(f"ecospold2/process/{label}/{index}/{name}")
    )
    description = _first_text(
        dataset,
        [
            ".//*[local-name()='activity']/*[local-name()='generalComment']/*[local-name()='text']/text()",
            ".//*[local-name()='activity']/*[local-name()='includedActivitiesStart']/text()",
        ],
    )
    return CanonicalEntity(
        entity_type="processes",
        internal_id=process_id,
        external_id=activity_id,
        name=name,
        raw={"description": description or f"Imported from EcoSpold 2 source {label}."},
    )


def _flow_entity(element, label: str, index: int) -> CanonicalEntity:
    name = _first_text(element, [".//*[local-name()='name']/text()", ".//@name"])
    name = name or f"EcoSpold 2 exchange {index}"
    flow_external_id = (
        _attr(element, "intermediateExchangeId")
        or _attr(element, "elementaryExchangeId")
        or _attr(element, "id")
    )
    flow_id = (
        flow_external_id
        if _is_uuid(flow_external_id)
        else _stable_id(f"ecospold2/flow/{label}/{name}/{_flow_type(element)}")
    )
    return CanonicalEntity(
        entity_type="flows",
        internal_id=flow_id,
        external_id=flow_external_id,
        name=name,
        category_path=_category_path(element),
        raw={"flowType": _flow_type(element), "unitName": _unit_name(element)},
    )


def _exchange(element, flow: CanonicalEntity, index: int) -> dict[str, Any]:
    amount = _attr(element, "amount") or "0"
    return {
        "internalId": index,
        "flow": {"@id": flow.internal_id, "name": flow.name},
        "flowRefId": flow.internal_id,
        "flowName": flow.name,
        "isInput": _is_input(element),
        "amount": amount,
    }


def _reference_first(exchanges: list[dict[str, Any]]) -> list[dict[str, Any]]:
    def sort_key(exchange: dict[str, Any]) -> tuple[int, int]:
        if exchange.get("isInput"):
            return (2, int(exchange["internalId"]))
        return (0, int(exchange["internalId"]))

    ordered = sorted(exchanges, key=sort_key)
    for index, exchange in enumerate(ordered, start=1):
        exchange["internalId"] = index
    return ordered


def _is_input(element) -> bool:
    return _first_text(element, ["./*[local-name()='inputGroup']/text()"]) is not None


def _flow_type(element) -> str:
    local_name = etree.QName(element).localname
    if local_name == "elementaryExchange":
        return "ELEMENTARY_FLOW"
    text = " ".join(element.xpath(".//*[local-name()='classificationValue']/text()"))
    if "waste" in text.lower():
        return "WASTE_FLOW"
    return "PRODUCT_FLOW"


def _unit_name(element) -> str | None:
    return _first_text(
        element, [".//*[local-name()='unitName']/text()", ".//@unitName"]
    )


def _category_path(element) -> list[str]:
    compartment = _first_text(
        element,
        [
            ".//*[local-name()='compartment']/*[local-name()='compartment']/text()",
            ".//*[local-name()='classificationValue']/text()",
        ],
    )
    subcompartment = _first_text(
        element,
        [".//*[local-name()='compartment']/*[local-name()='subcompartment']/text()"],
    )
    return [part for part in (compartment, subcompartment) if part]


def _first_element(element, expression: str):
    values = element.xpath(expression)
    return values[0] if values else None


def _first_text(element, expressions: list[str]) -> str | None:
    for expression in expressions:
        values = element.xpath(expression)
        for value in values:
            if isinstance(value, str) and value.strip():
                return value.strip()
            text = getattr(value, "text", None)
            if text and text.strip():
                return text.strip()
    return None


def _attr(element, name: str) -> str | None:
    if element is None:
        return None
    value = element.get(name)
    if value is None:
        return None
    value = value.strip()
    return value or None


def _is_uuid(value: str | None) -> bool:
    if not value:
        return False
    try:
        uuid.UUID(str(value))
    except ValueError:
        return False
    return True


def _stable_id(seed: str) -> str:
    return str(uuid.uuid5(uuid.NAMESPACE_URL, seed))
