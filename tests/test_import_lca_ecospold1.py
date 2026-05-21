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


def test_ecospold1_import_uses_filename_uuid_and_local_exchange_ids(tmp_path):
    process_uuid = "64e926e8-dd48-3704-b902-daaf546087c4"
    source = tmp_path / f"process_{process_uuid}.xml"
    source.write_text(
        """<?xml version="1.0" encoding="UTF-8"?>
<ecoSpold xmlns="http://www.EcoInvent.org/EcoSpold01">
  <dataset>
    <metaInformation>
      <processInformation>
        <referenceFunction name="hemp lime concrete" />
      </processInformation>
    </metaInformation>
    <flowData>
      <exchange number="60865" name="Sodium" unit="kg" meanValue="1.0" outputGroup="4" category="emissions to water" subCategory="river" />
      <exchange number="60865" name="Sodium" unit="kg" meanValue="2.0" outputGroup="4" category="emissions to water" subCategory="ocean" />
      <exchange number="1" name="hemp lime concrete" unit="kg" amount="1" outputGroup="0" />
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
    process_path = output_dir / "tidas" / "processes" / f"{process_uuid}.json"
    process = json.loads(process_path.read_text(encoding="utf-8"))["processDataSet"]
    exchanges = process["exchanges"]["exchange"]

    assert status == 0
    assert report["validation"]["tidas"]["ok"] is True
    assert report["validation"]["ilcd"]["ok"] is True
    assert (
        process["processInformation"]["dataSetInformation"]["common:UUID"]
        == process_uuid
    )
    assert process["administrativeInformation"]["publicationAndOwnership"][
        "common:permanentDataSetURI"
    ] == (
        "https://lcdn.tiangong.earth/datasetdetail/process.xhtml?"
        f"uuid={process_uuid}&version=00.00.001"
    )
    assert [exchange["@dataSetInternalID"] for exchange in exchanges] == [
        "1",
        "2",
        "3",
    ]
    assert exchanges[0]["generalComment"]["#text"] == (
        "Source EcoSpold1 exchange number: 60865."
    )

    flow = _find_flow(output_dir / "tidas", "hemp lime concrete")
    flow_id = flow["flowDataSet"]["flowInformation"]["dataSetInformation"][
        "common:UUID"
    ]
    assert flow["flowDataSet"]["administrativeInformation"]["publicationAndOwnership"][
        "common:permanentDataSetURI"
    ] == (
        "https://lcdn.tiangong.earth/datasetdetail/productFlow.xhtml?"
        f"uuid={flow_id}&version=00.00.001"
    )

    unitgroup = next((output_dir / "tidas" / "unitgroups").glob("*.json"))
    unitgroup_id = unitgroup.stem
    unitgroup_data = json.loads(unitgroup.read_text(encoding="utf-8"))[
        "unitGroupDataSet"
    ]
    assert (
        unitgroup_data["administrativeInformation"]["publicationAndOwnership"][
            "common:permanentDataSetURI"
        ]
        == f"https://lcdn.tiangong.earth/unitgroups/{unitgroup_id}?version=00.00.001"
    )


def _has_flow_property(tidas_dir, expected_name):
    for path in (tidas_dir / "flowproperties").glob("*.json"):
        data = json.loads(path.read_text(encoding="utf-8"))
        name = data["flowPropertyDataSet"]["flowPropertiesInformation"][
            "dataSetInformation"
        ]["common:name"]["#text"]
        if name == expected_name:
            return True
    return False


def _find_flow(tidas_dir, expected_name):
    for path in (tidas_dir / "flows").glob("*.json"):
        data = json.loads(path.read_text(encoding="utf-8"))
        name = data["flowDataSet"]["flowInformation"]["dataSetInformation"]["name"][
            "baseName"
        ]["#text"]
        if name == expected_name:
            return data
    raise AssertionError(f"Flow not found: {expected_name}")
