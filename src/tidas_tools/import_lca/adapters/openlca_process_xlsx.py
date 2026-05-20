"""openLCA process workbook source adapter."""

from __future__ import annotations

from pathlib import Path, PurePosixPath
from typing import Any
import re
import uuid
import zipfile

from lxml import etree

from .base import SourceAdapter
from ..model import CanonicalEntity
from ..report import ConversionReport
from ..store import MemoryCanonicalStore

_REL_NS = "http://schemas.openxmlformats.org/officeDocument/2006/relationships"
_PKG_REL_NS = "http://schemas.openxmlformats.org/package/2006/relationships"
_SS_NS = "http://schemas.openxmlformats.org/spreadsheetml/2006/main"


class OpenLcaProcessXlsxAdapter(SourceAdapter):
    """Read openLCA process XLSX workbooks into the canonical store."""

    format_id = "openlca-process-xlsx"

    def read(
        self,
        source: str | Path,
        store: MemoryCanonicalStore,
        report: ConversionReport,
    ) -> None:
        workbook = _Workbook(Path(source))
        process = _process_entity(workbook, source)
        if process is None:
            report.add_issue(
                severity="error",
                code="invalid_process_xlsx",
                message="Workbook does not contain openLCA General information.",
                source_object=str(source),
            )
            return

        flows = _flows(workbook, source)
        exchanges = _exchanges(workbook, flows, source)
        for flow in flows.values():
            store.add(flow)
        for exchange in exchanges:
            flow = exchange.pop("_flow_entity", None)
            if flow is not None:
                store.add(flow)

        process.raw["exchanges"] = exchanges
        store.add(process)
        report.add_issue(
            severity="warning",
            code="process_xlsx_minimal_mapping",
            message=(
                "Mapped openLCA process workbook with the current minimal adapter."
            ),
            source_object=str(source),
            context={
                "process_id": process.internal_id,
                "exchange_count": len(exchanges),
            },
        )


def _process_entity(
    workbook: "_Workbook", source: str | Path
) -> CanonicalEntity | None:
    info = _general_info(workbook)
    if not info:
        return None
    process_id = info.get("uuid")
    name = info.get("name") or Path(source).stem
    internal_id = (
        process_id
        if _is_uuid(process_id)
        else _stable_id(f"openlca-process-xlsx/process/{source}/{name}")
    )
    return CanonicalEntity(
        entity_type="processes",
        internal_id=internal_id,
        external_id=process_id,
        name=name,
        category_path=_split_category(info.get("category")),
        raw={"description": info.get("description") or "Imported from openLCA XLSX."},
    )


def _general_info(workbook: "_Workbook") -> dict[str, str]:
    rows = workbook.rows("General information")
    start = None
    for index, row in enumerate(rows):
        if _norm(_get(row, 0)) == "general information":
            start = index + 1
            break
    if start is None:
        return {}
    info: dict[str, str] = {}
    for row in rows[start:]:
        field = _norm(_get(row, 0))
        if not field:
            break
        value = _string(_get(row, 1))
        if value is not None:
            info[field] = value
    return info


def _flows(
    workbook: "_Workbook", source: str | Path
) -> dict[tuple[str, str], CanonicalEntity]:
    result: dict[tuple[str, str], CanonicalEntity] = {}
    for row in _table_rows(workbook, "Flows"):
        name = row.get("name")
        if not name:
            continue
        category = row.get("category") or ""
        flow_id = row.get("uuid")
        internal_id = (
            flow_id
            if _is_uuid(flow_id)
            else _stable_id(f"openlca-process-xlsx/flow/{source}/{name}/{category}")
        )
        entity = CanonicalEntity(
            entity_type="flows",
            internal_id=internal_id,
            external_id=flow_id,
            name=name,
            category_path=_split_category(category),
            raw={"flowType": _flow_type(row.get("type"))},
        )
        result[_flow_key(name, category)] = entity
    return result


def _exchanges(
    workbook: "_Workbook",
    flows: dict[tuple[str, str], CanonicalEntity],
    source: str | Path,
) -> list[dict[str, Any]]:
    exchanges: list[dict[str, Any]] = []
    output_rows = sorted(
        _table_rows(workbook, "Outputs"),
        key=lambda row: 0 if _truthy(row.get("is reference?")) else 1,
    )
    for row in output_rows:
        exchange = _exchange(row, False, len(exchanges) + 1, flows, source)
        if exchange is not None:
            exchanges.append(exchange)
    for row in _table_rows(workbook, "Inputs"):
        exchange = _exchange(row, True, len(exchanges) + 1, flows, source)
        if exchange is not None:
            exchanges.append(exchange)
    return exchanges


def _exchange(
    row: dict[str, str],
    is_input: bool,
    index: int,
    flows: dict[tuple[str, str], CanonicalEntity],
    source: str | Path,
) -> dict[str, Any] | None:
    flow_name = row.get("flow")
    if not flow_name:
        return None
    category = row.get("category") or ""
    key = _flow_key(flow_name, category)
    flow = flows.get(key)
    if flow is None:
        flow = CanonicalEntity(
            entity_type="flows",
            internal_id=_stable_id(
                f"openlca-process-xlsx/exchange-flow/{source}/{flow_name}/{category}"
            ),
            name=flow_name,
            category_path=_split_category(category),
            raw={"flowType": "PRODUCT_FLOW"},
        )
        flows[key] = flow
    if row.get("unit"):
        flow.raw["unitName"] = row["unit"]
    return {
        "internalId": index,
        "flow": {"@id": flow.internal_id, "name": flow.name},
        "flowRefId": flow.internal_id,
        "flowName": flow.name,
        "isInput": is_input,
        "amount": row.get("amount") or "0",
        "_flow_entity": flow,
    }


def _table_rows(workbook: "_Workbook", sheet_name: str) -> list[dict[str, str]]:
    rows = workbook.rows(sheet_name)
    if not rows:
        return []
    header_index = next(
        (index for index, row in enumerate(rows) if any(_string(cell) for cell in row)),
        None,
    )
    if header_index is None:
        return []
    headers = [_norm(cell) for cell in rows[header_index]]
    result = []
    for row in rows[header_index + 1 :]:
        if not any(_string(cell) for cell in row):
            continue
        item = {}
        for index, header in enumerate(headers):
            if not header:
                continue
            value = _string(_get(row, index))
            if value is not None:
                item[header] = value
        result.append(item)
    return result


class _Workbook:
    def __init__(self, path: Path) -> None:
        self.path = path
        with zipfile.ZipFile(path) as archive:
            self.shared_strings = _shared_strings(archive)
            self.sheet_paths = _sheet_paths(archive)
            self._rows = {
                name: _sheet_rows(archive, sheet_path, self.shared_strings)
                for name, sheet_path in self.sheet_paths.items()
            }

    def rows(self, sheet_name: str) -> list[list[Any]]:
        return self._rows.get(sheet_name, [])


def _sheet_paths(archive: zipfile.ZipFile) -> dict[str, str]:
    workbook_xml = etree.fromstring(archive.read("xl/workbook.xml"))
    rels_xml = etree.fromstring(archive.read("xl/_rels/workbook.xml.rels"))
    rels = {
        rel.get("Id"): rel.get("Target")
        for rel in rels_xml.xpath(
            "./rel:Relationship",
            namespaces={"rel": _PKG_REL_NS},
        )
    }
    sheet_paths = {}
    for sheet in workbook_xml.xpath(".//ss:sheet", namespaces={"ss": _SS_NS}):
        name = sheet.get("name")
        rel_id = sheet.get(f"{{{_REL_NS}}}id")
        target = rels.get(rel_id)
        if name and target:
            sheet_paths[name] = _resolve_xl_target(target)
    return sheet_paths


def _resolve_xl_target(target: str) -> str:
    if target.startswith("/"):
        return target.lstrip("/")
    return str(PurePosixPath("xl") / target)


def _shared_strings(archive: zipfile.ZipFile) -> list[str]:
    try:
        root = etree.fromstring(archive.read("xl/sharedStrings.xml"))
    except KeyError:
        return []
    strings = []
    for item in root.xpath(".//ss:si", namespaces={"ss": _SS_NS}):
        parts = item.xpath(".//ss:t/text()", namespaces={"ss": _SS_NS})
        strings.append("".join(parts))
    return strings


def _sheet_rows(
    archive: zipfile.ZipFile,
    sheet_path: str,
    shared_strings: list[str],
) -> list[list[Any]]:
    root = etree.fromstring(archive.read(sheet_path))
    rows = []
    for row in root.xpath(".//ss:sheetData/ss:row", namespaces={"ss": _SS_NS}):
        values: list[Any] = []
        for cell in row.xpath("./ss:c", namespaces={"ss": _SS_NS}):
            index = _column_index(cell.get("r"))
            while len(values) <= index:
                values.append(None)
            values[index] = _cell_value(cell, shared_strings)
        rows.append(values)
    return rows


def _cell_value(cell, shared_strings: list[str]) -> Any:
    cell_type = cell.get("t")
    if cell_type == "inlineStr":
        text = cell.xpath("./ss:is/ss:t/text()", namespaces={"ss": _SS_NS})
        return "".join(text)
    value = _first(cell.xpath("./ss:v/text()", namespaces={"ss": _SS_NS}))
    if value is None:
        return None
    if cell_type == "s":
        try:
            return shared_strings[int(value)]
        except (IndexError, ValueError):
            return None
    if cell_type == "b":
        return value == "1"
    return value


def _column_index(cell_ref: str | None) -> int:
    if not cell_ref:
        return 0
    letters = re.match(r"[A-Z]+", cell_ref.upper())
    if not letters:
        return 0
    index = 0
    for char in letters.group(0):
        index = index * 26 + ord(char) - ord("A") + 1
    return index - 1


def _flow_key(name: str, category: str | None) -> tuple[str, str]:
    return (_norm(name), _norm(category))


def _flow_type(value: str | None) -> str:
    text = _norm(value)
    if text.startswith("w"):
        return "WASTE_FLOW"
    if text.startswith("p"):
        return "PRODUCT_FLOW"
    return "ELEMENTARY_FLOW"


def _split_category(value: str | None) -> list[str]:
    if not value:
        return []
    return [part.strip() for part in str(value).split("/") if part.strip()]


def _truthy(value: str | None) -> bool:
    return _norm(value) in {"true", "yes", "y", "1", "x"}


def _get(row: list[Any], index: int) -> Any:
    return row[index] if index < len(row) else None


def _first(values: list[Any]) -> Any:
    return values[0] if values else None


def _string(value: Any) -> str | None:
    if value is None:
        return None
    if isinstance(value, bool):
        return "true" if value else "false"
    text = str(value).strip()
    return text or None


def _norm(value: Any) -> str:
    text = _string(value)
    return text.lower() if text else ""


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
