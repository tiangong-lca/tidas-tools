import json

from tidas_tools.import_lca.cli import main
from tidas_tools.validate import validate_package_dir

FLOW_ID = "11111111-1111-4111-8111-111111111111"
PROCESS_ID = "22222222-2222-4222-8222-222222222222"
UNIT_GROUP_ID = "33333333-3333-4333-8333-333333333333"
FLOW_PROPERTY_ID = "44444444-4444-4444-8444-444444444444"
ACTOR_ID = "55555555-5555-4555-8555-555555555555"
SOURCE_ID = "66666666-6666-4666-8666-666666666666"


def test_openlca_jsonld_minimal_import_writes_valid_tidas_package(tmp_path):
    source_dir = write_minimal_jsonld_fixture(tmp_path)
    output_dir = tmp_path / "out"

    status = main(
        [
            "--input",
            str(source_dir),
            "--output-dir",
            str(output_dir),
            "--from-format",
            "openlca-jsonld",
        ]
    )

    assert status == 0
    assert (output_dir / "tidas" / "contacts" / f"{ACTOR_ID}.json").is_file()
    assert (output_dir / "tidas" / "sources" / f"{SOURCE_ID}.json").is_file()
    assert (output_dir / "tidas" / "flows" / f"{FLOW_ID}.json").is_file()
    assert (output_dir / "tidas" / "processes" / f"{PROCESS_ID}.json").is_file()

    validation = validate_package_dir(str(output_dir / "tidas"))
    assert validation["ok"] is True

    report = json.loads(
        (output_dir / "conversion-report.json").read_text(encoding="utf-8")
    )
    assert report["source"]["detected_format"] == "openlca-jsonld"
    assert report["summary"]["contacts"] == 1
    assert report["summary"]["flows"] == 1
    assert report["summary"]["processes"] == 1
    assert report["summary"]["sources"] == 1
    assert report["validation"]["tidas"]["ok"] is True


def test_openlca_jsonld_minimal_import_can_write_valid_ilcd(tmp_path):
    source_dir = write_minimal_jsonld_fixture(tmp_path)
    output_dir = tmp_path / "out"

    status = main(
        [
            "--input",
            str(source_dir),
            "--output-dir",
            str(output_dir),
            "--from-format",
            "openlca-jsonld",
            "--target",
            "both",
        ]
    )

    report = json.loads(
        (output_dir / "conversion-report.json").read_text(encoding="utf-8")
    )

    assert status == 0
    assert report["validation"]["tidas"]["ok"] is True
    assert report["validation"]["ilcd"]["ok"] is True
    assert (output_dir / "ilcd" / "data" / "flows" / f"{FLOW_ID}.xml").is_file()


def write_minimal_jsonld_fixture(tmp_path):
    source_dir = tmp_path / "jsonld"
    source_dir.mkdir()
    (source_dir / "context.jsonld").write_text("{}", encoding="utf-8")
    (source_dir / "unit_group.json").write_text(
        json.dumps(
            {
                "@type": "UnitGroup",
                "@id": UNIT_GROUP_ID,
                "name": "Units of mass",
                "units": [{"name": "kg", "conversionFactor": 1.0}],
            }
        ),
        encoding="utf-8",
    )
    (source_dir / "flow_property.json").write_text(
        json.dumps(
            {
                "@type": "FlowProperty",
                "@id": FLOW_PROPERTY_ID,
                "name": "Mass",
                "unitGroup": {"@id": UNIT_GROUP_ID, "name": "Units of mass"},
            }
        ),
        encoding="utf-8",
    )
    (source_dir / "actor.json").write_text(
        json.dumps(
            {
                "@type": "Actor",
                "@id": ACTOR_ID,
                "name": "Example data author",
                "email": "author@example.org",
                "description": "Fixture actor",
            }
        ),
        encoding="utf-8",
    )
    (source_dir / "source.json").write_text(
        json.dumps(
            {
                "@type": "Source",
                "@id": SOURCE_ID,
                "name": "Example source",
                "textReference": "Example citation",
                "description": "Fixture source",
            }
        ),
        encoding="utf-8",
    )
    (source_dir / "flow.json").write_text(
        json.dumps(
            {
                "@type": "Flow",
                "@id": FLOW_ID,
                "name": "Steel product",
                "flowType": "PRODUCT_FLOW",
                "flowProperties": [
                    {
                        "flowProperty": {"@id": FLOW_PROPERTY_ID, "name": "Mass"},
                        "isRefFlowProperty": True,
                    }
                ],
            }
        ),
        encoding="utf-8",
    )
    (source_dir / "process.json").write_text(
        json.dumps(
            {
                "@type": "Process",
                "@id": PROCESS_ID,
                "name": "Steel production",
                "description": "Minimal process fixture",
                "exchanges": [
                    {
                        "internalId": 1,
                        "flow": {"@id": FLOW_ID, "name": "Steel product"},
                        "amount": 1.0,
                        "isInput": False,
                        "isQuantitativeReference": True,
                    }
                ],
            }
        ),
        encoding="utf-8",
    )
    return source_dir
