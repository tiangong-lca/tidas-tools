"""Tests for the native ILCD source adapter."""

import json

from tidas_tools.import_lca.adapters.ilcd import IlcdAdapter
from tidas_tools.import_lca.cli import main
from tidas_tools.import_lca.detect import FORMAT_ILCD, detect_format
from tidas_tools.import_lca.report import ConversionReport
from tidas_tools.import_lca.store import MemoryCanonicalStore

CONTACT_ID = "d5710976-d600-11da-a94d-0800200c9a66"
SOURCE_ID = "11111111-1111-1111-1111-111111111111"
UNITGROUP_ID = "22222222-2222-2222-2222-222222222222"
FLOWPROPERTY_ID = "33333333-3333-3333-3333-333333333333"
FLOW_ID = "44444444-4444-4444-4444-444444444444"
PROCESS_ID = "55555555-5555-5555-5555-555555555555"
LCIA_ID = "66666666-6666-6666-6666-666666666666"


def _write(path, text):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text, encoding="utf-8")


def build_ilcd_package(tmp_path):
    root = tmp_path / "pkg" / "ILCD"

    _write(
        root / "contacts" / f"{CONTACT_ID}.xml",
        f"""<?xml version="1.0" encoding="UTF-8"?>
<contactDataSet xmlns:common="http://lca.jrc.it/ILCD/Common" xmlns="http://lca.jrc.it/ILCD/Contact" version="1.1">
  <contactInformation><dataSetInformation>
    <common:UUID>{CONTACT_ID}</common:UUID>
    <common:shortName xml:lang="en">worldsteel</common:shortName>
    <common:name xml:lang="en">World Steel Association</common:name>
    <WWWAddress>www.worldsteel.org</WWWAddress>
  </dataSetInformation></contactInformation>
  <administrativeInformation><publicationAndOwnership>
    <common:dataSetVersion>20.20.002</common:dataSetVersion>
  </publicationAndOwnership></administrativeInformation>
</contactDataSet>""",
    )

    _write(
        root / "sources" / f"{SOURCE_ID}.xml",
        f"""<?xml version="1.0" encoding="UTF-8"?>
<sourceDataSet xmlns:common="http://lca.jrc.it/ILCD/Common" xmlns="http://lca.jrc.it/ILCD/Source" version="1.1">
  <sourceInformation><dataSetInformation>
    <common:UUID>{SOURCE_ID}</common:UUID>
    <common:shortName xml:lang="en">worldsteel route diagram</common:shortName>
    <sourceCitation>worldsteel, 2022.</sourceCitation>
    <publicationType>Undefined</publicationType>
    <referenceToDigitalFile uri="../external_docs/bof+route.png"/>
    <referenceToDigitalFile uri="http://lca.jrc.ec.europa.eu"/>
  </dataSetInformation></sourceInformation>
  <administrativeInformation><publicationAndOwnership>
    <common:dataSetVersion>20.20.002</common:dataSetVersion>
  </publicationAndOwnership></administrativeInformation>
</sourceDataSet>""",
    )

    _write(
        root / "unitgroups" / f"{UNITGROUP_ID}.xml",
        f"""<?xml version="1.0" encoding="UTF-8"?>
<unitGroupDataSet xmlns:common="http://lca.jrc.it/ILCD/Common" xmlns="http://lca.jrc.it/ILCD/UnitGroup" version="1.1">
  <unitGroupInformation>
    <dataSetInformation>
      <common:UUID>{UNITGROUP_ID}</common:UUID>
      <common:name xml:lang="en">Units of mass</common:name>
    </dataSetInformation>
    <quantitativeReference><referenceToReferenceUnit>0</referenceToReferenceUnit></quantitativeReference>
  </unitGroupInformation>
  <units>
    <unit dataSetInternalID="0"><name>kg</name><meanValue>1.0</meanValue></unit>
    <unit dataSetInternalID="1"><name>g</name><meanValue>0.001</meanValue></unit>
  </units>
  <administrativeInformation><publicationAndOwnership>
    <common:dataSetVersion>20.20.002</common:dataSetVersion>
  </publicationAndOwnership></administrativeInformation>
</unitGroupDataSet>""",
    )

    _write(
        root / "flowproperties" / f"{FLOWPROPERTY_ID}.xml",
        f"""<?xml version="1.0" encoding="UTF-8"?>
<flowPropertyDataSet xmlns:common="http://lca.jrc.it/ILCD/Common" xmlns="http://lca.jrc.it/ILCD/FlowProperty" version="1.1">
  <flowPropertiesInformation>
    <dataSetInformation>
      <common:UUID>{FLOWPROPERTY_ID}</common:UUID>
      <common:name xml:lang="en">Mass</common:name>
    </dataSetInformation>
    <quantitativeReference>
      <referenceToReferenceUnitGroup type="unit group data set" refObjectId="{UNITGROUP_ID}" uri="../unitgroups/{UNITGROUP_ID}.xml">
        <common:shortDescription xml:lang="en">Units of mass</common:shortDescription>
      </referenceToReferenceUnitGroup>
    </quantitativeReference>
  </flowPropertiesInformation>
  <administrativeInformation><publicationAndOwnership>
    <common:dataSetVersion>20.20.002</common:dataSetVersion>
  </publicationAndOwnership></administrativeInformation>
</flowPropertyDataSet>""",
    )

    _write(
        root / "flows" / f"{FLOW_ID}.xml",
        f"""<?xml version="1.0" encoding="UTF-8"?>
<flowDataSet xmlns:common="http://lca.jrc.it/ILCD/Common" xmlns="http://lca.jrc.it/ILCD/Flow" version="1.1">
  <flowInformation>
    <dataSetInformation>
      <common:UUID>{FLOW_ID}</common:UUID>
      <name><baseName xml:lang="en">Steel rebar</baseName></name>
      <classificationInformation><common:classification>
        <common:class level="0">Materials production</common:class>
        <common:class level="1">Metals and semimetals</common:class>
      </common:classification></classificationInformation>
    </dataSetInformation>
    <quantitativeReference><referenceToReferenceFlowProperty>0</referenceToReferenceFlowProperty></quantitativeReference>
  </flowInformation>
  <modellingAndValidation><LCIMethod><typeOfDataSet>Product flow</typeOfDataSet></LCIMethod></modellingAndValidation>
  <flowProperties>
    <flowProperty dataSetInternalID="0">
      <referenceToFlowPropertyDataSet type="flow property data set" refObjectId="{FLOWPROPERTY_ID}" uri="../flowproperties/{FLOWPROPERTY_ID}.xml">
        <common:shortDescription xml:lang="en">Mass</common:shortDescription>
      </referenceToFlowPropertyDataSet>
      <meanValue>1.0</meanValue>
    </flowProperty>
  </flowProperties>
  <administrativeInformation><publicationAndOwnership>
    <common:dataSetVersion>20.25.001</common:dataSetVersion>
  </publicationAndOwnership></administrativeInformation>
</flowDataSet>""",
    )

    _write(
        root / "processes" / f"{PROCESS_ID}.xml",
        f"""<?xml version="1.0" encoding="UTF-8"?>
<processDataSet xmlns:common="http://lca.jrc.it/ILCD/Common" xmlns="http://lca.jrc.it/ILCD/Process" version="1.1">
  <processInformation>
    <dataSetInformation>
      <common:UUID>{PROCESS_ID}</common:UUID>
      <name><baseName xml:lang="en">Steel rebar Global 2022</baseName></name>
      <common:generalComment xml:lang="en">Aggregated LCI result.</common:generalComment>
    </dataSetInformation>
    <quantitativeReference type="Reference flow(s)">
      <referenceToReferenceFlow>1</referenceToReferenceFlow>
    </quantitativeReference>
    <geography><locationOfOperationSupplyOrProduction location="GLO"/></geography>
    <time><common:referenceYear>2022</common:referenceYear></time>
  </processInformation>
  <exchanges>
    <exchange dataSetInternalID="1">
      <referenceToFlowDataSet type="flow data set" refObjectId="{FLOW_ID}" uri="../flows/{FLOW_ID}.xml">
        <common:shortDescription xml:lang="en">Steel rebar</common:shortDescription>
      </referenceToFlowDataSet>
      <exchangeDirection>Output</exchangeDirection>
      <meanAmount>1.0</meanAmount>
      <resultingAmount>1.0</resultingAmount>
    </exchange>
  </exchanges>
  <administrativeInformation><publicationAndOwnership>
    <common:dataSetVersion>20.25.001</common:dataSetVersion>
  </publicationAndOwnership></administrativeInformation>
</processDataSet>""",
    )

    # An LCIA method dataset that MUST be skipped (out of import scope).
    _write(
        root / "lciamethods" / f"{LCIA_ID}.xml",
        f"""<?xml version="1.0" encoding="UTF-8"?>
<LCIAMethodDataSet xmlns:common="http://lca.jrc.it/ILCD/Common" xmlns="http://lca.jrc.it/ILCD/LCIAMethod" version="1.1">
  <LCIAMethodInformation><dataSetInformation>
    <common:UUID>{LCIA_ID}</common:UUID>
  </dataSetInformation></LCIAMethodInformation>
</LCIAMethodDataSet>""",
    )

    return tmp_path / "pkg"


def test_detect_format_recognizes_ilcd_directory(tmp_path):
    package = build_ilcd_package(tmp_path)
    detected = detect_format(str(package))
    assert detected.format_id == FORMAT_ILCD
    assert detected.confidence == "high"


def test_ilcd_adapter_parses_entities_preserving_uuids_and_digital_files(tmp_path):
    package = build_ilcd_package(tmp_path)
    store = MemoryCanonicalStore()
    report = ConversionReport(source_path=str(package))
    IlcdAdapter().read(str(package), store, report)

    assert report.error_count == 0
    counts = store.counts()
    assert counts == {
        "contacts": 1,
        "sources": 1,
        "unitgroups": 1,
        "flowproperties": 1,
        "flows": 1,
        "processes": 1,
    }
    # LCIA methods are out of scope and must not be imported.
    assert "lciamethods" not in counts

    flow = store.get("flows", FLOW_ID)
    assert flow.name == "Steel rebar"
    assert flow.raw["flowType"] == "Product flow"
    assert flow.raw["flowPropertyRefId"] == FLOWPROPERTY_ID
    assert flow.raw["version"] == "20.25.001"  # source version preserved
    assert flow.category_path == ["Materials production", "Metals and semimetals"]

    flow_property = store.get("flowproperties", FLOWPROPERTY_ID)
    assert flow_property.raw["unitGroupRefId"] == UNITGROUP_ID

    unit_group = store.get("unitgroups", UNITGROUP_ID)
    reference_units = [u for u in unit_group.raw["units"] if u.get("referenceUnit")]
    assert len(reference_units) == 1 and reference_units[0]["name"] == "kg"

    process = store.get("processes", PROCESS_ID)
    exchanges = process.raw["exchanges"]
    assert len(exchanges) == 1
    assert exchanges[0]["flowRefId"] == FLOW_ID
    assert exchanges[0]["isInput"] is False
    assert exchanges[0]["isQuantitativeReference"] is True
    assert process.raw["location"] == "GLO"
    assert process.raw["referenceYear"] == "2022"

    source = store.get("sources", SOURCE_ID)
    # Only the local ../external_docs ref is preserved; the http URL is not.
    assert source.raw["referenceToDigitalFile"] == ["../external_docs/bof+route.png"]
    assert source.raw["url"] == "http://lca.jrc.ec.europa.eu"


def test_ilcd_import_writes_valid_tidas_package_with_digital_file(tmp_path):
    package = build_ilcd_package(tmp_path)
    output_dir = tmp_path / "out"
    status = main(
        [
            "--input",
            str(package),
            "--output-dir",
            str(output_dir),
            "--from-format",
            "ilcd",
            "--target",
            "tidas",
            "--validation-jobs",
            "0",
        ]
    )
    assert status == 0

    report = json.loads(
        (output_dir / "conversion-report.json").read_text(encoding="utf-8")
    )
    assert report["source"]["detected_format"] == "ilcd"
    assert report["validation"]["tidas"]["ok"] is True

    # Original UUID preserved as the TIDAS file name.
    process_file = output_dir / "tidas" / "processes" / f"{PROCESS_ID}.json"
    assert process_file.is_file()

    # The local digital-file ref survives into the TIDAS source (single -> object).
    source_payload = json.loads(
        (output_dir / "tidas" / "sources" / f"{SOURCE_ID}.json").read_text(
            encoding="utf-8"
        )
    )
    dataset_info = source_payload["sourceDataSet"]["sourceInformation"][
        "dataSetInformation"
    ]
    assert dataset_info["referenceToDigitalFile"] == {
        "@uri": "../external_docs/bof+route.png"
    }
    # The external URL is recorded as a description note, not a digital file.
    assert "lca.jrc.ec.europa.eu" in json.dumps(
        dataset_info.get("sourceDescriptionOrComment", {})
    )


def test_ilcd_import_preserves_source_dataset_versions(tmp_path):
    # R4: the converted TIDAS entities must keep their source dataSetVersion
    # (e.g. 20.25.001 products, 20.20.002 support), not be renumbered to 00.00.001.
    package = build_ilcd_package(tmp_path)
    output_dir = tmp_path / "out"
    status = main(
        [
            "--input",
            str(package),
            "--output-dir",
            str(output_dir),
            "--from-format",
            "ilcd",
            "--target",
            "tidas",
            "--validation-jobs",
            "0",
        ]
    )
    assert status == 0

    def version(category, dataset_id, root):
        payload = json.loads(
            (output_dir / "tidas" / category / f"{dataset_id}.json").read_text("utf-8")
        )
        return payload[root]["administrativeInformation"]["publicationAndOwnership"][
            "common:dataSetVersion"
        ]

    assert version("flows", FLOW_ID, "flowDataSet") == "20.25.001"
    assert version("contacts", CONTACT_ID, "contactDataSet") == "20.20.002"
    assert version("sources", SOURCE_ID, "sourceDataSet") == "20.20.002"
    assert version("unitgroups", UNITGROUP_ID, "unitGroupDataSet") == "20.20.002"
    assert (
        version("flowproperties", FLOWPROPERTY_ID, "flowPropertyDataSet") == "20.20.002"
    )
    assert version("processes", PROCESS_ID, "processDataSet") == "20.25.001"


ELEM_FLOW_ID = "218d3b51-1339-4389-b4f9-f4fbe8deea46"

ELEM_FLOW_XML = f"""<?xml version="1.0" encoding="UTF-8"?>
<flowDataSet xmlns:common="http://lca.jrc.it/ILCD/Common" xmlns="http://lca.jrc.it/ILCD/Flow" version="1.1">
  <flowInformation>
    <dataSetInformation>
      <common:UUID>{ELEM_FLOW_ID}</common:UUID>
      <name><baseName xml:lang="en">Lead-210</baseName></name>
      <classificationInformation>
        <common:elementaryFlowCategorization>
          <common:category level="0">Emissions</common:category>
          <common:category level="1">Emissions to water</common:category>
          <common:category level="2">Emissions to fresh water</common:category>
        </common:elementaryFlowCategorization>
      </classificationInformation>
      <CASNumber>014255-04-0</CASNumber>
    </dataSetInformation>
    <quantitativeReference><referenceToReferenceFlowProperty>0</referenceToReferenceFlowProperty></quantitativeReference>
  </flowInformation>
  <modellingAndValidation><LCIMethod><typeOfDataSet>Elementary flow</typeOfDataSet></LCIMethod></modellingAndValidation>
  <administrativeInformation><publicationAndOwnership>
    <common:dataSetVersion>20.25.001</common:dataSetVersion>
  </publicationAndOwnership></administrativeInformation>
</flowDataSet>"""


def test_ilcd_elementary_compartment_passthrough_to_tidas_category(tmp_path):
    # An ILCD elementary flow carries its compartment path as level+label (no
    # catId). The adapter captures it and the writer maps it onto the canonical
    # TIDAS category tree -- NOT the air-unspecified placeholder.
    from tidas_tools.import_lca.adapters.ilcd import _parse_flow, _parse_xml
    from tidas_tools.import_lca.writers.tidas_json import _flow_classification

    root = _parse_xml(ELEM_FLOW_XML.encode("utf-8"))
    name, raw = _parse_flow(root, "flows/elem.xml")
    assert name == "Lead-210"
    assert raw["elementaryCategorization"] == [
        {"level": "0", "text": "Emissions"},
        {"level": "1", "text": "Emissions to water"},
        {"level": "2", "text": "Emissions to fresh water"},
    ]

    block = _flow_classification("Elementary flow", raw)
    cats = block["common:elementaryFlowCategorization"]["common:category"]
    assert cats == [
        {"@level": "0", "@catId": "1", "#text": "Emissions"},
        {"@level": "1", "@catId": "1.1", "#text": "Emissions to water"},
        {"@level": "2", "@catId": "1.1.1", "#text": "Emissions to fresh water"},
    ]
    other = block["common:elementaryFlowCategorization"].get("common:other")
    if other is not None:
        assert "tidasimport:conversionGap" not in other
