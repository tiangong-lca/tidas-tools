import json

from tidas_tools.import_lca.cli import main


def test_ecospold2_minimal_import_can_write_valid_tidas_and_ilcd(tmp_path):
    source = tmp_path / "activity.spold"
    source.write_text(
        """<?xml version="1.0" encoding="UTF-8"?>
<ecoSpold xmlns="http://www.EcoInvent.org/EcoSpold02">
  <activityDataset>
    <activityDescription>
      <activity id="22222222-2222-4222-8222-222222222222">
        <activityName xml:lang="en">market for test product</activityName>
        <generalComment>
          <text xml:lang="en" index="1">Minimal EcoSpold 2 fixture</text>
        </generalComment>
      </activity>
    </activityDescription>
    <flowData>
      <intermediateExchange id="11111111-1111-4111-8111-111111111111" amount="1">
        <name xml:lang="en">test product</name>
        <unitName xml:lang="en">kg</unitName>
        <outputGroup>0</outputGroup>
      </intermediateExchange>
      <intermediateExchange id="33333333-3333-4333-8333-333333333333" amount="0.2">
        <name xml:lang="en">test input</name>
        <unitName xml:lang="en">kg</unitName>
        <inputGroup>5</inputGroup>
      </intermediateExchange>
      <elementaryExchange id="44444444-4444-4444-8444-444444444444" amount="1.5">
        <name xml:lang="en">carbon dioxide</name>
        <unitName xml:lang="en">kg</unitName>
        <compartment>
          <compartment xml:lang="en">air</compartment>
          <subcompartment xml:lang="en">low population density</subcompartment>
        </compartment>
        <outputGroup>4</outputGroup>
      </elementaryExchange>
    </flowData>
  </activityDataset>
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
    assert report["source"]["detected_format"] == "ecospold2"
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
