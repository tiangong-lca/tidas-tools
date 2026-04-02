import json

from tidas_tools.package_versions import normalize_package_versions


def _write_json(path, payload):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(payload, indent=2, ensure_ascii=False) + "\n")


def test_normalize_package_versions_updates_references_and_removes_old_versions(
    tmp_path,
):
    package_dir = tmp_path / "package"

    contact_uuid = "11111111-1111-1111-1111-111111111111"
    flow_uuid = "22222222-2222-2222-2222-222222222222"
    process_uuid = "33333333-3333-3333-3333-333333333333"

    _write_json(
        package_dir / "contacts" / f"{contact_uuid}_01.00.005.json",
        {"contactDataSet": {"common:UUID": contact_uuid}},
    )
    _write_json(
        package_dir / "contacts" / f"{contact_uuid}_01.00.006.json",
        {"contactDataSet": {"common:UUID": contact_uuid}},
    )
    _write_json(
        package_dir / "flows" / f"{flow_uuid}_01.01.000.json",
        {"flowDataSet": {"common:UUID": flow_uuid}},
    )
    _write_json(
        package_dir / "flows" / f"{flow_uuid}_01.01.001.json",
        {
            "flowDataSet": {
                "administrativeInformation": {
                    "publicationAndOwnership": {
                        "common:referenceToOwnershipOfDataSet": {
                            "@refObjectId": contact_uuid,
                            "@type": "contact data set",
                            "@version": "01.00.005",
                            "@uri": f"../contacts/{contact_uuid}_01.00.005.xml",
                        },
                        "common:referenceToPrecedingDataSetVersion": {
                            "@refObjectId": flow_uuid,
                            "@type": "flow data set",
                            "@version": "01.01.000",
                            "@uri": f"../flows/{flow_uuid}_01.01.000.xml",
                        },
                    }
                }
            }
        },
    )
    _write_json(
        package_dir / "processes" / f"{process_uuid}_01.01.000.json",
        {
            "processDataSet": {
                "exchanges": {
                    "exchange": [
                        {
                            "referenceToFlowDataSet": {
                                "@refObjectId": flow_uuid,
                                "@type": "flow data set",
                                "@version": "01.01.000",
                                "@uri": f"../flows/{flow_uuid}_01.01.000.xml",
                            }
                        }
                    ]
                }
            }
        },
    )

    summary = normalize_package_versions(package_dir)

    assert summary["duplicate_dataset_count"] == 2
    assert summary["removed_records"] == 2
    assert summary["updated_references"] == 2
    assert summary["removed_preceding_references"] == 1

    assert not (package_dir / "contacts" / f"{contact_uuid}_01.00.005.json").exists()
    assert not (package_dir / "flows" / f"{flow_uuid}_01.01.000.json").exists()
    assert (package_dir / "contacts" / f"{contact_uuid}_01.00.006.json").exists()
    assert (package_dir / "flows" / f"{flow_uuid}_01.01.001.json").exists()

    latest_flow = json.loads(
        (package_dir / "flows" / f"{flow_uuid}_01.01.001.json").read_text()
    )
    ownership_ref = latest_flow["flowDataSet"]["administrativeInformation"][
        "publicationAndOwnership"
    ]["common:referenceToOwnershipOfDataSet"]
    assert ownership_ref["@version"] == "01.00.006"
    assert ownership_ref["@uri"] == f"../contacts/{contact_uuid}_01.00.006.xml"
    assert (
        "common:referenceToPrecedingDataSetVersion"
        not in latest_flow["flowDataSet"]["administrativeInformation"][
            "publicationAndOwnership"
        ]
    )

    kept_process = json.loads(
        (package_dir / "processes" / f"{process_uuid}_01.01.000.json").read_text()
    )
    flow_ref = kept_process["processDataSet"]["exchanges"]["exchange"][0][
        "referenceToFlowDataSet"
    ]
    assert flow_ref["@version"] == "01.01.001"
    assert flow_ref["@uri"] == f"../flows/{flow_uuid}_01.01.001.xml"
