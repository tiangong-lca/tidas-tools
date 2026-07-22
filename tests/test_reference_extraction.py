import json
from pathlib import Path

from tidas_tools.import_lca.process_bundles import _collect_dependencies
from tidas_tools.reference_extraction import REFERENCE_ROLES, extract_references

FIXTURE = Path(__file__).parent / "fixtures" / "reference_extraction_v1" / "golden.json"


def test_reference_extraction_v1_golden_cases():
    fixture = json.loads(FIXTURE.read_text(encoding="utf-8"))

    for case in fixture["cases"]:
        result = extract_references(
            case["document_key"], case["category"], case["payload"]
        ).to_dict()
        if "expected" in case:
            assert result == case["expected"], case["name"]
        else:
            assert [edge["target_uuid"] for edge in result["edges"]] == case[
                "expected_edge_targets"
            ]
            assert result["issues"] == []


def test_reference_role_vocabulary_is_versioned_and_closed():
    assert REFERENCE_ROLES == (
        "lcia_factor_flow",
        "lifecycle_model_process",
        "process_exchange_flow",
        "support_document",
    )


def test_process_bundle_adapter_is_cycle_safe_and_reports_all_missing_targets(
    tmp_path,
):
    process_a = "11111111-1111-1111-1111-111111111111"
    process_b = "22222222-2222-2222-2222-222222222222"
    missing_source = "33333333-3333-3333-3333-333333333333"

    processes = tmp_path / "processes"
    processes.mkdir()
    (processes / f"{process_a}.json").write_text(
        json.dumps(
            {
                "references": [
                    _reference("process data set", process_b, "processes"),
                    _reference("source data set", missing_source, "sources"),
                ]
            }
        ),
        encoding="utf-8",
    )
    (processes / f"{process_b}.json").write_text(
        json.dumps(
            {
                "references": [
                    _reference("process data set", process_a, "processes"),
                    _reference("source data set", missing_source, "sources"),
                    {
                        "@type": "contact data set",
                        "@version": "01.00.000",
                        "@uri": "../contacts/missing.json",
                    },
                ]
            }
        ),
        encoding="utf-8",
    )
    package_index = {
        category: {}
        for category in (
            "contacts",
            "sources",
            "unitgroups",
            "flowproperties",
            "flows",
            "processes",
        )
    }
    package_index["processes"] = {
        process_a: processes / f"{process_a}.json",
        process_b: processes / f"{process_b}.json",
    }

    dependencies, unresolved = _collect_dependencies(process_a, package_index)

    assert dependencies["processes"] == {process_a, process_b}
    assert [item["ref_id"] for item in unresolved if item["ref_id"]] == [missing_source]
    assert [item["issue_code"] for item in unresolved if item.get("issue_code")] == [
        "reference_object_id_missing"
    ]


def _reference(ref_type: str, ref_id: str, category: str) -> dict:
    return {
        "@type": ref_type,
        "@refObjectId": ref_id,
        "@version": "01.00.000",
        "@uri": f"../{category}/{ref_id}.json",
    }
