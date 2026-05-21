"""EcoSpold 1 source adapter."""

from __future__ import annotations

from pathlib import Path
import re
from typing import Iterable
import uuid
import zipfile

from lxml import etree

from .base import SourceAdapter
from .xml_trace import element_trace
from ..model import CanonicalEntity
from ..report import ConversionReport
from ..store import MemoryCanonicalStore

UUID_RE = re.compile(
    r"(?P<uuid>[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12})"
)
YEAR_RE = re.compile(r"\b(?P<year>\d{4})\b")
FlowKey = tuple[str, ...]


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
        flow_cache: dict[FlowKey, CanonicalEntity] = {}
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
                source_refs = []
                for source_entity in _source_entities(dataset):
                    store.add(source_entity)
                    source_refs.append(
                        {
                            "id": source_entity.internal_id,
                            "name": source_entity.name or "Imported source",
                        }
                    )
                if source_refs:
                    process.raw["sourceRefs"] = source_refs
                exchanges = []
                for exchange_index, exchange in enumerate(
                    _exchange_elements(dataset), 1
                ):
                    flow, is_new_flow = _cached_flow_entity(
                        exchange, exchange_index, flow_cache
                    )
                    if is_new_flow:
                        store.add(flow)
                    exchanges.append(_exchange(exchange, flow, exchange_index))
                process.raw["exchanges"] = exchanges
                store.add(process)
                count += 1
                report.add_issue(
                    severity="warning",
                    code="ecospold1_mapping_with_trace",
                    message=(
                        "Mapped EcoSpold 1 dataset and preserved source trace metadata."
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
    reference_function = _first_element(
        dataset, ".//*[local-name()='referenceFunction']"
    )
    geography = _first_element(dataset, ".//*[local-name()='geography']")
    technology = _first_element(dataset, ".//*[local-name()='technology']")
    time_period = _first_element(dataset, ".//*[local-name()='timePeriod']")
    representativeness = _first_element(
        dataset, ".//*[local-name()='representativeness']"
    )
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
    technology_text = _attr(technology, "text") or _attr(
        reference_function, "includedProcesses"
    )
    time_description = _text_or_attrs(
        time_period,
        [
            "startDate",
            "endDate",
            "startYear",
            "endYear",
            "dataValidForEntirePeriod",
        ],
    )
    description = _first_text(
        dataset,
        [
            ".//*[local-name()='referenceFunction']/@generalComment",
            ".//*[local-name()='technology']/@text",
            ".//*[local-name()='timePeriod']/text()",
        ],
    )
    return CanonicalEntity(
        entity_type="processes",
        internal_id=process_id,
        external_id=filename_uuid,
        name=name,
        raw={
            "description": description or f"Imported from EcoSpold 1 source {label}.",
            "sourceFormat": "ecospold1",
            "sourceLabel": label,
            "sourceTrace": {
                "format": "ecospold1",
                "sourceObject": label,
                "dataset": element_trace(dataset, exclude_child_names={"flowData"}),
            },
            "location": _attr(geography, "location"),
            "locationDescription": _attr(geography, "text"),
            "referenceYear": _year_from(
                _attr(time_period, "startDate") or _attr(time_period, "startYear")
            ),
            "dataSetValidUntil": _year_from(
                _attr(time_period, "endDate") or _attr(time_period, "endYear")
            ),
            "timeDescription": time_description,
            "technologyDescription": technology_text,
            "dataCollectionPeriod": _period_text(time_period),
            "productionVolume": _attr(representativeness, "productionVolume"),
            "samplingProcedure": _attr(representativeness, "samplingProcedure"),
            "uncertaintyAdjustments": _attr(
                representativeness, "uncertaintyAdjustments"
            ),
            "useAdviceForDataSet": _attr(representativeness, "extrapolations"),
            "sourceCategory": _attr(reference_function, "category"),
            "sourceSubCategory": _attr(reference_function, "subCategory"),
            "sourceLocalCategory": _attr(reference_function, "localCategory"),
            "sourceLocalSubCategory": _attr(reference_function, "localSubCategory"),
        },
    )


def _source_entities(dataset) -> list[CanonicalEntity]:
    entities = []
    for source in dataset.xpath(".//*[local-name()='source']"):
        source_number = _attr(source, "sourceNumber")
        title = _attr(source, "title")
        author = _attr(source, "firstAuthor")
        year = _attr(source, "year")
        text = _attr(source, "text")
        name = title or author or source_number or "EcoSpold 1 source"
        seed = "\x1f".join(
            part or "" for part in (source_number, title, author, year, text)
        )
        source_id = _stable_id(f"ecospold1/source/{seed}")
        citation = ", ".join(part for part in (author, title, year) if part)
        entities.append(
            CanonicalEntity(
                entity_type="sources",
                internal_id=source_id,
                external_id=source_number,
                name=name,
                raw={
                    "shortName": name,
                    "sourceCitation": citation or text or name,
                    "description": text,
                    "sourceTrace": {
                        "format": "ecospold1",
                        "sourceObject": "source",
                        "source": element_trace(source),
                    },
                },
            )
        )
    return entities


def _cached_flow_entity(
    exchange, index: int, flow_cache: dict[FlowKey, CanonicalEntity]
) -> tuple[CanonicalEntity, bool]:
    key = _flow_key(exchange, index)
    flow = flow_cache.get(key)
    if flow is None:
        flow = _flow_entity(exchange, index, key)
        flow_cache[key] = flow
        return flow, True
    return flow, False


def _flow_entity(exchange, index: int, key: FlowKey) -> CanonicalEntity:
    name = _flow_name(exchange, index)
    category = _attr(exchange, "category")
    subcategory = _attr(exchange, "subCategory")
    flow_id = _stable_id(_flow_key_seed(key))
    raw = {
        "flowType": _flow_type(exchange),
        "unitName": _attr(exchange, "unit"),
        "CASNumber": _attr(exchange, "CASNumber"),
        "sumFormula": _attr(exchange, "formula"),
        "synonyms": _attr(exchange, "localName"),
        "sourceTrace": {
            "format": "ecospold1",
            "sourceObject": "exchange",
            "exchange": element_trace(exchange),
        },
    }
    return CanonicalEntity(
        entity_type="flows",
        internal_id=flow_id,
        name=name,
        category_path=[part for part in (category, subcategory) if part],
        raw=raw,
    )


def _flow_key(exchange, index: int) -> FlowKey:
    return (
        _key_text(_flow_type(exchange)),
        _key_text(_flow_name(exchange, index)),
        _key_text(_attr(exchange, "CASNumber")),
        _key_text(_attr(exchange, "formula")),
        _key_text(_attr(exchange, "category")),
        _key_text(_attr(exchange, "subCategory")),
        _key_text(_attr(exchange, "unit")),
    )


def _flow_key_seed(key: FlowKey) -> str:
    return "ecospold1/flow/" + "\x1f".join(key)


def _flow_name(exchange, index: int) -> str:
    return (
        _attr(exchange, "name")
        or _attr(exchange, "CASNumber")
        or _attr(exchange, "number")
        or f"Exchange {index}"
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
        "minimumAmount": _attr(exchange, "minValue") or _attr(exchange, "minAmount"),
        "maximumAmount": _attr(exchange, "maxValue") or _attr(exchange, "maxAmount"),
        "uncertaintyDistributionType": _uncertainty_type(
            _attr(exchange, "uncertaintyType")
        ),
        "relativeStandardDeviation95In": _attr(exchange, "standardDeviation95"),
        "generalComment": _attr(exchange, "generalComment"),
        "sourceTrace": {
            "format": "ecospold1",
            "sourceObject": "exchange",
            "exchange": element_trace(exchange),
        },
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


def _first_element(element, expression: str):
    values = element.xpath(expression)
    return values[0] if values else None


def _attr(element, name: str) -> str | None:
    if element is None:
        return None
    value = element.get(name)
    if value is None:
        return None
    value = value.strip()
    return value or None


def _key_text(value: str | None) -> str:
    if value is None:
        return ""
    return " ".join(value.strip().split()).casefold()


def _uuid_from_label(label: str) -> str | None:
    match = UUID_RE.search(Path(label).name)
    return match.group("uuid").lower() if match else None


def _year_from(value: str | None) -> int | None:
    if not value:
        return None
    match = YEAR_RE.search(value)
    return int(match.group("year")) if match else None


def _period_text(element) -> str | None:
    if element is None:
        return None
    start = _attr(element, "startDate") or _attr(element, "startYear")
    end = _attr(element, "endDate") or _attr(element, "endYear")
    parts = [part for part in (start, end) if part]
    if parts:
        return " - ".join(parts)
    return _text_or_attrs(element, [])


def _text_or_attrs(element, attr_names: list[str]) -> str | None:
    if element is None:
        return None
    text = _first_text(element, ["./text()"])
    if text:
        return text
    parts = []
    for name in attr_names:
        value = _attr(element, name)
        if value:
            parts.append(f"{name}={value}")
    return "; ".join(parts) if parts else None


def _uncertainty_type(value: str | None) -> str | None:
    if not value:
        return None
    text = value.strip().casefold().replace("_", "-")
    mapping = {
        "0": "undefined",
        "undefined": "undefined",
        "1": "log-normal",
        "lognormal": "log-normal",
        "log-normal": "log-normal",
        "2": "normal",
        "normal": "normal",
        "3": "triangular",
        "triangular": "triangular",
        "4": "uniform",
        "uniform": "uniform",
    }
    return mapping.get(text)


def _stable_id(seed: str) -> str:
    return str(uuid.uuid5(uuid.NAMESPACE_URL, seed))
