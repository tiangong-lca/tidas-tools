"""EcoSpold 1 source adapter."""

from __future__ import annotations

from pathlib import Path
import re
from typing import Iterable
import uuid
import zipfile

from lxml import etree

from .base import SourceAdapter
from ..model import CanonicalEntity
from ..report import ConversionReport
from ..store import MemoryCanonicalStore

UUID_RE = re.compile(
    r"(?P<uuid>[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12})"
)


class EcoSpold1Adapter(SourceAdapter):
    """Read EcoSpold 1 XML datasets into the canonical store."""

    format_id = "ecospold1"

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
                    message="Skipped invalid EcoSpold 1 XML payload.",
                    source_object=label,
                )
                continue
            datasets = _datasets(root)
            for index, dataset in enumerate(datasets, start=1):
                process = _process_entity(dataset, label, index)
                exchanges = []
                for exchange_index, exchange in enumerate(
                    _exchange_elements(dataset), 1
                ):
                    flow = _flow_entity(exchange, label, exchange_index)
                    store.add(flow)
                    exchanges.append(_exchange(exchange, flow, exchange_index))
                process.raw["exchanges"] = exchanges
                store.add(process)
                count += 1
                report.add_issue(
                    severity="warning",
                    code="ecospold1_minimal_mapping",
                    message=(
                        "Mapped EcoSpold 1 dataset with the current minimal adapter."
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
                code="no_ecospold1_datasets",
                message="No EcoSpold 1 datasets were found.",
            )


def _iter_xml_payloads(source: Path) -> Iterable[tuple[str, bytes]]:
    if source.is_dir():
        for path in sorted(source.rglob("*.xml")):
            yield str(path), path.read_bytes()
        return
    if source.suffix.lower() == ".zip":
        with zipfile.ZipFile(source) as archive:
            for name in sorted(archive.namelist()):
                if name.lower().endswith(".xml"):
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
    datasets = root.xpath(".//*[local-name()='dataset' or local-name()='dataSet']")
    return datasets or [root]


def _exchange_elements(dataset) -> list:
    return dataset.xpath(".//*[local-name()='exchange']")


def _process_entity(dataset, label: str, index: int) -> CanonicalEntity:
    name = _first_text(
        dataset,
        [
            ".//*[local-name()='referenceFunction']/@name",
            ".//*[local-name()='referenceFunction']/@generalComment",
            ".//*[local-name()='dataSetInformation']/@name",
            ".//@name",
        ],
    )
    name = name or f"EcoSpold 1 process {index}"
    filename_uuid = _uuid_from_label(label)
    process_id = filename_uuid or _stable_id(
        f"ecospold1/process/{label}/{index}/{name}"
    )
    return CanonicalEntity(
        entity_type="processes",
        internal_id=process_id,
        external_id=filename_uuid,
        name=name,
        raw={"description": f"Imported from EcoSpold 1 source {label}."},
    )


def _flow_entity(exchange, label: str, index: int) -> CanonicalEntity:
    name = (
        _attr(exchange, "name") or _attr(exchange, "casNumber") or f"Exchange {index}"
    )
    category = _attr(exchange, "category")
    subcategory = _attr(exchange, "subCategory")
    flow_id = _stable_id(
        f"ecospold1/flow/{label}/{name}/{category}/{subcategory}/{_attr(exchange, 'unit')}"
    )
    return CanonicalEntity(
        entity_type="flows",
        internal_id=flow_id,
        name=name,
        category_path=[part for part in (category, subcategory) if part],
        raw={"flowType": _flow_type(exchange), "unitName": _attr(exchange, "unit")},
    )


def _exchange(exchange, flow: CanonicalEntity, index: int) -> dict:
    amount = (
        _attr(exchange, "amount")
        or _attr(exchange, "meanValue")
        or _attr(exchange, "meanAmount")
        or "0"
    )
    source_number = _attr(exchange, "number")
    return {
        "internalId": index,
        "sourceExchangeNumber": source_number,
        "flow": {"@id": flow.internal_id, "name": flow.name},
        "flowRefId": flow.internal_id,
        "flowName": flow.name,
        "isInput": _direction(exchange) == "Input",
        "amount": amount,
    }


def _direction(exchange) -> str:
    if _attr(exchange, "inputGroup"):
        return "Input"
    return "Output"


def _flow_type(exchange) -> str:
    output_group = _attr(exchange, "outputGroup")
    if output_group == "0":
        return "PRODUCT_FLOW"
    category = " ".join(
        part.lower()
        for part in (_attr(exchange, "category"), _attr(exchange, "subCategory"))
        if part
    )
    if any(token in category for token in ("air", "water", "soil", "resource")):
        return "ELEMENTARY_FLOW"
    if _attr(exchange, "inputGroup") in {"4", "5"} or output_group in {"4", "5"}:
        return "ELEMENTARY_FLOW"
    return "PRODUCT_FLOW"


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
    value = element.get(name)
    if value is None:
        return None
    value = value.strip()
    return value or None


def _uuid_from_label(label: str) -> str | None:
    match = UUID_RE.search(Path(label).name)
    return match.group("uuid").lower() if match else None


def _stable_id(seed: str) -> str:
    return str(uuid.uuid5(uuid.NAMESPACE_URL, seed))
