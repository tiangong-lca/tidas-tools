"""ILCD (International Reference Life Cycle Data System) source adapter.

Reads a native ILCD 1.1 / EF3.x package (directory or zip) into the canonical
store, preserving original UUIDs, versions, classifications, exchanges, and the
source ``referenceToDigitalFile`` links. Entities are classified by their XML
root element namespace/local-name (not by directory prefix), so the classic
``ILCD/<type>/`` layout, the EPLCA ``data/<type>/`` layout, and flat layouts all
work. LCIA-method datasets are intentionally NOT mapped (reference/provenance
only — process-reachable cropping per the import governance proposal).
"""

from __future__ import annotations

from pathlib import Path
from typing import Any, Iterable
import zipfile

from lxml import etree

from .base import SourceAdapter
from ..model import CanonicalEntity
from ..report import ConversionReport
from ..store import MemoryCanonicalStore

# ILCD root element local-name -> canonical entity type. LCIAMethodDataSet is
# deliberately absent (out of import scope).
_ROOT_ENTITY_TYPES: dict[str, str] = {
    "contactDataSet": "contacts",
    "sourceDataSet": "sources",
    "unitGroupDataSet": "unitgroups",
    "flowPropertyDataSet": "flowproperties",
    "flowDataSet": "flows",
    "processDataSet": "processes",
}

# Directories that never hold importable ILCD datasets.
_SKIP_DIR_NAMES = {"schemas", "stylesheets", "external_docs", "other", "META-INF"}


class IlcdAdapter(SourceAdapter):
    """Read native ILCD XML datasets into the canonical store."""

    format_id = "ilcd"

    def read(
        self,
        source: str | Path,
        store: MemoryCanonicalStore,
        report: ConversionReport,
    ) -> None:
        count = 0
        for root, label in _iter_ilcd_payloads(Path(source)):
            local = _local_name(root.tag)
            entity_type = _ROOT_ENTITY_TYPES.get(local)
            if entity_type is None:
                continue
            entity = _to_entity(root, entity_type, label, report)
            if entity is None:
                report.add_issue(
                    severity="warning",
                    code="ilcd_dataset_missing_uuid",
                    message=f"Skipped ILCD {local} without a UUID.",
                    source_object=label,
                )
                continue
            store.add(entity)
            count += 1

        if count == 0:
            report.add_issue(
                severity="error",
                code="no_ilcd_datasets",
                message="No ILCD datasets were found in the source package.",
            )
            return

        report.add_issue(
            severity="warning",
            code="ilcd_semantic_mapping_with_trace",
            message=(
                "Mapped ILCD entities to TIDAS fields, preserved original UUIDs and "
                "source trace, and skipped LCIA-method datasets (reference-only)."
            ),
            context={"imported_entity_count": count},
        )


# ---------------------------------------------------------------------------
# Payload iteration (directory + zip), namespace-driven classification
# ---------------------------------------------------------------------------


def _iter_ilcd_payloads(source: Path) -> Iterable[tuple[Any, str]]:
    if source.is_dir():
        for path in sorted(source.rglob("*.xml")):
            if _skip_path(path.relative_to(source)):
                continue
            root = _parse_xml(path.read_bytes())
            if root is not None:
                yield root, path.relative_to(source).as_posix()
        return

    if source.suffix.lower() == ".zip":
        try:
            with zipfile.ZipFile(source) as archive:
                for name in sorted(archive.namelist()):
                    if not name.lower().endswith(".xml") or _skip_path(Path(name)):
                        continue
                    with archive.open(name) as stream:
                        root = _parse_xml(stream.read())
                    if root is not None:
                        yield root, name
        except zipfile.BadZipFile:
            return
        return

    # A single XML file.
    if source.suffix.lower() == ".xml":
        root = _parse_xml(source.read_bytes())
        if root is not None:
            yield root, source.name


def _skip_path(relative: Path) -> bool:
    return any(part in _SKIP_DIR_NAMES for part in relative.parts)


def _parse_xml(data: bytes):
    parser = etree.XMLParser(resolve_entities=False, no_network=True, recover=True)
    try:
        root = etree.fromstring(data, parser=parser)
    except etree.XMLSyntaxError:
        return None
    return root


def _local_name(tag: Any) -> str:
    if not isinstance(tag, str):
        return ""
    return tag.rsplit("}", 1)[-1]


# ---------------------------------------------------------------------------
# XML helpers (namespace-agnostic via local-name())
# ---------------------------------------------------------------------------


def _find(elem, *locals_: str):
    """First element matching the chain of direct-child local-names, or None."""
    if elem is None:
        return None
    current = [elem]
    for local in locals_:
        nxt = []
        for node in current:
            nxt.extend(node.xpath("./*[local-name()=$n]", n=local))
        if not nxt:
            return None
        current = nxt
    return current[0]


def _find_all(elem, local: str) -> list:
    if elem is None:
        return []
    return elem.xpath(".//*[local-name()=$n]", n=local)


def _text_of(elem) -> str | None:
    if elem is None or elem.text is None:
        return None
    text = elem.text.strip()
    return text or None


def _child_text(elem, *locals_: str) -> str | None:
    return _text_of(_find(elem, *locals_))


def _data_set_information(root):
    # <(...)Information>/<dataSetInformation>
    for info in root.xpath("./*[local-name()]"):
        dsi = _find(info, "dataSetInformation")
        if dsi is not None:
            return dsi
    return None


def _uuid(root) -> str | None:
    dsi = _data_set_information(root)
    if dsi is None:
        return None
    return _child_text(dsi, "UUID")


def _data_set_version(root) -> str | None:
    admin = _find(root, "administrativeInformation")
    if admin is None:
        return None
    return _child_text(admin, "publicationAndOwnership", "dataSetVersion")


def _classification_path(dsi) -> list[str]:
    classification = _find(dsi, "classificationInformation", "classification")
    if classification is None:
        classification = _find(
            dsi, "classificationInformation", "elementaryFlowCategorization"
        )
    if classification is None:
        return []
    nodes = classification.xpath("./*[local-name()='class' or local-name()='category']")
    ordered = sorted(nodes, key=lambda node: _int_attr(node.get("level")))
    return [text for node in ordered if (text := _text_of(node))]


def _int_attr(value: Any) -> int:
    try:
        return int(value)
    except (TypeError, ValueError):
        return 0


def _source_trace(label: str, source_object: str) -> dict[str, Any]:
    return {"format": "ilcd", "sourceObject": source_object, "file": label}


# ---------------------------------------------------------------------------
# Per-entity-type parsers
# ---------------------------------------------------------------------------


def _to_entity(
    root, entity_type: str, label: str, report: ConversionReport
) -> CanonicalEntity | None:
    uuid_value = _uuid(root)
    if not uuid_value:
        return None
    parser = _PARSERS[entity_type]
    name, raw = parser(root, label)
    raw.setdefault("sourceTrace", _source_trace(label, _local_name(root.tag)))
    version = _data_set_version(root)
    if version:
        raw.setdefault("version", version)
    return CanonicalEntity(
        entity_type=entity_type,
        internal_id=uuid_value,
        external_id=uuid_value,
        name=name,
        category_path=raw.pop("_category_path", []),
        raw=raw,
    )


def _parse_contact(root, label: str) -> tuple[str | None, dict[str, Any]]:
    dsi = _data_set_information(root)
    name = _child_text(dsi, "name") if dsi is not None else None
    short_name = _child_text(dsi, "shortName") if dsi is not None else None
    raw: dict[str, Any] = {}
    if short_name:
        raw["shortName"] = short_name
    if dsi is not None:
        for key, local in (
            ("email", "email"),
            ("telephone", "telephone"),
            ("website", "WWWAddress"),
            ("address", "contactAddress"),
            ("description", "contactDescriptionOrComment"),
        ):
            value = _child_text(dsi, local)
            if value:
                raw[key] = value
    return name or short_name, raw


def _parse_source(root, label: str) -> tuple[str | None, dict[str, Any]]:
    dsi = _data_set_information(root)
    short_name = _child_text(dsi, "shortName") if dsi is not None else None
    raw: dict[str, Any] = {}
    if dsi is not None:
        if short_name:
            raw["shortName"] = short_name
        citation = _child_text(dsi, "sourceCitation")
        if citation:
            raw["sourceCitation"] = citation
        publication_type = _child_text(dsi, "publicationType")
        if publication_type:
            raw["publicationType"] = publication_type
        description = _child_text(dsi, "sourceDescriptionOrComment")
        if description:
            raw["description"] = description
        # Preserve digital-file links. Local "../external_docs/*" refs stay as
        # referenceToDigitalFile (uploaded + rewritten downstream by the CLI);
        # plain http(s) URLs are kept as a source URL note (writer behavior).
        digital_files: list[str] = []
        for node in _find_all(dsi, "referenceToDigitalFile"):
            uri = (node.get("uri") or "").strip()
            if not uri:
                continue
            if uri.lower().startswith(("http://", "https://")):
                raw.setdefault("url", uri)
            else:
                digital_files.append(uri)
        if digital_files:
            raw["referenceToDigitalFile"] = digital_files
        raw["_category_path"] = _classification_path(dsi)
    return short_name, raw


def _parse_unit_group(root, label: str) -> tuple[str | None, dict[str, Any]]:
    dsi = _data_set_information(root)
    name = _child_text(dsi, "name") if dsi is not None else None
    reference_unit_id = _child_text(
        _find(root, "unitGroupInformation"),
        "quantitativeReference",
        "referenceToReferenceUnit",
    )
    units: list[dict[str, Any]] = []
    for unit in _find_all(root, "unit"):
        unit_name = _child_text(unit, "name")
        if not unit_name:
            continue
        internal_id = unit.get("dataSetInternalID")
        entry: dict[str, Any] = {"name": unit_name}
        factor = _child_text(unit, "meanValue")
        if factor is not None:
            entry["conversionFactor"] = factor
        if internal_id is not None and internal_id == reference_unit_id:
            entry["referenceUnit"] = True
        units.append(entry)
    raw: dict[str, Any] = {}
    if units:
        raw["units"] = units
    return name, raw


def _parse_flow_property(root, label: str) -> tuple[str | None, dict[str, Any]]:
    dsi = _data_set_information(root)
    name = _child_text(dsi, "name") if dsi is not None else None
    raw: dict[str, Any] = {}
    unit_group_refs = _find_all(root, "referenceToReferenceUnitGroup")
    unit_group_ref = unit_group_refs[0] if unit_group_refs else None
    if unit_group_ref is not None:
        ref_id = unit_group_ref.get("refObjectId")
        if ref_id:
            raw["unitGroupRefId"] = ref_id
            short = _child_text(unit_group_ref, "shortDescription")
            if short:
                raw["unitGroupName"] = short
    return name, raw


def _parse_flow(root, label: str) -> tuple[str | None, dict[str, Any]]:
    dsi = _data_set_information(root)
    name = _child_text(dsi, "name", "baseName") if dsi is not None else None
    raw: dict[str, Any] = {}
    type_of_data_set = _child_text(
        _find(root, "modellingAndValidation", "LCIMethod"), "typeOfDataSet"
    )
    if type_of_data_set:
        raw["flowType"] = type_of_data_set
    if dsi is not None:
        cas = _child_text(dsi, "CASNumber")
        if cas:
            raw["CASNumber"] = cas
        formula = _child_text(dsi, "sumFormula")
        if formula:
            raw["sumFormula"] = formula
        raw["_category_path"] = _classification_path(dsi)
        # Reference flow property: referenceToReferenceFlowProperty (internal id) lives
        # under flowInformation/quantitativeReference, NOT dataSetInformation. Resolve
        # it to flowProperty[@dataSetInternalID]/referenceToFlowPropertyDataSet.
        ref_internal = _child_text(
            _find(root, "flowInformation", "quantitativeReference"),
            "referenceToReferenceFlowProperty",
        )
        for flow_property in _find_all(root, "flowProperty"):
            if flow_property.get("dataSetInternalID") != ref_internal:
                continue
            ref = _find(flow_property, "referenceToFlowPropertyDataSet")
            if ref is not None and ref.get("refObjectId"):
                raw["flowPropertyRefId"] = ref.get("refObjectId")
                short = _child_text(ref, "shortDescription")
                if short:
                    raw["flowPropertyName"] = short
            break
    return name, raw


def _parse_process(root, label: str) -> tuple[str | None, dict[str, Any]]:
    process_information = _find(root, "processInformation")
    dsi = _data_set_information(root)
    name = None
    if dsi is not None:
        name = _child_text(dsi, "name", "baseName") or _child_text(dsi, "baseName")
    raw: dict[str, Any] = {}

    if process_information is not None:
        reference_flow_id = _child_text(
            process_information, "quantitativeReference", "referenceToReferenceFlow"
        )
        geography = _find(
            process_information, "geography", "locationOfOperationSupplyOrProduction"
        )
        if geography is not None and geography.get("location"):
            raw["location"] = geography.get("location")
        reference_year = _child_text(process_information, "time", "referenceYear")
        if reference_year:
            raw["referenceYear"] = reference_year
        comment = _child_text(dsi, "generalComment") if dsi is not None else None
        if comment:
            raw["description"] = comment
        raw["exchanges"] = _parse_exchanges(root, reference_flow_id)

    if dsi is not None:
        raw["_category_path"] = _classification_path(dsi)
    return name, raw


def _parse_exchanges(root, reference_flow_id: str | None) -> list[dict[str, Any]]:
    exchanges: list[dict[str, Any]] = []
    for exchange in _find_all(root, "exchange"):
        ref = _find(exchange, "referenceToFlowDataSet")
        flow_ref_id = ref.get("refObjectId") if ref is not None else None
        if not flow_ref_id:
            continue
        internal_id = exchange.get("dataSetInternalID")
        amount = _child_text(exchange, "resultingAmount") or _child_text(
            exchange, "meanAmount"
        )
        direction = (_child_text(exchange, "exchangeDirection") or "").lower()
        entry: dict[str, Any] = {
            "internalId": internal_id,
            "flowRefId": flow_ref_id,
            "flowName": (
                _child_text(ref, "shortDescription") if ref is not None else None
            ),
            "isInput": direction == "input",
            "amount": amount if amount is not None else "1",
            "isQuantitativeReference": (
                internal_id is not None and internal_id == reference_flow_id
            ),
        }
        location = _child_text(exchange, "location")
        if location:
            entry["location"] = location
        std_dev = _child_text(exchange, "relativeStandardDeviation95In")
        if std_dev:
            entry["relativeStandardDeviation95In"] = std_dev
        derivation = _child_text(exchange, "dataDerivationTypeStatus")
        if derivation:
            entry["dataDerivationTypeStatus"] = derivation
        comment = _child_text(exchange, "generalComment")
        if comment:
            entry["generalComment"] = comment
        exchanges.append({k: v for k, v in entry.items() if v is not None})
    return exchanges


_PARSERS = {
    "contacts": _parse_contact,
    "sources": _parse_source,
    "unitgroups": _parse_unit_group,
    "flowproperties": _parse_flow_property,
    "flows": _parse_flow,
    "processes": _parse_process,
}
