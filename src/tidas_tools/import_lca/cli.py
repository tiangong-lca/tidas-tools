"""Command line interface for external LCA imports."""

from __future__ import annotations

import argparse
import logging
from pathlib import Path
import sys

if __package__:
    from .adapters.ecospold1 import EcoSpold1Adapter
    from .adapters.ecospold2 import EcoSpold2Adapter
    from .adapters.openlca_jsonld import OpenLcaJsonLdAdapter
    from .adapters.openlca_process_xlsx import OpenLcaProcessXlsxAdapter
    from .adapters.simapro_csv import SimaProCsvAdapter
    from .detect import (
        FORMAT_UNKNOWN,
        FORMAT_UNSUPPORTED_ZOLCA,
        SUPPORTED_FORMATS,
        detect_format,
    )
    from .errors import AmbiguousFormatError
    from .report import ConversionReport
    from .store import MemoryCanonicalStore
    from .writers import write_ilcd_from_tidas, write_tidas_package
    from ..validate import validate_ilcd_package_dir, validate_package_dir
    from ..tidas_log import setup_logging
else:
    from tidas_tools.import_lca.adapters.ecospold1 import EcoSpold1Adapter
    from tidas_tools.import_lca.adapters.ecospold2 import EcoSpold2Adapter
    from tidas_tools.import_lca.adapters.openlca_jsonld import OpenLcaJsonLdAdapter
    from tidas_tools.import_lca.adapters.openlca_process_xlsx import (
        OpenLcaProcessXlsxAdapter,
    )
    from tidas_tools.import_lca.adapters.simapro_csv import SimaProCsvAdapter
    from tidas_tools.import_lca.detect import (
        FORMAT_UNKNOWN,
        FORMAT_UNSUPPORTED_ZOLCA,
        SUPPORTED_FORMATS,
        detect_format,
    )
    from tidas_tools.import_lca.errors import AmbiguousFormatError
    from tidas_tools.import_lca.report import ConversionReport
    from tidas_tools.import_lca.store import MemoryCanonicalStore
    from tidas_tools.import_lca.writers import (
        write_ilcd_from_tidas,
        write_tidas_package,
    )
    from tidas_tools.tidas_log import setup_logging
    from tidas_tools.validate import validate_ilcd_package_dir, validate_package_dir


TARGETS = {"tidas", "ilcd", "both"}
FROM_FORMATS = {"auto", *SUPPORTED_FORMATS}
DEFAULT_VALIDATION_JOBS = 1


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        description="Import external LCA formats into TIDAS and optionally ILCD/eILCD."
    )
    parser.add_argument(
        "--input",
        required=True,
        help="Source file, directory, or package to import.",
    )
    parser.add_argument(
        "--output-dir",
        required=True,
        help="Output directory for generated package and conversion report.",
    )
    parser.add_argument(
        "--from-format",
        choices=sorted(FROM_FORMATS),
        default="auto",
        help="Source format. Defaults to automatic detection.",
    )
    parser.add_argument(
        "--target",
        choices=sorted(TARGETS),
        default="tidas",
        help="Import target. Defaults to TIDAS.",
    )
    parser.add_argument(
        "--report",
        help="Conversion report path. Defaults to <output-dir>/conversion-report.json.",
    )
    parser.add_argument(
        "--mapping-dir",
        help="Optional custom mapping/reference data directory.",
    )
    parser.add_argument(
        "--language",
        default="en",
        help="Default language for generated multilingual text. Defaults to en.",
    )
    parser.add_argument(
        "--fail-on-warning",
        action="store_true",
        help="Return a non-zero exit code when warnings are present.",
    )
    parser.add_argument(
        "--validation-jobs",
        type=int,
        default=DEFAULT_VALIDATION_JOBS,
        help=(
            "Number of parallel validation worker processes. "
            "Use 0 to use all CPU cores. Defaults to 1."
        ),
    )
    parser.add_argument(
        "--detect-only",
        action="store_true",
        help="Only detect the input format and write the report.",
    )
    parser.add_argument(
        "--verbose",
        "-v",
        action="store_true",
        help="Enable verbose logging.",
    )
    return parser


def main(argv: list[str] | None = None) -> int:
    parser = build_parser()
    args = parser.parse_args(argv)
    setup_logging(args.verbose, "import")

    try:
        return run_import(args)
    except Exception as exc:
        logging.error("Unexpected import error: %s", exc)
        return 1


def run_import(args: argparse.Namespace) -> int:
    output_dir = Path(args.output_dir)
    output_dir.mkdir(parents=True, exist_ok=True)
    report_path = (
        Path(args.report) if args.report else output_dir / "conversion-report.json"
    )

    report = ConversionReport(
        source_path=str(Path(args.input)),
        target=args.target,
        tidas_dir=str(output_dir / "tidas"),
        ilcd_dir=str(output_dir / "ilcd") if args.target in {"ilcd", "both"} else None,
    )

    try:
        detected = detect_format(args.input, requested_format=args.from_format)
    except FileNotFoundError as exc:
        report.add_issue(
            severity="error",
            code="input_not_found",
            message=f"Input path does not exist: {exc}",
        )
        report.write_json(report_path)
        logging.error("Input path does not exist: %s", exc)
        return 2
    except AmbiguousFormatError as exc:
        report.add_issue(
            severity="error",
            code=exc.issue_code,
            message=str(exc),
        )
        report.write_json(report_path)
        logging.error("%s", exc)
        return 2

    report.detected_format = detected.format_id
    report.detection_evidence = detected.evidence

    if detected.format_id == FORMAT_UNSUPPORTED_ZOLCA:
        message = (
            ".zolca is intentionally out of scope for this importer. "
            "Please export JSON-LD, ILCD, EcoSpold, SimaPro CSV, or process XLSX first."
        )
        report.add_issue(
            severity="error",
            code="unsupported_format",
            message=message,
            context={"detected_format": detected.format_id},
        )
        report.write_json(report_path)
        logging.error(message)
        return 2

    if detected.format_id == FORMAT_UNKNOWN:
        message = "Could not detect a supported external LCA source format."
        report.add_issue(
            severity="error",
            code="unsupported_format",
            message=message,
        )
        report.write_json(report_path)
        logging.error(message)
        return 2

    logging.info(
        "Detected source format %s with %s confidence",
        detected.format_id,
        detected.confidence,
    )

    if args.detect_only:
        report.write_json(report_path)
        logging.info("Detection report written to %s", report_path)
        return _status_for_report(report, args.fail_on_warning)

    adapter = _adapter_for(detected.format_id)
    if adapter is None:
        report.add_issue(
            severity="error",
            code="adapter_not_implemented",
            message=f"Adapter for {detected.format_id} is not implemented yet.",
            context={
                "detected_format": detected.format_id,
                "target": args.target,
                "mapping_dir": args.mapping_dir,
                "language": args.language,
            },
        )
        report.write_json(report_path)
        logging.error("Adapter for %s is not implemented yet", detected.format_id)
        return 3

    store = MemoryCanonicalStore()
    adapter.read(args.input, store, report)
    report.object_counts = store.counts()
    if report.error_count:
        report.write_json(report_path)
        logging.error("Import failed with %s error(s)", report.error_count)
        return 2

    tidas_dir = Path(report.tidas_dir)
    write_tidas_package(store, tidas_dir)
    tidas_validation = validate_package_dir(
        str(tidas_dir), emit_logs=False, jobs=args.validation_jobs
    )
    report.validation["tidas"] = tidas_validation
    if not tidas_validation["ok"]:
        report.add_issue(
            severity="error",
            code="schema_validation_error",
            message="Generated TIDAS package failed validation.",
            context={"summary": tidas_validation["summary"]},
        )
        report.write_json(report_path)
        logging.error("Generated TIDAS package failed validation")
        return 2

    if args.target in {"ilcd", "both"}:
        ilcd_dir = Path(report.ilcd_dir)
        write_ilcd_from_tidas(tidas_dir, ilcd_dir)
        ilcd_validation = validate_ilcd_package_dir(
            str(ilcd_dir), emit_logs=False, jobs=args.validation_jobs
        )
        report.validation["ilcd"] = ilcd_validation
        if not ilcd_validation["ok"]:
            report.add_issue(
                severity="error",
                code="ilcd_validation_error",
                message="Generated ILCD/eILCD package failed validation.",
                context={"summary": ilcd_validation["summary"]},
            )
            report.write_json(report_path)
            logging.error("Generated ILCD/eILCD package failed validation")
            return 2

    report.write_json(report_path)
    logging.info("Import report written to %s", report_path)
    return _status_for_report(report, args.fail_on_warning)


def _status_for_report(report: ConversionReport, fail_on_warning: bool) -> int:
    if report.error_count:
        return 2
    if fail_on_warning and report.warning_count:
        return 2
    return 0


def _adapter_for(format_id: str):
    if format_id == "ecospold1":
        return EcoSpold1Adapter()
    if format_id == "ecospold2":
        return EcoSpold2Adapter()
    if format_id == "openlca-jsonld":
        return OpenLcaJsonLdAdapter()
    if format_id == "openlca-process-xlsx":
        return OpenLcaProcessXlsxAdapter()
    if format_id == "simapro-csv":
        return SimaProCsvAdapter()
    return None


if __name__ == "__main__":
    sys.exit(main())
