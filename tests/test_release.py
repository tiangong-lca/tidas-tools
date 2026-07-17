import hashlib
import json
from pathlib import Path
from xml.etree import ElementTree
import zipfile

import pytest

from tidas_tools.release import (
    RESULT_PROFILE,
    UNIT_PROFILE,
    ReleaseToolError,
    build_release_packages,
    convert_tidas_to_ilcd,
    deterministic_zip,
    load_dataset_index,
    resolve_profile_closure,
    semantic_roundtrip_report,
)

UNIT_ID = "11111111-1111-4111-8111-111111111111"
FLOW_ID = "22222222-2222-4222-8222-222222222222"
METHOD_ID = "44444444-4444-4444-8444-444444444444"
MODEL_ID = "c58f567c-c631-5a3a-90d9-c0cec7290cf8"
RESULT_ID = "ba3386d3-39c0-5a48-ae4d-e7ad90ec4996"
VERSION = "01.00.000"


def _reference(dataset_type, dataset_id, category):
    return {
        "@type": dataset_type,
        "@refObjectId": dataset_id,
        "@version": VERSION,
        "@uri": f"../{category}/{dataset_id}_{VERSION}.json",
    }


def _write_dataset(root, relative_path, document, dataset_type, role, dataset_id):
    file_path = root / relative_path
    file_path.parent.mkdir(parents=True, exist_ok=True)
    body = json.dumps(document, ensure_ascii=False, indent=2) + "\n"
    file_path.write_text(body, encoding="utf-8")
    return {
        "datasetType": dataset_type,
        "role": role,
        "uuid": dataset_id,
        "version": VERSION,
        "path": relative_path,
        "sha256": hashlib.sha256(body.encode()).hexdigest(),
        "byteSize": len(body.encode()),
        "canonicalContentHash": hashlib.sha256(
            json.dumps(document, sort_keys=True, separators=(",", ":")).encode()
        ).hexdigest(),
    }


def release_tree(tmp_path):
    root = tmp_path / "tidas"
    entries = [
        _write_dataset(
            root,
            f"flows/{FLOW_ID}_{VERSION}.json",
            {
                "flowDataSet": {
                    "flowInformation": {"dataSetInformation": {"common:UUID": FLOW_ID}}
                }
            },
            "flow",
            "support",
            FLOW_ID,
        ),
        _write_dataset(
            root,
            f"lifecyclemodels/{MODEL_ID}_{VERSION}.json",
            {
                "lifeCycleModelDataSet": {
                    "lifeCycleModelInformation": {
                        "dataSetInformation": {
                            "common:UUID": MODEL_ID,
                            "referenceToResultingProcess": _reference(
                                "process data set", RESULT_ID, "processes"
                            ),
                        },
                        "technology": {
                            "processes": {
                                "processInstance": {
                                    "referenceToProcess": _reference(
                                        "process data set", UNIT_ID, "processes"
                                    )
                                }
                            }
                        },
                    }
                }
            },
            "lifecyclemodel",
            "lifecycle_model",
            MODEL_ID,
        ),
        _write_dataset(
            root,
            f"processes/{RESULT_ID}_{VERSION}.json",
            {
                "processDataSet": {
                    "processInformation": {
                        "dataSetInformation": {"common:UUID": RESULT_ID}
                    },
                    "exchanges": {
                        "exchange": {
                            "referenceToFlowDataSet": _reference(
                                "flow data set", FLOW_ID, "flows"
                            )
                        }
                    },
                }
            },
            "process",
            "result_process",
            RESULT_ID,
        ),
        _write_dataset(
            root,
            f"processes/{UNIT_ID}_{VERSION}.json",
            {
                "processDataSet": {
                    "processInformation": {
                        "dataSetInformation": {"common:UUID": UNIT_ID}
                    },
                    "exchanges": {
                        "exchange": {
                            "referenceToFlowDataSet": _reference(
                                "flow data set", FLOW_ID, "flows"
                            )
                        }
                    },
                }
            },
            "process",
            "unit_process",
            UNIT_ID,
        ),
    ]
    entries.sort(key=lambda item: item["path"])
    index_path = tmp_path / "canonical-dataset-index.json"
    index_path.write_text(
        json.dumps(
            {
                "schemaVersion": "tiangong.release.canonical-dataset-index.v1",
                "datasetCount": len(entries),
                "byteSize": sum(item["byteSize"] for item in entries),
                "artifactSetHash": "0" * 64,
                "datasets": entries,
            },
            indent=2,
        )
        + "\n",
        encoding="utf-8",
    )
    return root, index_path


def test_profile_closure_and_four_packages_are_self_contained_and_deterministic(
    tmp_path,
):
    root, index_path = release_tree(tmp_path)
    entries = load_dataset_index(index_path, root)
    unit, unit_report = resolve_profile_closure(
        input_dir=root, entries=entries, profile_id=UNIT_PROFILE
    )
    result, result_report = resolve_profile_closure(
        input_dir=root, entries=entries, profile_id=RESULT_PROFILE
    )

    assert {entry.key for entry in unit} == {
        f"flow:{FLOW_ID}:{VERSION}",
        f"process:{UNIT_ID}:{VERSION}",
    }
    assert {entry.key for entry in unit}.issubset({entry.key for entry in result})
    assert unit_report["status"] == result_report["status"] == "passed"

    ilcd_dir = tmp_path / "ilcd"
    conversion = convert_tidas_to_ilcd(root, ilcd_dir)
    assert conversion["status"] == "passed"
    assert semantic_roundtrip_report(root, ilcd_dir)["status"] == "passed"

    first = build_release_packages(
        tidas_dir=root,
        ilcd_dir=ilcd_dir,
        dataset_index=index_path,
        output_dir=tmp_path / "packages-a",
    )
    second = build_release_packages(
        tidas_dir=root,
        ilcd_dir=ilcd_dir,
        dataset_index=index_path,
        output_dir=tmp_path / "packages-b",
    )
    assert first["status"] == "passed"
    assert len(first["packages"]) == 4
    assert [item["artifact"]["sha256"] for item in first["packages"]] == [
        item["artifact"]["sha256"] for item in second["packages"]
    ]

    unit_zip = next(
        item
        for item in first["packages"]
        if item["profileId"] == UNIT_PROFILE and item["format"] == "tidas"
    )
    with zipfile.ZipFile(unit_zip["artifact"]["path"]) as archive:
        assert archive.namelist() == sorted(archive.namelist())
        assert all(
            info.date_time == (1980, 1, 1, 0, 0, 0) for info in archive.infolist()
        )
        assert all(
            (info.external_attr >> 16) & 0o777 == 0o644 for info in archive.infolist()
        )


def _child_names(element):
    return [child.tag.rsplit("}", 1)[-1] for child in element]


def test_conversion_restores_nested_ilcd_schema_order_from_canonical_json(tmp_path):
    root = tmp_path / "tidas"
    process_path = root / f"processes/{RESULT_ID}_{VERSION}.json"
    process_path.parent.mkdir(parents=True)
    process_path.write_text(
        json.dumps(
            {
                "processDataSet": {
                    "LCIAResults": {"LCIAResult": {"meanAmount": "1"}},
                    "exchanges": {
                        "exchange": [
                            {
                                "dataDerivationTypeStatus": "Calculated",
                                "resultingAmount": "1",
                                "meanAmount": "1",
                                "exchangeDirection": "Output",
                                "referenceToFlowDataSet": _reference(
                                    "flow data set", FLOW_ID, "flows"
                                ),
                                "@dataSetInternalID": "0",
                            }
                        ]
                    },
                    "administrativeInformation": {},
                    "modellingAndValidation": {
                        "complianceDeclarations": {},
                        "validation": {},
                        "LCIMethodAndAllocation": {},
                    },
                    "processInformation": {
                        "geography": {},
                        "quantitativeReference": {},
                        "dataSetInformation": {
                            "common:generalComment": "result",
                            "classificationInformation": {},
                            "name": {},
                            "common:UUID": RESULT_ID,
                        },
                    },
                    "@xsi:schemaLocation": "process schema",
                    "@locations": "../ILCDLocations.xml",
                    "@version": "1.1",
                    "@xmlns:xsi": "http://www.w3.org/2001/XMLSchema-instance",
                    "@xmlns": "http://lca.jrc.it/ILCD/Process",
                    "@xmlns:common": "http://lca.jrc.it/ILCD/Common",
                }
            },
            sort_keys=True,
        ),
        encoding="utf-8",
    )
    model_path = root / f"lifecyclemodels/{MODEL_ID}_{VERSION}.json"
    model_path.parent.mkdir(parents=True)
    model_path.write_text(
        json.dumps(
            {
                "lifeCycleModelDataSet": {
                    "administrativeInformation": {},
                    "modellingAndValidation": {},
                    "lifeCycleModelInformation": {
                        "technology": {},
                        "quantitativeReference": {},
                        "dataSetInformation": {"common:UUID": MODEL_ID},
                    },
                    "@xsi:schemaLocation": "model schema",
                    "@locations": "../ILCDLocations.xml",
                    "@version": "1.1",
                    "@xmlns:xsi": "http://www.w3.org/2001/XMLSchema-instance",
                    "@xmlns": "http://eplca.jrc.ec.europa.eu/ILCD/LifeCycleModel/2017",
                    "@xmlns:common": "http://lca.jrc.it/ILCD/Common",
                }
            },
            sort_keys=True,
        ),
        encoding="utf-8",
    )
    lcia_path = root / f"lciamethods/{METHOD_ID}_{VERSION}.json"
    lcia_path.parent.mkdir(parents=True)
    lcia_path.write_text(
        json.dumps(
            {
                "LCIAMethodDataSet": {
                    "characterisationFactors": {
                        "factor": {
                            "deviatingRecommendation": "Level I",
                            "meanValue": "1",
                            "exchangeDirection": "Output",
                            "referenceToFlowDataSet": _reference(
                                "flow data set", FLOW_ID, "flows"
                            ),
                        }
                    },
                    "administrativeInformation": {},
                    "modellingAndValidation": {},
                    "LCIAMethodInformation": {
                        "impactModel": {"modelDescription": "fixture"},
                        "geography": {"impactLocation": "GLO"},
                        "time": {"duration": "time independent"},
                        "quantitativeReference": {},
                        "dataSetInformation": {"common:UUID": METHOD_ID},
                    },
                    "@xsi:schemaLocation": "lcia method schema",
                    "@version": "1.1",
                    "@xmlns:xsi": "http://www.w3.org/2001/XMLSchema-instance",
                    "@xmlns": "http://lca.jrc.it/ILCD/LCIAMethod",
                    "@xmlns:common": "http://lca.jrc.it/ILCD/Common",
                }
            },
            sort_keys=True,
        ),
        encoding="utf-8",
    )

    ilcd_dir = tmp_path / "ilcd"
    report = convert_tidas_to_ilcd(root, ilcd_dir)

    assert report["status"] == "passed"
    process_root = ElementTree.parse(
        ilcd_dir / "data" / process_path.relative_to(root).with_suffix(".xml")
    ).getroot()
    assert _child_names(process_root) == [
        "processInformation",
        "modellingAndValidation",
        "administrativeInformation",
        "exchanges",
        "LCIAResults",
    ]
    process_information = process_root[0]
    assert _child_names(process_information) == [
        "dataSetInformation",
        "quantitativeReference",
        "geography",
    ]
    assert _child_names(process_information[0]) == [
        "UUID",
        "name",
        "classificationInformation",
        "generalComment",
    ]
    assert _child_names(process_root[1]) == [
        "LCIMethodAndAllocation",
        "validation",
        "complianceDeclarations",
    ]
    assert _child_names(process_root[3][0]) == [
        "referenceToFlowDataSet",
        "exchangeDirection",
        "meanAmount",
        "resultingAmount",
        "dataDerivationTypeStatus",
    ]
    assert process_root[3][0][0].attrib["uri"].endswith(f"{FLOW_ID}_{VERSION}.xml")

    model_root = ElementTree.parse(
        ilcd_dir / "data" / model_path.relative_to(root).with_suffix(".xml")
    ).getroot()
    assert _child_names(model_root) == [
        "lifeCycleModelInformation",
        "modellingAndValidation",
        "administrativeInformation",
    ]
    assert _child_names(model_root[0]) == [
        "dataSetInformation",
        "quantitativeReference",
        "technology",
    ]
    lcia_root = ElementTree.parse(
        ilcd_dir / "data" / lcia_path.relative_to(root).with_suffix(".xml")
    ).getroot()
    assert _child_names(lcia_root) == [
        "LCIAMethodInformation",
        "modellingAndValidation",
        "administrativeInformation",
        "characterisationFactors",
    ]
    assert _child_names(lcia_root[0]) == [
        "dataSetInformation",
        "quantitativeReference",
        "time",
        "geography",
        "impactModel",
    ]
    assert _child_names(lcia_root[3][0]) == [
        "referenceToFlowDataSet",
        "exchangeDirection",
        "meanValue",
        "deviatingRecommendation",
    ]
    assert lcia_root[3][0][0].attrib["uri"].endswith(f"{FLOW_ID}_{VERSION}.xml")
    assert semantic_roundtrip_report(root, ilcd_dir)["status"] == "passed"


def test_closure_fails_closed_for_missing_or_inexact_references(tmp_path):
    root, index_path = release_tree(tmp_path)
    unit_path = root / f"processes/{UNIT_ID}_{VERSION}.json"
    document = json.loads(unit_path.read_text())
    reference = document["processDataSet"]["exchanges"]["exchange"][
        "referenceToFlowDataSet"
    ]
    reference.pop("@version")
    body = json.dumps(document, indent=2) + "\n"
    unit_path.write_text(body)

    index = json.loads(index_path.read_text())
    unit_entry = next(item for item in index["datasets"] if item["uuid"] == UNIT_ID)
    unit_entry["sha256"] = hashlib.sha256(body.encode()).hexdigest()
    index_path.write_text(json.dumps(index, indent=2) + "\n")
    entries = load_dataset_index(index_path, root)
    with pytest.raises(ReleaseToolError, match="without @version") as error:
        resolve_profile_closure(
            input_dir=root, entries=entries, profile_id=UNIT_PROFILE
        )
    assert error.value.code == "reference_version_missing"


def test_deterministic_zip_orders_archive_names_and_rejects_unsafe_paths(tmp_path):
    first = tmp_path / "z-source.json"
    second = tmp_path / "a-source.json"
    first.write_text("first", encoding="utf-8")
    second.write_text("second", encoding="utf-8")
    output = tmp_path / "ordered.zip"

    deterministic_zip(output, [(first, "a/member.json"), (second, "z/member.json")])
    with zipfile.ZipFile(output) as archive:
        assert archive.namelist() == ["a/member.json", "z/member.json"]

    with pytest.raises(ReleaseToolError) as error:
        deterministic_zip(output, [(first, "../escape.json")])
    assert error.value.code == "zip_member_path_invalid"
