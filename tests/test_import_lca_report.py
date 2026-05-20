import json

from tidas_tools.import_lca.report import ConversionReport


def test_conversion_report_writes_machine_readable_summary(tmp_path):
    report = ConversionReport(
        source_path="source.csv",
        detected_format="simapro-csv",
        target="tidas",
        tidas_dir=str(tmp_path / "tidas"),
        detection_evidence=["first line starts with {SimaPro"],
        object_counts={"processes": 2},
    )
    report.add_issue(
        severity="warning",
        code="low_confidence_mapping",
        message="Mapped flow by normalized name.",
        source_object="flow:abc",
        context={"confidence": "medium"},
    )

    output_path = tmp_path / "conversion-report.json"
    report.write_json(output_path)

    payload = json.loads(output_path.read_text(encoding="utf-8"))

    assert payload["tool"] == "tidas-tools"
    assert payload["source"]["detected_format"] == "simapro-csv"
    assert payload["summary"]["processes"] == 2
    assert payload["summary"]["warnings"] == 1
    assert payload["summary"]["errors"] == 0
    assert payload["issues"][0]["code"] == "low_confidence_mapping"
