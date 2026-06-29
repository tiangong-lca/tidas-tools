"""Format detection for external LCA sources.

The rules mirror the openLCA format-dispatch behavior documented in
``multi_format_to_ilcd_tidas_implementation_plan.md`` while staying fully
Python native.
"""

from __future__ import annotations

from collections import defaultdict
from dataclasses import dataclass
import json
from pathlib import Path
from typing import Iterable
import zipfile

from lxml import etree

from .errors import AmbiguousFormatError

FORMAT_ECOSPOLD1 = "ecospold1"
FORMAT_ECOSPOLD2 = "ecospold2"
FORMAT_SIMAPRO_CSV = "simapro-csv"
FORMAT_OPENLCA_JSONLD = "openlca-jsonld"
FORMAT_OPENLCA_PROCESS_XLSX = "openlca-process-xlsx"
FORMAT_ILCD = "ilcd"
FORMAT_UNSUPPORTED_ZOLCA = "unsupported-zolca"
FORMAT_UNKNOWN = "unknown"

SUPPORTED_FORMATS = {
    FORMAT_ECOSPOLD1,
    FORMAT_ECOSPOLD2,
    FORMAT_SIMAPRO_CSV,
    FORMAT_OPENLCA_JSONLD,
    FORMAT_OPENLCA_PROCESS_XLSX,
    FORMAT_ILCD,
}

CONFIDENCE_SCORE = {
    "high": 3,
    "medium": 2,
    "low": 1,
}


@dataclass(frozen=True)
class DetectedFormat:
    """Detected source format plus evidence used for the decision."""

    format_id: str
    confidence: str
    evidence: list[str]


@dataclass(frozen=True)
class _Candidate:
    format_id: str
    confidence: str
    evidence: str

    @property
    def score(self) -> int:
        return CONFIDENCE_SCORE[self.confidence]


def detect_format(source: str | Path, requested_format: str = "auto") -> DetectedFormat:
    """Detect the external LCA source format.

    If ``requested_format`` is not ``auto``, it is treated as an explicit user
    override and returned directly.
    """

    path = Path(source)
    if not path.exists():
        raise FileNotFoundError(path)

    if path.suffix.lower() == ".zolca":
        return DetectedFormat(
            format_id=FORMAT_UNSUPPORTED_ZOLCA,
            confidence="high",
            evidence=["file extension .zolca"],
        )

    if requested_format != "auto":
        return DetectedFormat(
            format_id=requested_format,
            confidence="high",
            evidence=[f"explicit --from-format {requested_format}"],
        )

    candidates = list(_scan_path(path))
    if not candidates:
        return DetectedFormat(
            format_id=FORMAT_UNKNOWN,
            confidence="low",
            evidence=["no supported LCA format signature found"],
        )

    return _select_candidate(candidates)


def _select_candidate(candidates: list[_Candidate]) -> DetectedFormat:
    unsupported_zolca = [
        candidate
        for candidate in candidates
        if candidate.format_id == FORMAT_UNSUPPORTED_ZOLCA
    ]
    if unsupported_zolca:
        return DetectedFormat(
            format_id=FORMAT_UNSUPPORTED_ZOLCA,
            confidence="high",
            evidence=[candidate.evidence for candidate in unsupported_zolca],
        )

    grouped: dict[str, list[_Candidate]] = defaultdict(list)
    for candidate in candidates:
        grouped[candidate.format_id].append(candidate)

    aggregate_scores = {
        format_id: sum(candidate.score for candidate in items)
        for format_id, items in grouped.items()
    }
    top_score = max(aggregate_scores.values())
    top_formats = [
        format_id for format_id, score in aggregate_scores.items() if score == top_score
    ]

    if len(top_formats) > 1:
        details = ", ".join(
            f"{format_id}: {_summarize_candidates(grouped[format_id])}"
            for format_id in sorted(top_formats)
        )
        raise AmbiguousFormatError(
            f"Ambiguous source format detection; candidates: {details}"
        )

    selected_format = top_formats[0]
    selected_candidates = sorted(
        grouped[selected_format], key=lambda candidate: candidate.score, reverse=True
    )
    confidence = selected_candidates[0].confidence
    evidence = _limited_evidence(selected_candidates)
    for format_id, items in sorted(grouped.items()):
        if format_id == selected_format:
            continue
        evidence.extend(_limited_evidence(items, prefix=f"secondary {format_id}: "))

    return DetectedFormat(
        format_id=selected_format,
        confidence=confidence,
        evidence=evidence,
    )


def _summarize_candidates(candidates: list[_Candidate]) -> str:
    first = candidates[0].evidence
    extra = len(candidates) - 1
    return first if extra <= 0 else f"{first} (+{extra} more)"


def _limited_evidence(
    candidates: list[_Candidate],
    *,
    prefix: str = "",
    limit: int = 25,
) -> list[str]:
    evidence = [prefix + candidate.evidence for candidate in candidates[:limit]]
    extra = len(candidates) - limit
    if extra > 0:
        evidence.append(f"{prefix}{extra} additional matching signatures omitted")
    return evidence


def _scan_path(path: Path) -> Iterable[_Candidate]:
    if path.is_dir():
        yield from _scan_directory(path)
        return

    suffix = path.suffix.lower()
    if suffix == ".spold":
        yield _Candidate(FORMAT_ECOSPOLD2, "high", "file extension .spold")
        return
    if suffix in {".csv", ".txt"}:
        yield from _scan_csv_bytes(_safe_read_bytes(path, 4096), str(path))
        return
    if suffix in {".xml", ".spold"}:
        yield from _scan_xml_bytes(_safe_read_bytes(path, 1024 * 1024), str(path))
        return
    if suffix == ".xlsx":
        yield from _scan_xlsx(path)
        return
    if suffix == ".zip":
        yield from _scan_zip(path)
        return
    if suffix in {".json", ".jsonld"}:
        yield from _scan_json_bytes(_safe_read_bytes(path, 1024 * 1024), str(path))


def _scan_directory(path: Path) -> Iterable[_Candidate]:
    if (path / "context.jsonld").is_file():
        yield _Candidate(FORMAT_OPENLCA_JSONLD, "high", "directory has context.jsonld")

    for child in sorted(path.rglob("*")):
        if not child.is_file():
            continue
        suffix = child.suffix.lower()
        if suffix == ".zolca":
            yield _Candidate(FORMAT_UNSUPPORTED_ZOLCA, "high", f"{child}: .zolca file")
            continue
        if suffix == ".spold":
            yield _Candidate(FORMAT_ECOSPOLD2, "high", f"{child}: .spold file")
            continue
        if suffix in {".csv", ".txt"}:
            yield from _scan_csv_bytes(_safe_read_bytes(child, 4096), str(child))
            continue
        if suffix == ".xml":
            yield from _scan_xml_bytes(_safe_read_bytes(child, 1024 * 1024), str(child))
            continue
        if suffix in {".json", ".jsonld"}:
            yield from _scan_json_bytes(
                _safe_read_bytes(child, 1024 * 1024), str(child)
            )
            continue
        if suffix == ".xlsx":
            yield from _scan_xlsx(child)


def _scan_zip(path: Path) -> Iterable[_Candidate]:
    try:
        with zipfile.ZipFile(path) as archive:
            names = archive.namelist()
            if "context.jsonld" in {Path(name).name for name in names}:
                yield _Candidate(
                    FORMAT_OPENLCA_JSONLD,
                    "high",
                    f"{path}: zip contains context.jsonld",
                )

            for name in names:
                lower_name = name.lower()
                if lower_name.endswith("/"):
                    continue
                if lower_name.endswith(".zolca"):
                    yield _Candidate(
                        FORMAT_UNSUPPORTED_ZOLCA,
                        "high",
                        f"{path}: zip entry {name} is .zolca",
                    )
                    continue
                if lower_name.endswith(".spold"):
                    yield _Candidate(
                        FORMAT_ECOSPOLD2,
                        "high",
                        f"{path}: zip entry {name} has .spold extension",
                    )
                    continue
                if lower_name.endswith((".csv", ".txt")):
                    yield from _scan_csv_bytes(
                        _safe_read_zip_entry(archive, name, 4096),
                        f"{path}:{name}",
                    )
                    continue
                if lower_name.endswith(".xml"):
                    yield from _scan_xml_bytes(
                        _safe_read_zip_entry(archive, name, 1024 * 1024),
                        f"{path}:{name}",
                    )
                    continue
                if lower_name.endswith((".json", ".jsonld")):
                    yield from _scan_json_bytes(
                        _safe_read_zip_entry(archive, name, 1024 * 1024),
                        f"{path}:{name}",
                    )
    except zipfile.BadZipFile:
        return


def _scan_xlsx(path: Path) -> Iterable[_Candidate]:
    try:
        with zipfile.ZipFile(path) as archive:
            names = set(archive.namelist())
    except zipfile.BadZipFile:
        return

    if "[Content_Types].xml" in names and "xl/workbook.xml" in names:
        yield _Candidate(
            FORMAT_OPENLCA_PROCESS_XLSX,
            "medium",
            f"{path}: valid XLSX workbook container",
        )


def _scan_csv_bytes(data: bytes, source_label: str) -> Iterable[_Candidate]:
    first_line = (
        data.splitlines()[0].decode("utf-8-sig", errors="ignore") if data else ""
    )
    if first_line.startswith("{SimaPro "):
        yield _Candidate(
            FORMAT_SIMAPRO_CSV,
            "high",
            f"{source_label}: first line starts with '{{SimaPro '",
        )


def _scan_xml_bytes(data: bytes, source_label: str) -> Iterable[_Candidate]:
    if not data.strip():
        return
    parser = etree.XMLParser(resolve_entities=False, no_network=True, recover=True)
    try:
        root = etree.fromstring(data, parser=parser)
    except etree.XMLSyntaxError:
        return

    namespace, local_name = _split_xml_tag(root.tag)
    signature = f"{namespace} {local_name}".lower()
    if "ecospold02" in signature or local_name.lower() == "ecospold2":
        yield _Candidate(
            FORMAT_ECOSPOLD2,
            "high",
            f"{source_label}: XML root {{{namespace}}}{local_name}",
        )
        return
    if "ecospold01" in signature or local_name.lower() == "ecospold":
        yield _Candidate(
            FORMAT_ECOSPOLD1,
            "high" if "ecospold01" in signature else "medium",
            f"{source_label}: XML root {{{namespace}}}{local_name}",
        )
        return
    if "lca.jrc.it/ilcd" in signature and local_name.lower().endswith("dataset"):
        yield _Candidate(
            FORMAT_ILCD,
            "high",
            f"{source_label}: ILCD XML root {{{namespace}}}{local_name}",
        )


def _scan_json_bytes(data: bytes, source_label: str) -> Iterable[_Candidate]:
    if not data.strip():
        return
    try:
        payload = json.loads(data.decode("utf-8-sig"))
    except (json.JSONDecodeError, UnicodeDecodeError):
        return

    if not isinstance(payload, dict):
        return

    context = payload.get("@context")
    entity_type = payload.get("@type")
    if _looks_like_openlca_context(context) or _looks_like_openlca_type(entity_type):
        yield _Candidate(
            FORMAT_OPENLCA_JSONLD,
            "high",
            f"{source_label}: JSON-LD @context/@type looks like openLCA",
        )
        return

    if any(key in payload for key in ("@id", "refId", "version")) and any(
        key in payload for key in ("name", "category", "flowType", "processType")
    ):
        yield _Candidate(
            FORMAT_OPENLCA_JSONLD,
            "medium",
            f"{source_label}: JSON object has openLCA-like fields",
        )


def _looks_like_openlca_context(context: object) -> bool:
    if isinstance(context, str):
        lowered = context.lower()
        return "openlca" in lowered or "olca" in lowered or "context.jsonld" in lowered
    if isinstance(context, list):
        return any(_looks_like_openlca_context(item) for item in context)
    if isinstance(context, dict):
        keys = {str(key).lower() for key in context.keys()}
        return "olca" in keys or "openlca" in keys
    return False


def _looks_like_openlca_type(entity_type: object) -> bool:
    known_types = {
        "actor",
        "source",
        "unitgroup",
        "flowproperty",
        "flow",
        "process",
        "impactcategory",
        "impactmethod",
        "productsystem",
        "epd",
    }
    if not isinstance(entity_type, str):
        return False
    return entity_type.replace(" ", "").lower() in known_types


def _split_xml_tag(tag: str) -> tuple[str, str]:
    if tag.startswith("{") and "}" in tag:
        namespace, local_name = tag[1:].split("}", 1)
        return namespace, local_name
    return "", tag


def _safe_read_bytes(path: Path, limit: int) -> bytes:
    with path.open("rb") as stream:
        return stream.read(limit)


def _safe_read_zip_entry(
    archive: zipfile.ZipFile,
    name: str,
    limit: int,
) -> bytes:
    try:
        with archive.open(name) as stream:
            return stream.read(limit)
    except (KeyError, RuntimeError, zipfile.BadZipFile):
        return b""
