import json

from tidas_tools.import_lca.cli import main
from tidas_tools.validate import validate_package_dir

FLOW_ID = "11111111-1111-4111-8111-111111111111"
PROCESS_ID = "22222222-2222-4222-8222-222222222222"
UNIT_GROUP_ID = "33333333-3333-4333-8333-333333333333"
FLOW_PROPERTY_ID = "44444444-4444-4444-8444-444444444444"
ACTOR_ID = "55555555-5555-4555-8555-555555555555"
SOURCE_ID = "66666666-6666-4666-8666-666666666666"
PROVIDER_PROCESS_ID = "77777777-7777-4777-8777-777777777777"
CONSUMER_PROCESS_ID = "88888888-8888-4888-8888-888888888888"
LOCATION_ID = "99999999-9999-4999-8999-999999999999"


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
    assert report["summary"]["sources"] == 2
    assert report["validation"]["tidas"]["ok"] is True
    assert report["target"]["mapping_csv"] is None
    assert report["target"]["mapping_csv_rows"] is None
    assert not (output_dir / "mapping.csv").exists()
    assert not (output_dir / "mapping.csv.gz").exists()
    process_payload = json.loads(
        (output_dir / "tidas" / "processes" / f"{PROCESS_ID}.json").read_text(
            encoding="utf-8"
        )
    )
    assert (
        process_payload["processDataSet"]["processInformation"]["time"][
            "common:referenceYear"
        ]
        == 9999
    )
    process = process_payload["processDataSet"]
    assert (
        process["processInformation"]["geography"][
            "locationOfOperationSupplyOrProduction"
        ]["@location"]
        == "US"
    )
    review = process["modellingAndValidation"]["validation"]["review"]
    assert review["@type"] == "Independent external review"
    assert review["common:referenceToCompleteReviewReport"]["@refObjectId"] == SOURCE_ID
    classification_trace = process["processInformation"]["dataSetInformation"][
        "classificationInformation"
    ]["common:classification"]["common:other"]["tidasimport:sourceTrace"]
    assert classification_trace["@marker"] == "TIDAS_IMPORT_TRACE_V1"
    assert classification_trace["payload"]["sourceCategoryPath"] == [
        "31-33: Manufacturing",
        "3311: Iron and Steel Mills and Ferroalloy Manufacturing",
    ]

    flow_payload = json.loads(
        (output_dir / "tidas" / "flows" / f"{FLOW_ID}.json").read_text(encoding="utf-8")
    )
    flow_info = flow_payload["flowDataSet"]["flowInformation"]["dataSetInformation"]
    assert flow_info["CASNumber"] == "7439-89-6"
    assert flow_info["classificationInformation"]["common:classification"][
        "common:other"
    ]["tidasimport:sourceTrace"]["payload"]["sourceCategoryPath"] == [
        "Technosphere flows",
        "Metals",
    ]
    exchange = process["exchanges"]["exchange"][0]
    assert exchange["meanAmount"] == "0.123456789012345678"


def test_openlca_jsonld_minimal_import_writes_process_bundles_by_default(tmp_path):
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

    bundle_dir = output_dir / "process-bundles" / PROCESS_ID
    manifest = json.loads((bundle_dir / "manifest.json").read_text(encoding="utf-8"))
    index = json.loads(
        (output_dir / "process-bundles" / "index.json").read_text(encoding="utf-8")
    )
    report = json.loads(
        (output_dir / "conversion-report.json").read_text(encoding="utf-8")
    )

    assert status == 0
    assert index["process_count"] == 1
    assert index["bundles"][0]["process_id"] == PROCESS_ID
    assert manifest["process_id"] == PROCESS_ID
    assert manifest["unresolved_references"] == []
    assert f"tidas/processes/{PROCESS_ID}.json" in manifest["files"]["processes"]
    assert f"tidas/flows/{FLOW_ID}.json" in manifest["files"]["flows"]
    assert (
        f"tidas/flowproperties/{FLOW_PROPERTY_ID}.json"
        in manifest["files"]["flowproperties"]
    )
    assert f"tidas/unitgroups/{UNIT_GROUP_ID}.json" in manifest["files"]["unitgroups"]
    assert f"tidas/sources/{SOURCE_ID}.json" in manifest["files"]["sources"]
    assert (bundle_dir / "tidas" / "processes" / f"{PROCESS_ID}.json").is_file()
    assert (bundle_dir / "tidas" / "flows" / f"{FLOW_ID}.json").is_file()
    assert (
        bundle_dir / "tidas" / "flowproperties" / f"{FLOW_PROPERTY_ID}.json"
    ).is_file()
    assert (bundle_dir / "tidas" / "unitgroups" / f"{UNIT_GROUP_ID}.json").is_file()
    assert report["target"]["process_bundles_dir"] == str(
        output_dir / "process-bundles"
    )
    assert report["target"]["process_bundle_count"] == 1
    assert report["summary"]["process_bundles"] == 1

    validation = validate_package_dir(str(bundle_dir / "tidas"))
    assert validation["ok"] is True


def test_openlca_jsonld_minimal_import_can_disable_process_bundles(tmp_path):
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
            "--no-process-bundles",
        ]
    )

    report = json.loads(
        (output_dir / "conversion-report.json").read_text(encoding="utf-8")
    )

    assert status == 0
    assert not (output_dir / "process-bundles").exists()
    assert report["target"]["process_bundles_dir"] is None
    assert report["target"]["process_bundle_count"] is None
    assert report["summary"]["process_bundles"] == 0


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


def test_openlca_jsonld_provider_links_create_valid_lifecycle_model(tmp_path):
    source_dir = write_provider_graph_jsonld_fixture(tmp_path)
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

    report = json.loads(
        (output_dir / "conversion-report.json").read_text(encoding="utf-8")
    )
    lifecycle_files = list((output_dir / "tidas" / "lifecyclemodels").glob("*.json"))

    assert status == 0
    assert report["summary"]["lifecyclemodels"] == 1
    assert report["validation"]["tidas"]["ok"] is True
    assert len(lifecycle_files) == 1

    lifecycle = json.loads(lifecycle_files[0].read_text(encoding="utf-8"))
    model = lifecycle["lifeCycleModelDataSet"]
    instances = model["lifeCycleModelInformation"]["technology"]["processes"][
        "processInstance"
    ]
    instance_by_process = {
        item["referenceToProcess"]["@refObjectId"]: item for item in instances
    }
    provider_instance = instance_by_process[PROVIDER_PROCESS_ID]
    consumer_instance = instance_by_process[CONSUMER_PROCESS_ID]
    output_exchange = provider_instance["connections"]["outputExchange"]
    downstream = output_exchange["downstreamProcess"]

    assert output_exchange["@flowUUID"] == FLOW_ID
    assert downstream["@id"] == consumer_instance["@dataSetInternalID"]
    assert downstream["@flowUUID"] == FLOW_ID
    assert (
        model["lifeCycleModelInformation"]["dataSetInformation"]["common:other"][
            "tidasimport:sourceTrace"
        ]["@marker"]
        == "TIDAS_IMPORT_TRACE_V1"
    )

    consumer = json.loads(
        (output_dir / "tidas" / "processes" / f"{CONSUMER_PROCESS_ID}.json").read_text(
            encoding="utf-8"
        )
    )
    exchange = consumer["processDataSet"]["exchanges"]["exchange"][0]
    trace = exchange["common:other"]["tidasimport:sourceTrace"]
    assert trace["@marker"] == "TIDAS_IMPORT_TRACE_V1"
    assert trace["payload"]["exchangeMetadata"]["providerRefId"] == PROVIDER_PROCESS_ID


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
    (source_dir / "location.json").write_text(
        json.dumps(
            {
                "@type": "Location",
                "@id": LOCATION_ID,
                "name": "United States of America (the)",
                "code": "US",
                "category": "Country",
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
                "category": "Technosphere flows/Metals",
                "cas": "7439-89-6",
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
    process_payload = {
        "@type": "Process",
        "@id": PROCESS_ID,
        "name": "Steel production",
        "category": "31-33: Manufacturing/3311: Iron and Steel Mills and Ferroalloy Manufacturing",
        "description": "Minimal process fixture",
        "location": {
            "@type": "Location",
            "@id": LOCATION_ID,
            "name": "United States of America (the)",
        },
        "processDocumentation": {
            "reviews": [
                {
                    "reviewType": "Independent external review",
                    "details": "Fixture review details",
                    "report": {
                        "@type": "Source",
                        "@id": SOURCE_ID,
                        "name": "Example source",
                    },
                }
            ]
        },
        "exchanges": [
            {
                "internalId": 1,
                "flow": {"@id": FLOW_ID, "name": "Steel product"},
                "amount": "__AMOUNT__",
                "isInput": False,
                "isQuantitativeReference": True,
            }
        ],
    }
    (source_dir / "process.json").write_text(
        json.dumps(process_payload).replace('"__AMOUNT__"', "0.123456789012345678"),
        encoding="utf-8",
    )
    return source_dir


def write_provider_graph_jsonld_fixture(tmp_path):
    source_dir = write_minimal_jsonld_fixture(tmp_path)
    (source_dir / "provider_process.json").write_text(
        json.dumps(
            {
                "@type": "Process",
                "@id": PROVIDER_PROCESS_ID,
                "name": "Steel billet production",
                "description": "Provider process fixture",
                "processType": "UNIT_PROCESS",
                "processDocumentation": {
                    "validFrom": "2020-01-01",
                    "validUntil": "2024-12-31",
                    "technologyDescription": "Electric arc furnace route",
                    "sources": [
                        {
                            "@type": "Source",
                            "@id": SOURCE_ID,
                            "name": "Example source",
                        }
                    ],
                    "dataGenerator": {
                        "@type": "Actor",
                        "@id": ACTOR_ID,
                        "name": "Example data author",
                    },
                },
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
    (source_dir / "consumer_process.json").write_text(
        json.dumps(
            {
                "@type": "Process",
                "@id": CONSUMER_PROCESS_ID,
                "name": "Steel sheet production",
                "description": "Consumer process fixture",
                "processType": "UNIT_PROCESS",
                "defaultAllocationMethod": "PHYSICAL_ALLOCATION",
                "processDocumentation": {
                    "validFrom": "2021-01-01",
                    "geographyDescription": "United States",
                    "samplingDescription": "Secondary data sample",
                    "intendedApplication": "Provider graph test fixture",
                },
                "exchanges": [
                    {
                        "internalId": 1,
                        "flow": {"@id": FLOW_ID, "name": "Steel product"},
                        "amount": 2.5,
                        "isInput": True,
                        "unit": {"@id": "kg", "name": "kg"},
                        "defaultProvider": {
                            "@type": "Process",
                            "@id": PROVIDER_PROCESS_ID,
                            "name": "Steel billet production",
                        },
                        "uncertainty": {
                            "@type": "Uncertainty",
                            "distributionType": "TRIANGLE_DISTRIBUTION",
                            "minimum": 2.0,
                            "maximum": 3.0,
                        },
                    },
                    {
                        "internalId": 2,
                        "flow": {"@id": FLOW_ID, "name": "Steel product"},
                        "amount": 1.0,
                        "isInput": False,
                        "isQuantitativeReference": True,
                    },
                ],
            }
        ),
        encoding="utf-8",
    )
    return source_dir
