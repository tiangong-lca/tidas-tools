import json

from tidas_tools.import_lca.cli import main


def test_ecospold1_minimal_import_can_write_valid_tidas_and_ilcd(tmp_path):
    source = tmp_path / "dataset.xml"
    source.write_text(
        """<?xml version="1.0" encoding="UTF-8"?>
<ecoSpold xmlns="http://www.EcoInvent.org/EcoSpold01">
  <dataset>
    <metaInformation>
      <processInformation>
        <referenceFunction name="market for steel product" />
      </processInformation>
    </metaInformation>
    <flowData>
      <exchange number="1" name="steel product" unit="kg" amount="1" outputGroup="0" />
      <exchange number="2" name="carbon dioxide" unit="kg" amount="2.5" outputGroup="4" category="air" subCategory="unspecified" />
      <exchange number="3" name="iron ore" unit="kg" amount="1.4" inputGroup="4" category="resource" subCategory="in ground" />
    </flowData>
  </dataset>
</ecoSpold>
""",
        encoding="utf-8",
    )

    output_dir = tmp_path / "out"
    status = main(
        [
            "--input",
            str(source),
            "--output-dir",
            str(output_dir),
            "--target",
            "both",
        ]
    )

    report = json.loads(
        (output_dir / "conversion-report.json").read_text(encoding="utf-8")
    )

    assert status == 0
    assert report["source"]["detected_format"] == "ecospold1"
    assert report["summary"]["flows"] == 3
    assert report["summary"]["processes"] == 1
    assert report["validation"]["tidas"]["ok"] is True
    assert report["validation"]["ilcd"]["ok"] is True
    assert _has_flow_property(output_dir / "tidas", "Amount in kg")


def _has_flow_property(tidas_dir, expected_name):
    for path in (tidas_dir / "flowproperties").glob("*.json"):
        data = json.loads(path.read_text(encoding="utf-8"))
        name = data["flowPropertyDataSet"]["flowPropertiesInformation"][
            "dataSetInformation"
        ]["common:name"]["#text"]
        if name == expected_name:
            return True
    return False
