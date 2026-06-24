"""Writer placeholder / conversion-gap behaviour (import fault-line 2 fixes).

Covers: RoW location handling, deterministic placeholder timestamps, and the
machine-distinguishable conversion-gap markers for placeholder classifications.
"""

import json

from tidas_tools.import_lca.writers import tidas_json as W


def _contains(node, target) -> bool:
    if node == target:
        return True
    if isinstance(node, dict):
        return any(_contains(value, target) for value in node.values())
    if isinstance(node, list):
        return any(_contains(item, target) for item in node)
    return False


# --- A3: RoW / location ---------------------------------------------------


def test_row_resolves_to_itself_not_glo():
    assert W._location_code("RoW") == "RoW"
    assert W._location_gap("RoW") is None


def test_unknown_location_collapses_to_glo_with_gap():
    assert W._location_code("Mars") == "GLO"
    gap = W._location_gap("Mars")
    assert gap is not None
    assert gap["status"] == "placeholder-location"
    assert "Mars" in gap["detail"]


def test_known_and_empty_location_have_no_gap():
    assert W._location_gap("US") is None
    assert W._location_gap("") is None
    assert W._location_gap(None) is None


# --- B-time: deterministic placeholder timestamp -------------------------


def test_timestamp_placeholder_is_deterministic_and_distinguishable():
    first = W._admin_info(category="flows", dataset_id="x")
    second = W._admin_info(category="flows", dataset_id="x")
    # No wall-clock now(): identical across calls (idempotent re-import).
    assert first == second
    assert _contains(first, W.PLACEHOLDER_TIMESTAMP)


def test_real_source_timestamp_is_preserved():
    info = W._admin_info(
        category="flows", dataset_id="x", timestamp="2020-05-01T00:00:00Z"
    )
    assert _contains(info, "2020-05-01T00:00:00Z")
    assert not _contains(info, W.PLACEHOLDER_TIMESTAMP)


# --- B-class: distinguishable classification placeholders ----------------


def test_product_classification_placeholder_marked_and_preserves_source():
    raw = {"sourceCategoryPath": ["Technosphere", "Metals"], "category": "steel"}
    block = W._flow_classification("Product flow", raw)
    # The schema-required slot is still a valid (placeholder) CPC chain.
    assert block["common:classification"]["common:class"][-1]["@classId"] == "94900"
    other = block["common:classification"]["common:other"]
    # The real source taxonomy is preserved, not overwritten.
    assert other["tidasimport:sourceTrace"]["payload"]["sourceCategoryPath"] == [
        "Technosphere",
        "Metals",
    ]
    # The placeholder is machine-distinguishable from a real classification.
    gap = other["tidasimport:conversionGap"]
    assert gap["@marker"] == W.IMPORT_GAP_MARKER
    assert gap["status"] == "unclassified-pending"


def test_process_classification_placeholder_marked():
    block = W._default_process_classification({"category": "energy systems"})
    gap = block["common:classification"]["common:other"]["tidasimport:conversionGap"]
    assert gap["status"] == "unclassified-pending"


def test_mapped_elementary_compartment_has_no_gap():
    # A recognisable FEDEFL compartment path maps to a real category -> no gap.
    block = W._flow_classification("Elementary flow", {"category": "emission/air"})
    other = block["common:elementaryFlowCategorization"].get("common:other")
    if other is not None:
        assert "tidasimport:conversionGap" not in other


def test_missing_compartment_falls_back_with_gap():
    block = W._flow_classification("Elementary flow", {})
    other = block["common:elementaryFlowCategorization"]["common:other"]
    assert other["tidasimport:conversionGap"]["status"] == "placeholder-compartment"


# --- B3: scanner ----------------------------------------------------------


def test_contact_fields_emitted_in_ilcd_sequence_order():
    # ILCD ContactDataSet xs:sequence: contactAddress, telephone, telefax,
    # email, WWWAddress, ..., contactDescriptionOrComment.
    payload = W._contact_payload(
        contact_id="11111111-1111-4111-8111-111111111111",
        name="Name",
        short_name="Short",
        email="a@b.com",
        telephone="123",
        website="http://example.org",
        address="1 Test Street",
        description="A description",
    )
    info = payload["contactDataSet"]["contactInformation"]["dataSetInformation"]
    ordered = [
        k
        for k in info
        if k
        in (
            "contactAddress",
            "telephone",
            "email",
            "WWWAddress",
            "contactDescriptionOrComment",
        )
    ]
    assert ordered == [
        "contactAddress",
        "telephone",
        "email",
        "WWWAddress",
        "contactDescriptionOrComment",
    ]


def test_scan_conversion_gaps_collects_markers(tmp_path):
    flows = tmp_path / "flows"
    flows.mkdir()
    flow = {
        "flowDataSet": {
            "flowInformation": {
                "dataSetInformation": {
                    "classificationInformation": W._flow_classification(
                        "Product flow", {"category": "steel"}
                    )
                }
            }
        }
    }
    (flows / "f1.json").write_text(json.dumps(flow), encoding="utf-8")

    processes = tmp_path / "processes"
    processes.mkdir()
    process = {
        "administrativeInformation": W._admin_info(
            category="processes", dataset_id="p1"
        )
    }
    (processes / "p1.json").write_text(json.dumps(process), encoding="utf-8")

    gaps = W.scan_conversion_gaps(tmp_path)
    statuses = {gap["status"] for gap in gaps}
    assert "unclassified-pending" in statuses
    assert "placeholder-timestamp" in statuses
    assert all("file" in gap for gap in gaps)
