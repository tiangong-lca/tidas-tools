import json
import zipfile

import pytest

from tidas_tools.import_lca.detect import (
    FORMAT_ECOSPOLD1,
    FORMAT_ECOSPOLD2,
    FORMAT_OPENLCA_JSONLD,
    FORMAT_OPENLCA_PROCESS_XLSX,
    FORMAT_SIMAPRO_CSV,
    FORMAT_UNSUPPORTED_ZOLCA,
    detect_format,
)
from tidas_tools.import_lca.errors import AmbiguousFormatError


def test_detect_zolca_even_with_explicit_override(tmp_path):
    source = tmp_path / "database.zolca"
    source.write_bytes(b"not inspected")

    detected = detect_format(source, requested_format=FORMAT_OPENLCA_JSONLD)

    assert detected.format_id == FORMAT_UNSUPPORTED_ZOLCA
    assert detected.confidence == "high"


def test_detect_simapro_csv(tmp_path):
    source = tmp_path / "simapro.csv"
    source.write_text("{SimaPro 9.5}\nprocess data", encoding="utf-8")

    detected = detect_format(source)

    assert detected.format_id == FORMAT_SIMAPRO_CSV
    assert detected.confidence == "high"
    assert "SimaPro" in detected.evidence[0]


def test_detect_ecospold1_xml(tmp_path):
    source = tmp_path / "dataset.xml"
    source.write_text(
        '<ecoSpold xmlns="http://www.EcoInvent.org/EcoSpold01" />',
        encoding="utf-8",
    )

    detected = detect_format(source)

    assert detected.format_id == FORMAT_ECOSPOLD1
    assert detected.confidence == "high"


def test_detect_ecospold2_spold_extension(tmp_path):
    source = tmp_path / "dataset.spold"
    source.write_text("<anything />", encoding="utf-8")

    detected = detect_format(source)

    assert detected.format_id == FORMAT_ECOSPOLD2
    assert detected.confidence == "high"


def test_detect_openlca_jsonld_file(tmp_path):
    source = tmp_path / "process.json"
    source.write_text(
        json.dumps({"@type": "Process", "@id": "abc", "name": "Example"}),
        encoding="utf-8",
    )

    detected = detect_format(source)

    assert detected.format_id == FORMAT_OPENLCA_JSONLD
    assert detected.confidence == "high"


def test_detect_openlca_jsonld_zip_with_context(tmp_path):
    source = tmp_path / "olca.zip"
    with zipfile.ZipFile(source, "w") as archive:
        archive.writestr("context.jsonld", "{}")

    detected = detect_format(source)

    assert detected.format_id == FORMAT_OPENLCA_JSONLD
    assert detected.confidence == "high"


def test_detect_xlsx_container_as_openlca_process_workbook_candidate(tmp_path):
    source = tmp_path / "process.xlsx"
    with zipfile.ZipFile(source, "w") as archive:
        archive.writestr("[Content_Types].xml", "<Types />")
        archive.writestr("xl/workbook.xml", "<workbook />")

    detected = detect_format(source)

    assert detected.format_id == FORMAT_OPENLCA_PROCESS_XLSX
    assert detected.confidence == "medium"


def test_detect_zip_ambiguous_high_confidence_candidates(tmp_path):
    source = tmp_path / "mixed.zip"
    with zipfile.ZipFile(source, "w") as archive:
        archive.writestr(
            "es1.xml",
            '<ecoSpold xmlns="http://www.EcoInvent.org/EcoSpold01" />',
        )
        archive.writestr("data.csv", "{SimaPro 9.5}\nprocess data")

    with pytest.raises(AmbiguousFormatError):
        detect_format(source)


def test_detect_zip_prefers_dominant_ecospold_package_over_simapro_import_guide(
    tmp_path,
):
    source = tmp_path / "bafu-like.zip"
    with zipfile.ZipFile(source, "w") as archive:
        archive.writestr(
            "Import Guide for SimaPro/SetupDatabase_file.CSV", "{SimaPro 9.5}\n"
        )
        for index in range(3):
            archive.writestr(
                f"LCI ecoSpold version2 Files/process_{index}.xml",
                "<ecoSpold><dataset /></ecoSpold>",
            )

    detected = detect_format(source)

    assert detected.format_id == FORMAT_ECOSPOLD1
    assert detected.confidence == "medium"
    assert any("SimaPro" in item for item in detected.evidence)


def test_detect_zip_rejects_embedded_zolca_before_other_candidates(tmp_path):
    source = tmp_path / "mixed.zip"
    with zipfile.ZipFile(source, "w") as archive:
        archive.writestr("backup.zolca", "database backup")
        archive.writestr("context.jsonld", "{}")

    detected = detect_format(source)

    assert detected.format_id == FORMAT_UNSUPPORTED_ZOLCA
    assert detected.confidence == "high"
