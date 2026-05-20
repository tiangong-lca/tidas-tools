"""SimaPro CSV block-format source adapter."""

from __future__ import annotations

from pathlib import Path
from typing import Any
import uuid

from .base import SourceAdapter
from ..model import CanonicalEntity
from ..report import ConversionReport
from ..store import MemoryCanonicalStore


class SimaProCsvAdapter(SourceAdapter):
    """Read SimaPro CSV process blocks into the canonical store."""

    format_id = "simapro-csv"

    def read(
        self,
        source: str | Path,
        store: MemoryCanonicalStore,
        report: ConversionReport,
    ) -> None:
        text = _read_text(Path(source))
        separator = _separator(text)
        processes = _process_blocks(text.splitlines())
        for index, sections in enumerate(processes, start=1):
            process = _process_entity(sections, source, index)
            exchanges = []
            exchange_index = 0
            for section_name, rows in sections.items():
                for row in rows:
                    exchange = _exchange(
                        section_name, row, separator, exchange_index + 1
                    )
                    if exchange is None:
                        continue
                    exchange_index += 1
                    flow = _flow_entity(section_name, exchange, source)
                    store.add(flow)
                    exchange["flow"] = {"@id": flow.internal_id, "name": flow.name}
                    exchange["flowRefId"] = flow.internal_id
                    exchange["flowName"] = flow.name
                    exchanges.append(exchange)
            process.raw["exchanges"] = exchanges
            store.add(process)
            report.add_issue(
                severity="warning",
                code="simapro_csv_minimal_mapping",
                message="Mapped SimaPro CSV process with the current minimal adapter.",
                source_object=str(source),
                context={
                    "process_id": process.internal_id,
                    "exchange_count": len(exchanges),
                },
            )

        if not processes:
            report.add_issue(
                severity="error",
                code="no_simapro_process_blocks",
                message="No SimaPro Process blocks were found.",
            )


def _read_text(path: Path) -> str:
    data = path.read_bytes()
    for encoding in ("cp1252", "utf-8-sig", "utf-8"):
        try:
            return data.decode(encoding)
        except UnicodeDecodeError:
            continue
    return data.decode("utf-8", errors="replace")


def _separator(text: str) -> str:
    for line in text.splitlines()[:30]:
        if line.startswith("{CSV separator:"):
            value = line.removeprefix("{CSV separator:").removesuffix("}").strip()
            if value.lower() == "semicolon":
                return ";"
            if value.lower() == "comma":
                return ","
            if value:
                return value[0]
    return ";"


def _process_blocks(lines: list[str]) -> list[dict[str, list[str]]]:
    blocks = []
    index = 0
    while index < len(lines):
        if lines[index].strip() != "Process":
            index += 1
            continue
        index += 1
        sections: dict[str, list[str]] = {}
        while index < len(lines):
            line = lines[index].strip()
            if line == "End":
                blocks.append(sections)
                break
            if not line:
                index += 1
                continue
            section_name = line
            index += 1
            rows = []
            while index < len(lines):
                row = lines[index].strip()
                if not row:
                    break
                if row == "End":
                    index -= 1
                    break
                rows.append(row)
                index += 1
            sections[section_name] = rows
            index += 1
        index += 1
    return blocks


def _process_entity(
    sections: dict[str, list[str]], source: str | Path, index: int
) -> CanonicalEntity:
    name = _first(sections, "Process name") or f"SimaPro process {index}"
    process_id = _stable_id(f"simapro/process/{source}/{index}/{name}")
    return CanonicalEntity(
        entity_type="processes",
        internal_id=process_id,
        name=name,
        raw={
            "description": _first(sections, "Comment") or "Imported from SimaPro CSV."
        },
    )


def _flow_entity(section_name: str, exchange: dict[str, Any], source: str | Path):
    name = exchange["flowName"]
    flow_id = _stable_id(f"simapro/flow/{source}/{section_name}/{name}")
    return CanonicalEntity(
        entity_type="flows",
        internal_id=flow_id,
        name=name,
        category_path=[section_name],
        raw={
            "flowType": _flow_type(section_name),
            "unitName": exchange.get("unitName"),
        },
    )


def _exchange(
    section_name: str,
    row: str,
    separator: str,
    row_index: int,
) -> dict[str, Any] | None:
    if section_name not in _EXCHANGE_SECTIONS:
        return None
    parts = [part.strip().replace("\x7f", "\n") for part in row.split(separator)]
    if not parts or not parts[0]:
        return None
    indexes = _EXCHANGE_SECTIONS[section_name]
    amount = _get(parts, indexes["amount"]) or "0"
    return {
        "internalId": row_index,
        "flowName": parts[0],
        "isInput": indexes["direction"] == "Input",
        "amount": _numeric(amount),
        "unitName": _get(parts, indexes["unit"]),
    }


_EXCHANGE_SECTIONS = {
    "Products": {"amount": 2, "unit": 1, "direction": "Output"},
    "Avoided products": {"amount": 2, "unit": 1, "direction": "Output"},
    "Materials/fuels": {"amount": 2, "unit": 1, "direction": "Input"},
    "Electricity/heat": {"amount": 2, "unit": 1, "direction": "Input"},
    "Resources": {"amount": 3, "unit": 2, "direction": "Input"},
    "Emissions to air": {"amount": 3, "unit": 2, "direction": "Output"},
    "Emissions to water": {"amount": 3, "unit": 2, "direction": "Output"},
    "Emissions to soil": {"amount": 3, "unit": 2, "direction": "Output"},
    "Final waste flows": {"amount": 3, "unit": 2, "direction": "Output"},
    "Non material emissions": {"amount": 3, "unit": 2, "direction": "Output"},
    "Social issues": {"amount": 3, "unit": 2, "direction": "Output"},
    "Economic issues": {"amount": 3, "unit": 2, "direction": "Output"},
    "Waste to treatment": {"amount": 3, "unit": 2, "direction": "Input"},
}


def _flow_type(section_name: str) -> str:
    if section_name in {"Final waste flows", "Waste to treatment"}:
        return "WASTE_FLOW"
    if section_name in {
        "Resources",
        "Emissions to air",
        "Emissions to water",
        "Emissions to soil",
        "Non material emissions",
        "Social issues",
        "Economic issues",
    }:
        return "ELEMENTARY_FLOW"
    return "PRODUCT_FLOW"


def _numeric(value: str) -> str:
    value = value.strip().replace(",", ".")
    try:
        return f"{float(value):g}"
    except ValueError:
        return "0"


def _first(sections: dict[str, list[str]], key: str) -> str | None:
    rows = sections.get(key) or []
    return rows[0] if rows else None


def _get(parts: list[str], index: int) -> str | None:
    return parts[index] if index < len(parts) else None


def _stable_id(seed: str) -> str:
    return str(uuid.uuid5(uuid.NAMESPACE_URL, seed))
