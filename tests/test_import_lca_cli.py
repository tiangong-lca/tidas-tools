import json

import pytest

from tidas_tools.import_lca import cli
from tidas_tools.import_lca.cli import main


def test_cli_detect_only_writes_report(tmp_path):
    source = tmp_path / "simapro.csv"
    source.write_text("{SimaPro 9.5}\nprocess data", encoding="utf-8")
    output_dir = tmp_path / "out"

    status = main(
        [
            "--input",
            str(source),
            "--output-dir",
            str(output_dir),
            "--detect-only",
        ]
    )

    report = json.loads(
        (output_dir / "conversion-report.json").read_text(encoding="utf-8")
    )

    assert status == 0
    assert report["source"]["detected_format"] == "simapro-csv"
    assert report["summary"]["errors"] == 0


def test_cli_process_bundles_are_enabled_by_default():
    args = cli.build_parser().parse_args(
        [
            "--input",
            "source.jsonld",
            "--output-dir",
            "out",
        ]
    )

    assert args.process_bundles is True


def test_cli_can_disable_process_bundles():
    args = cli.build_parser().parse_args(
        [
            "--input",
            "source.jsonld",
            "--output-dir",
            "out",
            "--no-process-bundles",
        ]
    )

    assert args.process_bundles is False


def test_cli_mapping_csv_is_disabled_by_default():
    args = cli.build_parser().parse_args(
        [
            "--input",
            "source.jsonld",
            "--output-dir",
            "out",
        ]
    )

    assert args.write_mapping_csv is False


def test_cli_can_enable_mapping_csv():
    args = cli.build_parser().parse_args(
        [
            "--input",
            "source.jsonld",
            "--output-dir",
            "out",
            "--write-mapping-csv",
        ]
    )

    assert args.write_mapping_csv is True


def test_cli_rejects_legacy_process_bundles_flag():
    with pytest.raises(SystemExit) as exc:
        cli.build_parser().parse_args(
            [
                "--input",
                "source.jsonld",
                "--output-dir",
                "out",
                "--process-bundles",
            ]
        )

    assert exc.value.code == 2


def test_cli_rejects_zolca_with_report(tmp_path):
    source = tmp_path / "database.zolca"
    source.write_bytes(b"database backup")
    output_dir = tmp_path / "out"

    status = main(
        [
            "--input",
            str(source),
            "--output-dir",
            str(output_dir),
        ]
    )

    report = json.loads(
        (output_dir / "conversion-report.json").read_text(encoding="utf-8")
    )

    assert status == 2
    assert report["source"]["detected_format"] == "unsupported-zolca"
    assert report["issues"][0]["code"] == "unsupported_format"


def test_cli_reports_missing_adapter_when_dispatch_is_not_registered(
    tmp_path, monkeypatch
):
    source = tmp_path / "simapro.csv"
    source.write_text("{SimaPro 9.5}\nprocess data", encoding="utf-8")
    output_dir = tmp_path / "out"
    monkeypatch.setattr(cli, "_adapter_for", lambda _format_id: None)

    status = main(
        [
            "--input",
            str(source),
            "--output-dir",
            str(output_dir),
        ]
    )

    report = json.loads(
        (output_dir / "conversion-report.json").read_text(encoding="utf-8")
    )

    assert status == 3
    assert report["source"]["detected_format"] == "simapro-csv"
    assert report["issues"][0]["code"] == "adapter_not_implemented"
