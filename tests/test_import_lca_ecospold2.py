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
      <timePeriod>
        <startDate>2024-01-01</startDate>
      </timePeriod>
      <geography>
        <shortname xml:lang="en">CH</shortname>
      </geography>
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
        <CASNumber>124-38-9</CASNumber>
        <sumFormula>CO2</sumFormula>
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
    process = _read_process(
        output_dir / "tidas", "22222222-2222-4222-8222-222222222222"
    )
    process_info = process["processInformation"]
    assert process_info["time"]["common:referenceYear"] == 2024
    assert (
        process_info["geography"]["locationOfOperationSupplyOrProduction"]["@location"]
        == "CH"
    )
    assert process["exchanges"]["exchange"][0]["generalComment"]["#text"] == (
        "Source EcoSpold2 exchange id: 11111111-1111-4111-8111-111111111111."
    )
    co2 = _find_flow(output_dir / "tidas", "carbon dioxide")["flowDataSet"][
        "flowInformation"
    ]["dataSetInformation"]
    assert co2["CASNumber"] == "124-38-9"
    assert co2["sumFormula"] == "CO2"


def test_ecospold2_import_maps_semantic_fields_and_source_trace(tmp_path):
    process_uuid = "22222222-2222-4222-8222-222222222222"
    product_id = "11111111-1111-4111-8111-111111111111"
    source = tmp_path / f"{process_uuid}_{product_id}.spold"
    source.write_text(
        f"""<?xml version="1.0" encoding="UTF-8"?>
<ecoSpold xmlns="http://www.EcoInvent.org/EcoSpold02">
  <fileAttributes majorRelease="3" minorRelease="12" internalSchemaVersion="2.0" defaultLanguage="en"/>
  <activityDataset>
    <activityDescription>
      <activity id="{process_uuid}" specialActivityType="0">
        <activityName xml:lang="en">market for enriched product</activityName>
        <generalComment>
          <text xml:lang="en" index="1">Activity note</text>
        </generalComment>
      </activity>
      <timePeriod>
        <startDate>2024-01-01</startDate>
        <endDate>2025-12-31</endDate>
        <comment xml:lang="en">Temporal representativeness note</comment>
      </timePeriod>
      <geography>
        <shortname xml:lang="en">CH</shortname>
        <comment xml:lang="en">Swiss geography note</comment>
      </geography>
      <technology>
        <comment xml:lang="en">Heat pump route</comment>
      </technology>
    </activityDescription>
    <flowData>
      <intermediateExchange id="{product_id}" amount="1" productionVolumeAmount="42.0" isCalculatedAmount="true" intermediateExchangeId="{product_id}">
        <name xml:lang="en">enriched product</name>
        <unitName xml:lang="en">kg</unitName>
        <comment xml:lang="en">calculated reference product</comment>
        <outputGroup>0</outputGroup>
      </intermediateExchange>
      <intermediateExchange id="33333333-3333-4333-8333-333333333333" amount="0.2" isCalculatedAmount="true" activityLinkId="aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa">
        <name xml:lang="en">linked input</name>
        <unitName xml:lang="en">kg</unitName>
        <comment xml:lang="en">linked input comment</comment>
        <uncertainty>
          <lognormal meanValue="0.2" variance="0.12"/>
        </uncertainty>
        <inputGroup>5</inputGroup>
      </intermediateExchange>
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
    process = _read_only_process(output_dir / "tidas")
    process_info = process["processInformation"]
    linked_input = _exchange_for_flow(process, "linked input")

    assert status == 0
    assert report["validation"]["tidas"]["ok"] is True
    assert report["validation"]["ilcd"]["ok"] is True
    assert process_info["time"]["common:referenceYear"] == 2024
    assert process_info["time"]["common:dataSetValidUntil"] == 2025
    assert (
        process_info["time"]["common:timeRepresentativenessDescription"]["#text"]
        == "2024-01-01 - 2025-12-31; Temporal representativeness note"
    )
    assert (
        process_info["geography"]["locationOfOperationSupplyOrProduction"][
            "descriptionOfRestrictions"
        ]["#text"]
        == "Swiss geography note"
    )
    assert (
        process_info["technology"]["technologyDescriptionAndIncludedProcesses"]["#text"]
        == "Heat pump route"
    )
    assert (
        _trace_marker(process_info["dataSetInformation"]["common:other"])
        == "TIDAS_IMPORT_TRACE_V1"
    )
    assert linked_input["dataDerivationTypeStatus"] == "Calculated"
    assert linked_input["uncertaintyDistributionType"] == "log-normal"
    assert "linked input comment" in linked_input["generalComment"]["#text"]
    assert _trace_marker(linked_input["common:other"]) == "TIDAS_IMPORT_TRACE_V1"


def test_ecospold2_import_uses_filename_uuid_and_merges_generated_flows(tmp_path):
    source_dir = tmp_path / "source"
    source_dir.mkdir()
    first_uuid = "00000000-0000-4000-8000-000000000101"
    second_uuid = "00000000-0000-4000-8000-000000000102"
    _write_ecospold2_process(
        source_dir / f"process_{first_uuid}.spold",
        process_name="market for steel",
        product_name="steel",
    )
    _write_ecospold2_process(
        source_dir / f"process_{second_uuid}.spold",
        process_name="market for aluminium",
        product_name="aluminium",
    )

    output_dir = tmp_path / "out"
    status = main(
        [
            "--input",
            str(source_dir),
            "--output-dir",
            str(output_dir),
            "--target",
            "both",
        ]
    )

    report = json.loads(
        (output_dir / "conversion-report.json").read_text(encoding="utf-8")
    )
    first_process = _read_process(output_dir / "tidas", first_uuid)
    second_process = _read_process(output_dir / "tidas", second_uuid)
    first_co2_ref = _flow_ref_for_exchange(first_process, "carbon dioxide")
    second_co2_ref = _flow_ref_for_exchange(second_process, "carbon dioxide")

    assert status == 0
    assert report["validation"]["tidas"]["ok"] is True
    assert report["validation"]["ilcd"]["ok"] is True
    assert report["summary"]["processes"] == 2
    assert report["summary"]["flows"] == 3
    assert first_co2_ref == second_co2_ref
    assert len(list((output_dir / "tidas" / "flows").glob("*.json"))) == 3


def test_ecospold2_import_keeps_distinct_native_flow_uuids(tmp_path):
    source = tmp_path / "activity.spold"
    first_flow_id = "aaaaaaaa-0000-4000-8000-000000000001"
    second_flow_id = "aaaaaaaa-0000-4000-8000-000000000002"
    source.write_text(
        f"""<?xml version="1.0" encoding="UTF-8"?>
<ecoSpold xmlns="http://www.EcoInvent.org/EcoSpold02">
  <activityDataset>
    <activityDescription>
      <activity id="22222222-2222-4222-8222-222222222222">
        <activityName xml:lang="en">market for duplicate named emissions</activityName>
      </activity>
    </activityDescription>
    <flowData>
      <intermediateExchange id="11111111-1111-4111-8111-111111111111" amount="1">
        <name xml:lang="en">test product</name>
        <unitName xml:lang="en">kg</unitName>
        <outputGroup>0</outputGroup>
      </intermediateExchange>
      <elementaryExchange id="{first_flow_id}" amount="1.5">
        <name xml:lang="en">carbon dioxide</name>
        <unitName xml:lang="en">kg</unitName>
        <compartment>
          <compartment xml:lang="en">air</compartment>
          <subcompartment xml:lang="en">low population density</subcompartment>
        </compartment>
        <outputGroup>4</outputGroup>
      </elementaryExchange>
      <elementaryExchange id="{second_flow_id}" amount="2.5">
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
    process = _read_process(
        output_dir / "tidas", "22222222-2222-4222-8222-222222222222"
    )
    co2_refs = _flow_refs_for_exchanges(process, "carbon dioxide")

    assert status == 0
    assert report["validation"]["tidas"]["ok"] is True
    assert report["validation"]["ilcd"]["ok"] is True
    assert report["summary"]["flows"] == 3
    assert set(co2_refs) == {first_flow_id, second_flow_id}


def test_ecospold2_import_reads_child_activity_datasets_without_process_collisions(
    tmp_path,
):
    source_dir = tmp_path / "source"
    source_dir.mkdir()
    activity_id = "bbbbbbbb-0000-4000-8000-000000000001"
    first_product_id = "cccccccc-0000-4000-8000-000000000001"
    second_product_id = "cccccccc-0000-4000-8000-000000000002"
    _write_ecospold2_process(
        source_dir / f"{activity_id}_{first_product_id}.spold",
        process_name="multi output activity",
        product_name="first reference product",
        dataset_element="childActivityDataset",
        activity_id=activity_id,
        product_id=first_product_id,
    )
    _write_ecospold2_process(
        source_dir / f"{activity_id}_{second_product_id}.spold",
        process_name="multi output activity",
        product_name="second reference product",
        dataset_element="childActivityDataset",
        activity_id=activity_id,
        product_id=second_product_id,
    )

    output_dir = tmp_path / "out"
    status = main(
        [
            "--input",
            str(source_dir),
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
    assert report["validation"]["tidas"]["ok"] is True
    assert report["validation"]["ilcd"]["ok"] is True
    assert report["summary"]["processes"] == 2
    assert len(list((output_dir / "tidas" / "processes").glob("*.json"))) == 2


def _has_flow_property(tidas_dir, expected_name):
    for path in (tidas_dir / "flowproperties").glob("*.json"):
        data = json.loads(path.read_text(encoding="utf-8"))
        name = data["flowPropertyDataSet"]["flowPropertiesInformation"][
            "dataSetInformation"
        ]["common:name"]["#text"]
        if name == expected_name:
            return True
    return False


def _write_ecospold2_process(
    path,
    *,
    process_name,
    product_name,
    dataset_element="activityDataset",
    activity_id=None,
    product_id=None,
):
    activity_attribute = f' id="{activity_id}"' if activity_id else ""
    product_attribute = f' intermediateExchangeId="{product_id}"' if product_id else ""
    path.write_text(
        f"""<?xml version="1.0" encoding="UTF-8"?>
<ecoSpold xmlns="http://www.EcoInvent.org/EcoSpold02">
  <{dataset_element}>
    <activityDescription>
      <activity{activity_attribute}>
        <activityName xml:lang="en">{process_name}</activityName>
      </activity>
    </activityDescription>
    <flowData>
      <intermediateExchange amount="1"{product_attribute}>
        <name xml:lang="en">{product_name}</name>
        <unitName xml:lang="en">kg</unitName>
        <outputGroup>0</outputGroup>
      </intermediateExchange>
      <elementaryExchange amount="2.5">
        <name xml:lang="en">carbon dioxide</name>
        <unitName xml:lang="en">kg</unitName>
        <compartment>
          <compartment xml:lang="en">air</compartment>
          <subcompartment xml:lang="en">unspecified</subcompartment>
        </compartment>
        <outputGroup>4</outputGroup>
      </elementaryExchange>
    </flowData>
  </{dataset_element}>
</ecoSpold>
""",
        encoding="utf-8",
    )


def _read_process(tidas_dir, process_id):
    return json.loads(
        (tidas_dir / "processes" / f"{process_id}.json").read_text(encoding="utf-8")
    )["processDataSet"]


def _read_only_process(tidas_dir):
    process_paths = list((tidas_dir / "processes").glob("*.json"))
    assert len(process_paths) == 1
    return json.loads(process_paths[0].read_text(encoding="utf-8"))["processDataSet"]


def _flow_ref_for_exchange(process, flow_name):
    refs = _flow_refs_for_exchanges(process, flow_name)
    if not refs:
        raise AssertionError(f"Exchange not found: {flow_name}")
    return refs[0]


def _flow_refs_for_exchanges(process, flow_name):
    refs = []
    for exchange in process["exchanges"]["exchange"]:
        ref = exchange["referenceToFlowDataSet"]
        if ref["common:shortDescription"]["#text"] == flow_name:
            refs.append(ref["@refObjectId"])
    return refs


def _exchange_for_flow(process, flow_name):
    for exchange in process["exchanges"]["exchange"]:
        ref = exchange["referenceToFlowDataSet"]
        if ref["common:shortDescription"]["#text"] == flow_name:
            return exchange
    raise AssertionError(f"Exchange not found: {flow_name}")


def _trace_marker(common_other):
    return common_other["tidasimport:sourceTrace"]["@marker"]


def _find_flow(tidas_dir, expected_name):
    for path in (tidas_dir / "flows").glob("*.json"):
        data = json.loads(path.read_text(encoding="utf-8"))
        name = data["flowDataSet"]["flowInformation"]["dataSetInformation"]["name"][
            "baseName"
        ]["#text"]
        if name == expected_name:
            return data
    raise AssertionError(f"Flow not found: {expected_name}")
