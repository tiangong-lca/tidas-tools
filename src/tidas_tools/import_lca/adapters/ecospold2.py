"""EcoSpold 2 source adapter."""

from __future__ import annotations

from pathlib import Path
import re
from typing import Any, Iterable
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
        flow_cache: dict[str, CanonicalEntity] = {}
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
                process = _process_entity(dataset, label, index, root)
                exchanges = []
                for exchange_index, element in enumerate(
                    _exchange_elements(dataset), 1
                ):
                    flow, is_new_flow = _cached_flow_entity(
                        element, exchange_index, flow_cache
                    )
                    if is_new_flow:
                        store.add(flow)
                    exchanges.append(_exchange(element, flow, exchange_index))
                process.raw["exchanges"] = _reference_first(exchanges)
                store.add(process)
                count += 1
                report.add_issue(
                    severity="warning",
                    code="ecospold2_mapping_with_trace",
                    message=(
                        "Mapped EcoSpold 2 activity dataset and preserved source trace metadata."
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
    datasets = root.xpath(
        ".//*[local-name()='activityDataset' or local-name()='childActivityDataset']"
    )
    if datasets:
        return datasets
    local_name = etree.QName(root).localname
    return [root] if local_name in {"activityDataset", "childActivityDataset"} else []


def _exchange_elements(dataset) -> list:
    return dataset.xpath(
        ".//*[local-name()='intermediateExchange' or local-name()='elementaryExchange']"
    )


def _process_entity(dataset, label: str, index: int, root) -> CanonicalEntity:
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
    activity_uuid = _uuid_value(activity_id)
    filename_uuids = _uuids_from_label(label)
    process_id = _process_id(
        label=label,
        index=index,
        name=name,
        activity_uuid=activity_uuid,
        filename_uuids=filename_uuids,
    )
    description = _first_text(
        dataset,
        [
            ".//*[local-name()='activity']/*[local-name()='generalComment']/*[local-name()='text']/text()",
            ".//*[local-name()='activity']/*[local-name()='includedActivitiesStart']/text()",
        ],
    )
    time_period = _first_element(
        dataset, ".//*[local-name()='activityDescription']/*[local-name()='timePeriod']"
    )
    if time_period is None:
        time_period = _first_element(dataset, ".//*[local-name()='timePeriod']")
    geography = _first_element(
        dataset, ".//*[local-name()='activityDescription']/*[local-name()='geography']"
    )
    if geography is None:
        geography = _first_element(dataset, ".//*[local-name()='geography']")
    technology = _first_element(
        dataset, ".//*[local-name()='activityDescription']/*[local-name()='technology']"
    )
    if technology is None:
        technology = _first_element(dataset, ".//*[local-name()='technology']")
    file_attributes = _first_element(root, ".//*[local-name()='fileAttributes']")
    return CanonicalEntity(
        entity_type="processes",
        internal_id=process_id,
        external_id=activity_id or (filename_uuids[0] if filename_uuids else None),
        name=name,
        raw={
            "description": description or f"Imported from EcoSpold 2 source {label}.",
            "location": _location(dataset),
            "locationDescription": _first_text(
                geography,
                [
                    "./*[local-name()='comment']/*[local-name()='text']/text()",
                    "./*[local-name()='comment']/text()",
                    "./*[local-name()='shortname']/text()",
                ],
            ),
            "referenceYear": _reference_year(dataset),
            "dataSetValidUntil": _end_year(time_period),
            "timeDescription": _time_description(time_period),
            "technologyDescription": _first_text(
                technology,
                [
                    "./*[local-name()='comment']/*[local-name()='text']/text()",
                    "./*[local-name()='comment']/text()",
                    ".//*[local-name()='text']/text()",
                ],
            ),
            "technologicalApplicability": description,
            "sourceActivityId": activity_id,
            "sourceFileUUIDs": filename_uuids,
            "sourceFormat": "ecospold2",
            "sourceLabel": label,
            "sourceTrace": {
                "format": "ecospold2",
                "sourceObject": label,
                "rootAttributes": element_trace(
                    root,
                    exclude_child_names={"activityDataset", "childActivityDataset"},
                ),
                "fileAttributes": (
                    element_trace(file_attributes)
                    if file_attributes is not None
                    else None
                ),
                "dataset": element_trace(dataset, exclude_child_names={"flowData"}),
            },
        },
    )


def _cached_flow_entity(
    element, index: int, flow_cache: dict[str, CanonicalEntity]
) -> tuple[CanonicalEntity, bool]:
    flow_id = _flow_id(element, index)
    flow = flow_cache.get(flow_id)
    if flow is None:
        flow = _flow_entity(element, index, flow_id)
        flow_cache[flow_id] = flow
        return flow, True
    return flow, False


def _flow_entity(element, index: int, flow_id: str) -> CanonicalEntity:
    raw = {
        "flowType": _flow_type(element),
        "unitName": _unit_name(element),
        "unitId": _attr(element, "unitId"),
        "sourceTrace": {
            "format": "ecospold2",
            "sourceObject": "exchange",
            "exchange": element_trace(element),
        },
    }
    cas_number = _cas_number(element)
    sum_formula = _sum_formula(element)
    if cas_number:
        raw["CASNumber"] = cas_number
    if sum_formula:
        raw["sumFormula"] = sum_formula
    return CanonicalEntity(
        entity_type="flows",
        internal_id=flow_id,
        external_id=_flow_external_id(element),
        name=_flow_name(element, index),
        category_path=_category_path(element),
        raw=raw,
    )


def _flow_id(element, index: int) -> str:
    flow_uuid = _uuid_value(_flow_external_id(element))
    if flow_uuid:
        return flow_uuid
    return _stable_id(_flow_key_seed(_flow_key(element, index)))


def _flow_external_id(element) -> str | None:
    return (
        _attr(element, "intermediateExchangeId")
        or _attr(element, "elementaryExchangeId")
        or _attr(element, "id")
    )


def _flow_key(element, index: int) -> FlowKey:
    return (
        _key_text(etree.QName(element).localname),
        _key_text(_flow_type(element)),
        _key_text(_flow_name(element, index)),
        _key_text(_cas_number(element)),
        _key_text(_sum_formula(element)),
        _key_text(" / ".join(_category_path(element))),
        _key_text(_unit_name(element)),
    )


def _flow_key_seed(key: FlowKey) -> str:
    return "ecospold2/flow/" + "\x1f".join(key)


def _flow_name(element, index: int) -> str:
    return (
        _first_text(
            element,
            [
                "./*[local-name()='name']/text()",
                "./@name",
                ".//*[local-name()='name']/text()",
                ".//@name",
            ],
        )
        or f"EcoSpold 2 exchange {index}"
    )


def _process_id(
    *,
    label: str,
    index: int,
    name: str,
    activity_uuid: str | None,
    filename_uuids: list[str],
) -> str:
    if len(filename_uuids) == 1:
        return filename_uuids[0]
    if len(filename_uuids) > 1:
        return _stable_id(f"ecospold2/process/file/{Path(label).stem}")
    return activity_uuid or _stable_id(f"ecospold2/process/{label}/{index}/{name}")


def _exchange(element, flow: CanonicalEntity, index: int) -> dict[str, Any]:
    amount = _attr(element, "amount") or "0"
    return {
        "internalId": index,
        "sourceExchangeId": _source_exchange_id(element),
        "flow": {"@id": flow.internal_id, "name": flow.name},
        "flowRefId": flow.internal_id,
        "flowName": flow.name,
        "isInput": _is_input(element),
        "amount": amount,
        "activityLinkId": _attr(element, "activityLinkId"),
        "productionVolumeAmount": _attr(element, "productionVolumeAmount"),
        "isCalculatedAmount": _attr(element, "isCalculatedAmount"),
        "dataDerivationTypeStatus": _data_derivation_type(element),
        "uncertaintyDistributionType": _uncertainty_distribution_type(element),
        "generalComment": _first_text(
            element,
            [
                "./*[local-name()='comment']/*[local-name()='text']/text()",
                "./*[local-name()='comment']/text()",
            ],
        ),
        "sourceTrace": {
            "format": "ecospold2",
            "sourceObject": "exchange",
            "exchange": element_trace(element),
        },
    }


def _source_exchange_id(element) -> str | None:
    return _attr(element, "id") or _flow_external_id(element)


def _reference_first(exchanges: list[dict[str, Any]]) -> list[dict[str, Any]]:
    def sort_key(exchange: dict[str, Any]) -> tuple[int, int]:
        if exchange.get("isInput"):
            return (2, int(exchange["internalId"]))
        return (0, int(exchange["internalId"]))

    ordered = sorted(exchanges, key=sort_key)
    for index, exchange in enumerate(ordered, start=1):
        exchange["internalId"] = index
    return ordered


def _data_derivation_type(element) -> str | None:
    value = _attr(element, "isCalculatedAmount")
    if value and value.casefold() == "true":
        return "Calculated"
    return None


def _uncertainty_distribution_type(element) -> str | None:
    uncertainty = _first_element(element, "./*[local-name()='uncertainty']")
    if uncertainty is None:
        return None
    for child in uncertainty:
        if not isinstance(child.tag, str):
            continue
        local_name = etree.QName(child).localname.casefold()
        mapping = {
            "lognormal": "log-normal",
            "normal": "normal",
            "triangular": "triangular",
            "uniform": "uniform",
        }
        if local_name in mapping:
            return mapping[local_name]
    return None


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
            "./*[local-name()='compartment']/*[local-name()='compartment']/text()",
            "./*[local-name()='compartment']/@compartment",
        ],
    )
    subcompartment = _first_text(
        element,
        [
            "./*[local-name()='compartment']/*[local-name()='subcompartment']/text()",
            "./*[local-name()='compartment']/@subcompartment",
        ],
    )
    parts = [part for part in (compartment, subcompartment) if part]
    parts.extend(
        value
        for value in _all_texts(
            element, [".//*[local-name()='classificationValue']/text()"]
        )
        if value not in parts
    )
    return parts


def _cas_number(element) -> str | None:
    return _first_text(
        element,
        [
            "./*[local-name()='CASNumber']/text()",
            "./*[local-name()='casNumber']/text()",
            "./@CASNumber",
            "./@casNumber",
        ],
    )


def _sum_formula(element) -> str | None:
    return _first_text(
        element,
        [
            "./*[local-name()='sumFormula']/text()",
            "./*[local-name()='formula']/text()",
            "./@sumFormula",
            "./@formula",
            "./@chemicalFormula",
        ],
    )


def _location(dataset) -> str | None:
    value = _first_text(
        dataset,
        [
            ".//*[local-name()='activityDescription']/*[local-name()='geography']/*[local-name()='shortname']/text()",
            ".//*[local-name()='geography']/*[local-name()='shortname']/text()",
            ".//*[local-name()='geography']/@shortname",
            ".//*[local-name()='geography']/@location",
            ".//*[local-name()='activity']/@geography",
        ],
    )
    return _location_code(value)


def _location_code(value: str | None) -> str | None:
    if not value:
        return None
    text = value.strip()
    if text.casefold() == "row":
        return "GLO"
    if text.upper() == text and re.fullmatch(r"[A-Z]{2,7}|EU-[A-Z0-9&-]+", text):
        return text
    return None


def _reference_year(dataset) -> int | None:
    value = _first_text(
        dataset,
        [
            ".//*[local-name()='activityDescription']/*[local-name()='timePeriod']/*[local-name()='startDate']/text()",
            ".//*[local-name()='timePeriod']/@startDate",
            ".//*[local-name()='timePeriod']/@startYear",
            ".//*[local-name()='timePeriod']/*[local-name()='startYear']/text()",
            ".//*[local-name()='timePeriod']/*[local-name()='startDate']/text()",
        ],
    )
    if not value:
        return None
    match = YEAR_RE.search(value)
    if not match:
        return None
    return int(match.group("year"))


def _end_year(time_period) -> int | None:
    if time_period is None:
        return None
    value = _first_text(
        time_period,
        [
            "./*[local-name()='endDate']/text()",
            "./*[local-name()='endYear']/text()",
            "./@endDate",
            "./@endYear",
        ],
    )
    if not value:
        return None
    match = YEAR_RE.search(value)
    return int(match.group("year")) if match else None


def _time_description(time_period) -> str | None:
    if time_period is None:
        return None
    start = _first_text(
        time_period,
        [
            "./*[local-name()='startDate']/text()",
            "./*[local-name()='startYear']/text()",
            "./@startDate",
            "./@startYear",
        ],
    )
    end = _first_text(
        time_period,
        [
            "./*[local-name()='endDate']/text()",
            "./*[local-name()='endYear']/text()",
            "./@endDate",
            "./@endYear",
        ],
    )
    comment = _first_text(
        time_period,
        [
            "./*[local-name()='comment']/*[local-name()='text']/text()",
            "./*[local-name()='comment']/text()",
        ],
    )
    parts = []
    if start or end:
        parts.append(" - ".join(part for part in (start, end) if part))
    if comment:
        parts.append(comment)
    return "; ".join(parts) if parts else None


def _first_element(element, expression: str):
    if element is None:
        return None
    values = element.xpath(expression)
    return values[0] if values else None


def _first_text(element, expressions: list[str]) -> str | None:
    if element is None:
        return None
    for expression in expressions:
        values = element.xpath(expression)
        for value in values:
            if isinstance(value, str) and value.strip():
                return value.strip()
            text = getattr(value, "text", None)
            if text and text.strip():
                return text.strip()
    return None


def _all_texts(element, expressions: list[str]) -> list[str]:
    texts = []
    for expression in expressions:
        values = element.xpath(expression)
        for value in values:
            text = value if isinstance(value, str) else getattr(value, "text", None)
            if text and text.strip():
                texts.append(text.strip())
    return texts


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


def _uuids_from_label(label: str) -> list[str]:
    return [match.group("uuid").lower() for match in UUID_RE.finditer(Path(label).name)]


def _uuid_value(value: str | None) -> str | None:
    if not value:
        return None
    try:
        return str(uuid.UUID(str(value)))
    except ValueError:
        return None


def _stable_id(seed: str) -> str:
    return str(uuid.uuid5(uuid.NAMESPACE_URL, seed))
