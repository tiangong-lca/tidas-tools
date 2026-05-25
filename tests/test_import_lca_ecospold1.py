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


def test_ecospold1_import_reads_exchange_group_child_elements(tmp_path):
    process_uuid = "64e926e8-dd48-3704-b902-daaf546087c4"
    source = tmp_path / f"process_{process_uuid}.xml"
    source.write_text(
        """<?xml version="1.0" encoding="UTF-8"?>
<ecoSpold xmlns="http://www.EcoInvent.org/EcoSpold01">
  <dataset>
    <metaInformation>
      <processInformation>
        <referenceFunction name="BAFU-style child group fixture" />
      </processInformation>
    </metaInformation>
    <flowData>
      <exchange number="1" name="reference product" unit="kg" amount="1">
        <outputGroup>0</outputGroup>
      </exchange>
      <exchange number="2" name="electricity input" unit="kWh" meanValue="2" category="electricity" subCategory="supply mix">
        <inputGroup>5</inputGroup>
      </exchange>
      <exchange number="3" name="ore resource" unit="kg" meanValue="3" category="resources" subCategory="in ground">
        <inputGroup>4</inputGroup>
      </exchange>
      <exchange number="4" name="stack emission" unit="kg" meanValue="4" category="emissions to air" subCategory="unspecified">
        <outputGroup>4</outputGroup>
      </exchange>
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
    process = _read_process(output_dir / "tidas", process_uuid)
    electricity_exchange = _exchange_for_flow(process, "electricity input")
    resource_exchange = _exchange_for_flow(process, "ore resource")
    emission_exchange = _exchange_for_flow(process, "stack emission")

    assert status == 0
    assert report["validation"]["tidas"]["ok"] is True
    assert report["validation"]["ilcd"]["ok"] is True
    assert (
        _exchange_for_flow(process, "reference product")["exchangeDirection"]
        == "Output"
    )
    assert electricity_exchange["exchangeDirection"] == "Input"
    assert resource_exchange["exchangeDirection"] == "Input"
    assert emission_exchange["exchangeDirection"] == "Output"
    assert _flow_type(output_dir / "tidas", "electricity input") == "Product flow"
    assert _flow_type(output_dir / "tidas", "ore resource") == "Elementary flow"
    assert _flow_type(output_dir / "tidas", "stack emission") == "Elementary flow"
    exchange_trace = _trace_payload(electricity_exchange["common:other"])["sourceTrace"]
    assert exchange_trace["sourceClassification"]["inputGroup"] == "5"


def test_ecospold1_import_maps_semantic_fields_and_source_trace(tmp_path):
    process_uuid = "64e926e8-dd48-3704-b902-daaf546087c4"
    source = tmp_path / f"process_{process_uuid}.xml"
    source.write_text(
        """<?xml version="1.0" encoding="UTF-8"?>
<ecoSpold xmlns="http://www.EcoInvent.org/EcoSpold01">
  <dataset>
    <metaInformation>
      <processInformation>
        <referenceFunction name="market for enriched steel" generalComment="Reference function note" category="metals" subCategory="steel" localCategory="local metals" localSubCategory="local steel" />
        <geography location="CH" text="Swiss market boundary" />
        <technology text="Electric arc furnace route" />
        <timePeriod startDate="2020-01-01" endDate="2023-12-31" dataValidForEntirePeriod="true">survey period</timePeriod>
      </processInformation>
      <modellingAndValidation>
        <representativeness productionVolume="123 kg" samplingProcedure="supplier survey" uncertaintyAdjustments="pedigree adjusted" extrapolations="use only for Swiss market" />
      </modellingAndValidation>
      <sources>
        <source sourceNumber="10" firstAuthor="A. Author" year="2021" title="Steel source" text="Primary source note" />
      </sources>
    </metaInformation>
    <flowData>
      <exchange number="1" name="enriched steel" unit="kg" amount="1" outputGroup="0" />
      <exchange number="2" name="carbon dioxide" CASNumber="124-38-9" formula="CO2" unit="kg" amount="2.5000000000000001" outputGroup="4" category="air" subCategory="unspecified" localCategory="local air" localSubCategory="local stack" minValue="1.2" maxValue="3.4" uncertaintyType="1" standardDeviation95="12.3456" generalComment="measured stack emissions" localName="CO2 local" />
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
    process = _read_process(output_dir / "tidas", process_uuid)
    process_info = process["processInformation"]
    data_sources = process["modellingAndValidation"][
        "dataSourcesTreatmentAndRepresentativeness"
    ]
    co2_exchange = _exchange_for_flow(process, "carbon dioxide")
    co2_flow = _find_flow(output_dir / "tidas", "carbon dioxide")["flowDataSet"][
        "flowInformation"
    ]["dataSetInformation"]

    assert status == 0
    assert report["validation"]["tidas"]["ok"] is True
    assert report["validation"]["ilcd"]["ok"] is True
    assert process_info["time"]["common:referenceYear"] == 2020
    assert process_info["time"]["common:dataSetValidUntil"] == 2023
    assert (
        process_info["geography"]["locationOfOperationSupplyOrProduction"]["@location"]
        == "CH"
    )
    assert (
        process_info["technology"]["technologyDescriptionAndIncludedProcesses"]["#text"]
        == "Electric arc furnace route"
    )
    assert data_sources["annualSupplyOrProductionVolume"]["#text"] == "123 kg"
    assert data_sources["samplingProcedure"]["#text"] == "supplier survey"
    assert "source data set" == data_sources["referenceToDataSource"]["@type"]
    assert (
        _trace_marker(process_info["dataSetInformation"]["common:other"])
        == "TIDAS_IMPORT_TRACE_V1"
    )
    process_trace = _trace_payload(process_info["dataSetInformation"]["common:other"])
    assert process_trace["sourceClassification"]["category"] == "metals"
    assert process_trace["sourceClassification"]["localCategory"] == "local metals"
    assert co2_exchange["meanAmount"] == "2.5000000000000001"
    assert co2_exchange["minimumAmount"] == "1.2"
    assert co2_exchange["maximumAmount"] == "3.4"
    assert co2_exchange["uncertaintyDistributionType"] == "log-normal"
    assert "relativeStandardDeviation95In" not in co2_exchange
    assert "measured stack emissions" in co2_exchange["generalComment"]["#text"]
    assert _trace_marker(co2_exchange["common:other"]) == "TIDAS_IMPORT_TRACE_V1"
    exchange_trace = _trace_payload(co2_exchange["common:other"])["sourceTrace"]
    assert exchange_trace["sourceClassification"]["localCategory"] == "local air"
    assert (
        _trace_attribute(exchange_trace["exchange"], "standardDeviation95") == "12.3456"
    )
    assert co2_flow["CASNumber"] == "124-38-9"
    assert co2_flow["sumFormula"] == "CO2"
    assert co2_flow["common:synonyms"]["#text"] == "CO2 local"
    assert _trace_marker(co2_flow["common:other"]) == "TIDAS_IMPORT_TRACE_V1"
    flow_trace = _trace_payload(co2_flow["common:other"])
    assert flow_trace["sourceClassification"]["subCategory"] == "unspecified"


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


def test_ecospold1_import_merges_duplicate_flows_across_processes(tmp_path):
    source_dir = tmp_path / "source"
    source_dir.mkdir()
    first_uuid = "00000000-0000-4000-8000-000000000001"
    second_uuid = "00000000-0000-4000-8000-000000000002"
    _write_ecospold1_process(
        source_dir / f"process_{first_uuid}.xml",
        process_name="market for steel",
        product_name="steel",
    )
    _write_ecospold1_process(
        source_dir / f"process_{second_uuid}.xml",
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


def _write_ecospold1_process(path, *, process_name, product_name):
    path.write_text(
        f"""<?xml version="1.0" encoding="UTF-8"?>
<ecoSpold xmlns="http://www.EcoInvent.org/EcoSpold01">
  <dataset>
    <metaInformation>
      <processInformation>
        <referenceFunction name="{process_name}" />
      </processInformation>
    </metaInformation>
    <flowData>
      <exchange number="1" name="{product_name}" unit="kg" amount="1" outputGroup="0" />
      <exchange number="2" name="carbon dioxide" unit="kg" meanValue="2.5" outputGroup="4" category="air" subCategory="unspecified" />
    </flowData>
  </dataset>
</ecoSpold>
""",
        encoding="utf-8",
    )


def _read_process(tidas_dir, process_id):
    return json.loads(
        (tidas_dir / "processes" / f"{process_id}.json").read_text(encoding="utf-8")
    )["processDataSet"]


def _flow_ref_for_exchange(process, flow_name):
    for exchange in process["exchanges"]["exchange"]:
        ref = exchange["referenceToFlowDataSet"]
        if ref["common:shortDescription"]["#text"] == flow_name:
            return ref["@refObjectId"]
    raise AssertionError(f"Exchange not found: {flow_name}")


def _exchange_for_flow(process, flow_name):
    for exchange in process["exchanges"]["exchange"]:
        ref = exchange["referenceToFlowDataSet"]
        if ref["common:shortDescription"]["#text"] == flow_name:
            return exchange
    raise AssertionError(f"Exchange not found: {flow_name}")


def _trace_marker(common_other):
    return common_other["tidasimport:sourceTrace"]["@marker"]


def _trace_payload(common_other):
    return common_other["tidasimport:sourceTrace"]["payload"]


def _trace_attribute(trace, name):
    for attribute in trace.get("attributes", []):
        if attribute["name"] == name:
            return attribute["value"]
    raise AssertionError(f"Trace attribute not found: {name}")


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


def _flow_type(tidas_dir, expected_name):
    return _find_flow(tidas_dir, expected_name)["flowDataSet"][
        "modellingAndValidation"
    ]["LCIMethod"]["typeOfDataSet"]
