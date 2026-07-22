from decimal import Decimal
import json

from tidas_tools.import_lca.adapters.openlca_jsonld import OpenLcaJsonLdAdapter
from tidas_tools.import_lca.cli import main
from tidas_tools.import_lca.report import ConversionReport
from tidas_tools.import_lca.store import MemoryCanonicalStore
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
MASS_UNIT_GROUP_ID = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa"
ENERGY_UNIT_GROUP_ID = "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb"
MASS_FLOW_PROPERTY_ID = "cccccccc-cccc-4ccc-8ccc-cccccccccccc"
ENERGY_FLOW_PROPERTY_ID = "dddddddd-dddd-4ddd-8ddd-dddddddddddd"
KG_UNIT_ID = "eeeeeeee-eeee-4eee-8eee-eeeeeeeeeeee"
G_UNIT_ID = "ffffffff-ffff-4fff-8fff-ffffffffffff"
MJ_UNIT_ID = "abababab-abab-4bab-8bab-abababababab"
FUEL_FLOW_ID = "12121212-1212-4121-8121-121212121212"
UNKNOWN_UNIT_ID = "34343434-3434-4343-8343-343434343434"


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


def test_openlca_jsonld_normalizes_same_property_exchange_units(tmp_path):
    source_dir = write_unit_normalization_jsonld_fixture(tmp_path)
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
    process_payload = json.loads(
        (output_dir / "tidas" / "processes" / f"{PROCESS_ID}.json").read_text(
            encoding="utf-8"
        )
    )
    exchanges = process_payload["processDataSet"]["exchanges"]["exchange"]
    gram_exchange = exchanges[1]
    assert Decimal(gram_exchange["meanAmount"]) == Decimal("0.5")
    assert Decimal(gram_exchange["resultingAmount"]) == Decimal("0.5")
    metadata = gram_exchange["common:other"]["tidasimport:sourceTrace"]["payload"][
        "exchangeMetadata"
    ]
    assert metadata["unitId"] == KG_UNIT_ID
    assert metadata["unitName"] == "kg"

    report = json.loads(
        (output_dir / "conversion-report.json").read_text(encoding="utf-8")
    )
    issue = next(
        issue
        for issue in report["issues"]
        if issue["code"] == "exchange_amounts_normalized_to_reference_units"
    )
    assert issue["severity"] == "warning"
    assert issue["context"]["normalized_same_property"] == 1
    assert issue["context"]["already_reference"] == 1

    store, _ = read_canonical_store(source_dir)
    exchange = store.get("processes", PROCESS_ID).raw["exchanges"][1]
    assert exchange["amount"] == Decimal("0.5")
    assert exchange["sourceAmount"] == 500
    assert exchange["sourceUnitId"] == G_UNIT_ID
    assert exchange["sourceUnitName"] == "g"
    assert exchange["unitId"] == KG_UNIT_ID
    assert exchange["unitName"] == "kg"
    normalization = exchange["amountNormalization"]
    assert Decimal(normalization["factor"]) == Decimal("0.001")
    assert normalization["sourceUnit"] == "g"
    assert normalization["targetUnit"] == "kg"
    assert normalization["crossProperty"] is False


def test_openlca_jsonld_normalizes_cross_property_exchange_to_reference_property(
    tmp_path,
):
    source_dir = write_unit_normalization_jsonld_fixture(tmp_path)

    store, report = read_canonical_store(source_dir)

    exchange = store.get("processes", PROCESS_ID).raw["exchanges"][2]
    assert exchange["amount"] == Decimal("2")
    assert exchange["sourceAmount"] == 100
    assert exchange["unitId"] == KG_UNIT_ID
    assert exchange["unitName"] == "kg"
    assert exchange["sourceUnitId"] == MJ_UNIT_ID
    assert exchange["sourceUnitName"] == "MJ"
    assert exchange["flowPropertyRefId"] == MASS_FLOW_PROPERTY_ID
    assert exchange["flowPropertyName"] == "Mass"
    assert exchange["sourceFlowPropertyRefId"] == ENERGY_FLOW_PROPERTY_ID
    assert exchange["sourceFlowPropertyName"] == "Energy"
    normalization = exchange["amountNormalization"]
    assert normalization["crossProperty"] is True
    assert Decimal(normalization["factor"]) == Decimal("0.02")
    assert normalization["sourceUnit"] == "MJ"
    assert normalization["targetUnit"] == "kg"

    issue = next(
        issue
        for issue in report.issues
        if issue.code == "exchange_amounts_normalized_to_reference_units"
    )
    assert issue.context["normalized_cross_property"] == 1


def test_openlca_jsonld_leaves_unknown_unit_exchange_unresolved(tmp_path):
    source_dir = write_unit_normalization_jsonld_fixture(tmp_path)

    store, report = read_canonical_store(source_dir)

    exchange = store.get("processes", PROCESS_ID).raw["exchanges"][3]
    assert exchange["amount"] == 7
    assert "amountNormalization" not in exchange
    assert "sourceAmount" not in exchange
    assert exchange["unitId"] == UNKNOWN_UNIT_ID

    issue = next(
        issue
        for issue in report.issues
        if issue.code == "exchange_amounts_normalized_to_reference_units"
    )
    assert issue.context["unresolved"] == {"unknown_unit": 1}
    sample = issue.context["unresolved_samples"][0]
    assert sample["process_id"] == PROCESS_ID
    assert sample["internalId"] == 4
    assert sample["unitId"] == UNKNOWN_UNIT_ID
    assert sample["reason"] == "unknown_unit"

    unresolved_issue = next(
        issue
        for issue in report.issues
        if issue.code == "exchange_unit_normalization_unresolved"
    )
    assert unresolved_issue.severity == "warning"
    assert unresolved_issue.context["unresolved_total"] == 1
    assert unresolved_issue.context["unresolved"] == {"unknown_unit": 1}


def test_openlca_jsonld_scales_uncertainty_bounds_with_unit_normalization(tmp_path):
    source_dir = write_unit_normalization_jsonld_fixture(tmp_path)

    store, _ = read_canonical_store(source_dir)

    exchange = store.get("processes", PROCESS_ID).raw["exchanges"][1]
    assert exchange["minimumAmount"] == Decimal("0.4")
    assert exchange["maximumAmount"] == Decimal("0.6")
    assert exchange["uncertaintyDistributionType"] == "triangular"


def test_openlca_jsonld_lifecycle_model_uses_normalized_exchange_amounts(tmp_path):
    source_dir = write_unit_normalization_provider_jsonld_fixture(tmp_path)

    store, _ = read_canonical_store(source_dir)

    models = list(store.iter_type("lifecyclemodels"))
    assert len(models) == 1
    connections = models[0].raw["connections"]
    assert len(connections) == 1
    connection = connections[0]
    assert connection["providerProcessId"] == PROVIDER_PROCESS_ID
    assert connection["consumerProcessId"] == CONSUMER_PROCESS_ID
    assert connection["amount"] == Decimal("0.5")


def read_canonical_store(source_dir):
    store = MemoryCanonicalStore()
    report = ConversionReport(source_path=str(source_dir))
    OpenLcaJsonLdAdapter().read(source_dir, store, report)
    return store, report


def write_unit_normalization_jsonld_fixture(tmp_path):
    source_dir = tmp_path / "jsonld"
    source_dir.mkdir()
    (source_dir / "context.jsonld").write_text("{}", encoding="utf-8")
    (source_dir / "unit_group_mass.json").write_text(
        json.dumps(
            {
                "@type": "UnitGroup",
                "@id": MASS_UNIT_GROUP_ID,
                "name": "Units of mass",
                "units": [
                    {
                        "@id": KG_UNIT_ID,
                        "name": "kg",
                        "conversionFactor": 1.0,
                        "referenceUnit": True,
                    },
                    {"@id": G_UNIT_ID, "name": "g", "conversionFactor": 0.001},
                ],
            }
        ),
        encoding="utf-8",
    )
    (source_dir / "unit_group_energy.json").write_text(
        json.dumps(
            {
                "@type": "UnitGroup",
                "@id": ENERGY_UNIT_GROUP_ID,
                "name": "Units of energy",
                "units": [
                    {
                        "@id": MJ_UNIT_ID,
                        "name": "MJ",
                        "conversionFactor": 1.0,
                        "referenceUnit": True,
                    }
                ],
            }
        ),
        encoding="utf-8",
    )
    (source_dir / "flow_property_mass.json").write_text(
        json.dumps(
            {
                "@type": "FlowProperty",
                "@id": MASS_FLOW_PROPERTY_ID,
                "name": "Mass",
                "unitGroup": {"@id": MASS_UNIT_GROUP_ID, "name": "Units of mass"},
            }
        ),
        encoding="utf-8",
    )
    (source_dir / "flow_property_energy.json").write_text(
        json.dumps(
            {
                "@type": "FlowProperty",
                "@id": ENERGY_FLOW_PROPERTY_ID,
                "name": "Energy",
                "unitGroup": {"@id": ENERGY_UNIT_GROUP_ID, "name": "Units of energy"},
            }
        ),
        encoding="utf-8",
    )
    (source_dir / "flow_steel.json").write_text(
        json.dumps(
            {
                "@type": "Flow",
                "@id": FLOW_ID,
                "name": "Steel product",
                "flowType": "PRODUCT_FLOW",
                "flowProperties": [
                    {
                        "flowProperty": {
                            "@id": MASS_FLOW_PROPERTY_ID,
                            "name": "Mass",
                        },
                        "conversionFactor": 1.0,
                        "isRefFlowProperty": True,
                    }
                ],
            }
        ),
        encoding="utf-8",
    )
    (source_dir / "flow_fuel.json").write_text(
        json.dumps(
            {
                "@type": "Flow",
                "@id": FUEL_FLOW_ID,
                "name": "Fuel",
                "flowType": "PRODUCT_FLOW",
                "flowProperties": [
                    {
                        "flowProperty": {
                            "@id": MASS_FLOW_PROPERTY_ID,
                            "name": "Mass",
                        },
                        "conversionFactor": 1.0,
                        "isRefFlowProperty": True,
                    },
                    {
                        "flowProperty": {
                            "@id": ENERGY_FLOW_PROPERTY_ID,
                            "name": "Energy",
                        },
                        "conversionFactor": 50.0,
                    },
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
                "description": "Unit normalization fixture",
                "exchanges": [
                    {
                        "internalId": 1,
                        "flow": {
                            "@id": FLOW_ID,
                            "name": "Steel product",
                            "refUnit": "kg",
                        },
                        "amount": 1.0,
                        "isInput": False,
                        "isQuantitativeReference": True,
                        "unit": {"@id": KG_UNIT_ID, "name": "kg"},
                        "flowProperty": {
                            "@id": MASS_FLOW_PROPERTY_ID,
                            "name": "Mass",
                        },
                    },
                    {
                        "internalId": 2,
                        "flow": {
                            "@id": FLOW_ID,
                            "name": "Steel product",
                            "refUnit": "kg",
                        },
                        "amount": 500,
                        "isInput": True,
                        "unit": {"@id": G_UNIT_ID, "name": "g"},
                        "flowProperty": {
                            "@id": MASS_FLOW_PROPERTY_ID,
                            "name": "Mass",
                        },
                        "uncertainty": {
                            "@type": "Uncertainty",
                            "distributionType": "TRIANGLE_DISTRIBUTION",
                            "minimum": 400,
                            "maximum": 600,
                        },
                    },
                    {
                        "internalId": 3,
                        "flow": {
                            "@id": FUEL_FLOW_ID,
                            "name": "Fuel",
                            "refUnit": "kg",
                        },
                        "amount": 100,
                        "isInput": True,
                        "unit": {"@id": MJ_UNIT_ID, "name": "MJ"},
                        "flowProperty": {
                            "@id": ENERGY_FLOW_PROPERTY_ID,
                            "name": "Energy",
                        },
                    },
                    {
                        "internalId": 4,
                        "flow": {
                            "@id": FLOW_ID,
                            "name": "Steel product",
                            "refUnit": "kg",
                        },
                        "amount": 7,
                        "isInput": True,
                        "unit": {"@id": UNKNOWN_UNIT_ID, "name": "bogus"},
                        "flowProperty": {
                            "@id": MASS_FLOW_PROPERTY_ID,
                            "name": "Mass",
                        },
                    },
                ],
            }
        ),
        encoding="utf-8",
    )
    return source_dir


def write_unit_normalization_provider_jsonld_fixture(tmp_path):
    source_dir = write_unit_normalization_jsonld_fixture(tmp_path)
    (source_dir / "provider_process.json").write_text(
        json.dumps(
            {
                "@type": "Process",
                "@id": PROVIDER_PROCESS_ID,
                "name": "Steel billet production",
                "processType": "UNIT_PROCESS",
                "exchanges": [
                    {
                        "internalId": 1,
                        "flow": {
                            "@id": FLOW_ID,
                            "name": "Steel product",
                            "refUnit": "kg",
                        },
                        "amount": 1.0,
                        "isInput": False,
                        "isQuantitativeReference": True,
                        "unit": {"@id": KG_UNIT_ID, "name": "kg"},
                        "flowProperty": {
                            "@id": MASS_FLOW_PROPERTY_ID,
                            "name": "Mass",
                        },
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
                "processType": "UNIT_PROCESS",
                "exchanges": [
                    {
                        "internalId": 1,
                        "flow": {
                            "@id": FLOW_ID,
                            "name": "Steel product",
                            "refUnit": "kg",
                        },
                        "amount": 500,
                        "isInput": True,
                        "unit": {"@id": G_UNIT_ID, "name": "g"},
                        "flowProperty": {
                            "@id": MASS_FLOW_PROPERTY_ID,
                            "name": "Mass",
                        },
                        "defaultProvider": {
                            "@type": "Process",
                            "@id": PROVIDER_PROCESS_ID,
                            "name": "Steel billet production",
                        },
                    },
                    {
                        "internalId": 2,
                        "flow": {
                            "@id": FUEL_FLOW_ID,
                            "name": "Fuel",
                            "refUnit": "kg",
                        },
                        "amount": 1.0,
                        "isInput": False,
                        "isQuantitativeReference": True,
                        "unit": {"@id": KG_UNIT_ID, "name": "kg"},
                        "flowProperty": {
                            "@id": MASS_FLOW_PROPERTY_ID,
                            "name": "Mass",
                        },
                    },
                ],
            }
        ),
        encoding="utf-8",
    )
    return source_dir


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


def test_elementary_categorization_maps_fedefl_compartments():
    from tidas_tools.import_lca.writers.tidas_json import (
        _elementary_categorization,
        _flow_classification,
    )

    def leaf(path):
        cats = _elementary_categorization({"category": path})
        assert cats is not None, path
        return (cats[0]["#text"], cats[-1]["@catId"], cats[-1]["#text"])

    # Emissions: real compartment -> correct leaf (was all "air, unspecified" before)
    assert leaf("Elementary flows/emission/air") == (
        "Emissions",
        "1.3.4",
        "Emissions to air, unspecified",
    )
    assert leaf("Elementary flows/emission/air/troposphere/urban") == (
        "Emissions",
        "1.3.1",
        "Emissions to urban air close to ground",
    )
    assert leaf("Elementary flows/emission/air/troposphere/rural") == (
        "Emissions",
        "1.3.2",
        "Emissions to non-urban air or from high stacks",
    )
    assert leaf("Elementary flows/emission/air/stratosphere") == (
        "Emissions",
        "1.3.3",
        "Emissions to lower stratosphere and upper troposphere",
    )
    assert leaf("Elementary flows/emission/water/saline water body/ocean") == (
        "Emissions",
        "1.1.2",
        "Emissions to sea water",
    )
    assert leaf("Elementary flows/emission/water/fresh water body/river") == (
        "Emissions",
        "1.1.1",
        "Emissions to fresh water",
    )
    assert leaf("Elementary flows/emission/ground") == (
        "Emissions",
        "1.2.3",
        "Emissions to soil, unspecified",
    )
    assert leaf("Elementary flows/emission/ground/human-dominated/agricultural") == (
        "Emissions",
        "1.2.1",
        "Emissions to agricultural soil",
    )
    # Resources: correct top-level category kind (was wrongly under "Emissions")
    assert leaf("Elementary flows/resource/ground/subterranean") == (
        "Resources",
        "2.1.8",
        "Non-renewable resources from ground, unspecified",
    )
    assert leaf("Elementary flows/resource/water") == (
        "Resources",
        "2.2.7",
        "Renewable resources from water, unspecified",
    )

    # ecoSpold single-token compartments and non-FEDEFL shapes -> None (no regression):
    # the writer keeps the legacy "air, unspecified" default for these.
    assert _elementary_categorization({"category": "air"}) is None
    assert _elementary_categorization({"category": "resource"}) is None
    assert (
        _elementary_categorization({"category": "air/low population density"}) is None
    )
    assert _elementary_categorization({}) is None
    default = _flow_classification("Elementary flow", {"category": "resource"})
    leaves = default["common:elementaryFlowCategorization"]["common:category"]
    assert leaves[-1]["#text"] == "Emissions to air, unspecified"


def test_uncertainty_dispersion_maps_lognormal_and_normal():
    from decimal import Decimal
    from tidas_tools.import_lca.adapters.openlca_jsonld import _uncertainty_dispersion

    # log-normal -> GSD^2, rounded to TIDAS Perc (<=3 decimals, 0..100)
    assert _uncertainty_dispersion(
        {"distributionType": "LOG_NORMAL_DISTRIBUTION", "geomSd": Decimal("2.0")}
    ) == Decimal("4.000")
    assert _uncertainty_dispersion(
        {"distributionType": "LOG_NORMAL_DISTRIBUTION", "geomSd": Decimal("1.05")}
    ) == Decimal("1.102")
    # GSD^2 > 100 has no valid Perc slot -> None (residual U2, stays in trace)
    assert (
        _uncertainty_dispersion(
            {"distributionType": "LOG_NORMAL_DISTRIBUTION", "geomSd": Decimal("11")}
        )
        is None
    )
    # normal -> 2*SD
    assert _uncertainty_dispersion(
        {"distributionType": "NORMAL_DISTRIBUTION", "sd": Decimal("3")}
    ) == Decimal("6.000")
    # triangular/uniform carry no dispersion (use min/max)
    assert (
        _uncertainty_dispersion(
            {"distributionType": "TRIANGLE_DISTRIBUTION", "minimum": 1, "maximum": 2}
        )
        is None
    )


def test_process_pedigree_maps_to_ilcd_data_quality_indicators():
    from tidas_tools.import_lca.adapters.openlca_jsonld import (
        _dq_system_index,
        _process_data_quality_indicators,
    )

    system_id = "70bf370f-9912-4ec1-baa3-fbd4eaf85a10"
    system = {
        "@type": "DQSystem",
        "@id": system_id,
        "indicators": [
            {"position": 1, "name": "Process Review"},
            {"position": 2, "name": "Process Completeness"},
        ],
    }
    index = _dq_system_index([("x", system)])
    assert index == {system_id: [(1, "Process Review"), (2, "Process Completeness")]}
    indicators = _process_data_quality_indicators(
        {"dqEntry": "(2;1)", "dqSystem": {"@id": system_id}}, index
    )
    assert indicators == [
        {"@name": "Methodological appropriateness and consistency", "@value": "Good"},
        {"@name": "Completeness", "@value": "Very good"},
    ]
    # no pedigree -> no indicators (raw still round-trips in trace)
    assert _process_data_quality_indicators({}, index) == []


def test_allocation_factors_map_to_exchange_allocations_list():
    from decimal import Decimal
    from tidas_tools.import_lca.writers.tidas_json import (
        _apply_exchange_allocations,
        _exchange_item,
    )

    class _Ent:
        raw = {
            "allocationFactors": [
                {
                    "exchange": {"internalId": 7, "flow": {"@id": "in-x"}},
                    "product": {"@id": "prodA"},
                    "value": Decimal("0.6"),
                },
                {
                    "exchange": {"internalId": 7, "flow": {"@id": "in-x"}},
                    "product": {"@id": "prodB"},
                    "value": Decimal("0.4"),
                },
                {
                    "exchange": {"internalId": 7, "flow": {"@id": "in-x"}},
                    "product": {"@id": "prodA"},
                    "value": Decimal("0"),
                },  # sparse zero -> omitted
            ]
        }

    exchanges = [
        {
            "internalId": 7,
            "isInput": True,
            "flow": {"@id": "in-x"},
            "amount": Decimal("5"),
        },
        {
            "internalId": 1,
            "isInput": False,
            "flow": {"@id": "prodA"},
            "amount": Decimal("1"),
        },
        {
            "internalId": 2,
            "isInput": False,
            "flow": {"@id": "prodB"},
            "amount": Decimal("1"),
        },
    ]
    items = [_exchange_item(e, i + 1) for i, e in enumerate(exchanges)]
    _apply_exchange_allocations(_Ent(), exchanges, items)
    # the allocated input exchange (idx 1) carries a list of allocation entries
    allocation = items[0]["allocations"]["allocation"]
    assert isinstance(allocation, list) and len(allocation) == 2  # zero cell omitted
    fractions = {
        a["@internalReferenceToCoProduct"]: a["@allocatedFraction"] for a in allocation
    }
    # co-products prodA (output idx 2) and prodB (output idx 3); fractions sum to 100.
    # Trailing fractional zeros are stripped to stay within eILCD Perc totalDigits=5
    # ("60.000" -> "60"): same value, canonical form (see _percentage fix).
    assert fractions == {"2": "60", "3": "40"}
    # the co-product outputs themselves carry no allocations
    assert "allocations" not in items[1]
    assert "allocations" not in items[2]


def test_reference_year_falls_back_to_creation_date_and_data_sources_always_emitted():
    from tidas_tools.import_lca.adapters.openlca_jsonld import _to_entity
    from tidas_tools.import_lca.writers.tidas_json import (
        _data_sources_treatment_and_representativeness,
    )

    # process with no validFrom but a creationDate, and no source refs / documentation
    item = {
        "@type": "Process",
        "@id": "d3d3d3d3-3333-4333-8333-333333333333",
        "name": "Sparse process",
        "processType": "UNIT_PROCESS",
        "processDocumentation": {"creationDate": "2021-01-07T19:02:01Z"},
        "exchanges": [
            {
                "@type": "Exchange",
                "internalId": 1,
                "isInput": False,
                "isQuantitativeReference": True,
                "amount": 1,
                "flow": {
                    "@type": "Flow",
                    "@id": "f0f0f0f0-0000-4000-8000-000000000000",
                    "name": "Product",
                    "flowType": "PRODUCT_FLOW",
                    "refUnit": "kg",
                },
                "unit": {"@id": "u0", "name": "kg"},
            },
        ],
    }
    entity = _to_entity(item, "x", {}, {})
    # referenceYear comes from creationDate, NOT the 9999 placeholder
    assert entity.raw["referenceYear"] == 2021
    # the schema-required dataSourcesTreatmentAndRepresentativeness block is always emitted,
    # even with no source refs / documentation (the format-source fallback fills it)
    block = _data_sources_treatment_and_representativeness(entity)
    assert block is not None
    assert "referenceToDataSource" in block


def test_unit_group_reference_unit_is_meanvalue_one_not_first():
    # Regression: USLCI "Units of mass*length" lists lb*mi first but t*km
    # (conversionFactor 1.0) is the real reference unit. The writer must point
    # referenceToReferenceUnit at the meanValue==1 unit, not hardcode "1".
    from tidas_tools.import_lca.model.entities import CanonicalEntity
    from tidas_tools.import_lca.writers.tidas_json import _unit_group_dataset

    entity = CanonicalEntity(
        entity_type="unitgroups",
        internal_id="838aaa21-0117-11db-92e3-0800200c9a66",
        name="Units of mass*length",
        raw={
            "units": [
                {"name": "lb*mi", "conversionFactor": "0.00072998615"},
                {"name": "t*mi", "conversionFactor": "1.609344"},
                {"name": "kg*km", "conversionFactor": "0.001"},
                {"name": "t*km", "conversionFactor": "1.0"},
                {"name": "kt*km", "conversionFactor": "1000.0"},
            ]
        },
    )
    ds = _unit_group_dataset(entity)
    qr = ds["unitGroupDataSet"]["unitGroupInformation"]["quantitativeReference"]
    units = ds["unitGroupDataSet"]["units"]["unit"]
    ref_id = qr["referenceToReferenceUnit"]
    ref_unit = next(u for u in units if u["@dataSetInternalID"] == ref_id)
    assert ref_unit["name"] == "t*km"
    assert ref_unit["meanValue"] == "1.0"


def test_unit_group_reference_unit_honors_explicit_flag():
    # When openLCA marks referenceUnit=true, that wins even if another unit's
    # factor happens to be 1.0.
    from tidas_tools.import_lca.model.entities import CanonicalEntity
    from tidas_tools.import_lca.writers.tidas_json import _unit_group_dataset

    entity = CanonicalEntity(
        entity_type="unitgroups",
        internal_id="11111111-1111-4111-8111-111111111111",
        name="Units of time",
        raw={
            "units": [
                {"name": "hr", "conversionFactor": "1.0", "referenceUnit": False},
                {"name": "a", "conversionFactor": "8760.0", "referenceUnit": True},
            ]
        },
    )
    ds = _unit_group_dataset(entity)
    qr = ds["unitGroupDataSet"]["unitGroupInformation"]["quantitativeReference"]
    units = ds["unitGroupDataSet"]["units"]["unit"]
    ref_unit = next(
        u for u in units if u["@dataSetInternalID"] == qr["referenceToReferenceUnit"]
    )
    assert ref_unit["name"] == "a"


def test_percentage_strips_trailing_zeros_to_fit_eilcd_perc():
    # Regression: allocatedFraction 100% was emitted as "100.000" (6 digits),
    # which the eILCD Perc pattern (totalDigits=5) rejects -> 6,943 schema errors
    # in a full USLCI conversion. _percentage must strip trailing fractional zeros.
    import json, re
    from tidas_tools.import_lca.writers.tidas_json import (
        _percentage,
        _allocation_fraction,
    )

    perc_pattern = json.loads(
        (
            __import__("pathlib").Path(__file__).resolve().parents[1]
            / "src/tidas_tools/tidas/schemas/tidas_data_types.json"
        ).read_text()
    )["$defs"]["Perc"]["pattern"]
    PR = re.compile(perc_pattern)

    assert _percentage("100.000") == "100"
    assert _percentage("50.0") == "50"
    assert _percentage("33.333") == "33.333"  # genuine 3-fraction, already <=5 digits
    assert _percentage("12.500") == "12.5"
    assert _allocation_fraction("1.0") == "100"  # full allocation to one co-product
    for v in ("100.000", "50.0", "33.333", "12.500"):
        out = _percentage(v)
        assert out is not None and PR.fullmatch(out), (v, out)
    for v in ("1.0", "0.5", "0.33333", "0.66667"):
        out = _allocation_fraction(v)
        assert out is not None and PR.fullmatch(out), (v, out)
