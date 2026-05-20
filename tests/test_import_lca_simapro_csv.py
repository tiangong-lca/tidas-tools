import json

from tidas_tools.import_lca.cli import main


def test_simapro_csv_minimal_import_can_write_valid_tidas_and_ilcd(tmp_path):
    source = tmp_path / "process.csv"
    source.write_text(
        """{SimaPro 8.0}
{processes}
{CSV separator: Semicolon}
{Decimal separator: ,}

Process

Process name
Test process

Products
my product;kg;0,5;100;not defined;Agricultural;

Materials/fuels
Soy oil, refined, at plant/kg/RNA;kg;0,2;Undefined;0;0;0;

Emissions to air
carbon dioxide;low. pop.;kg;1,5;Undefined;0;0;0;

End
""",
        encoding="cp1252",
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
    assert report["source"]["detected_format"] == "simapro-csv"
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
