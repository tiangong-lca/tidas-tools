"""ILCD-alignment guards for the TIDAS schemas (audit fault-line 1).

These lock in the tightenings that keep "valid under TIDAS => valid under ILCD":
shared Version/@type enums, anchored Real, eILCD Perc, LCIA-specific review
enums, boolean normalisation/weighting, two-letter HK/MO/TW, and the
lifecycle-model @version / integer reference-process fixes. They also assert the
EN and ZH schema mirrors stay structurally identical.
"""

import json
from pathlib import Path

EN = Path(__file__).resolve().parents[1] / "src/tidas_tools/tidas/schemas"
ZH = Path(__file__).resolve().parents[1] / "src/tidas_tools/tidas/schemas_zh"


def _load(root, name):
    return json.loads((root / name).read_text(encoding="utf-8"))


def _find_all(node, key):
    if isinstance(node, dict):
        if key in node:
            yield node[key]
        for value in node.values():
            yield from _find_all(value, key)
    elif isinstance(node, list):
        for item in node:
            yield from _find_all(item, key)


def _strip_descriptions(node):
    """Structural view of a schema: drop description text so EN/ZH compare equal."""
    if isinstance(node, dict):
        return {
            k: _strip_descriptions(v) for k, v in node.items() if k != "description"
        }
    if isinstance(node, list):
        return [_strip_descriptions(v) for v in node]
    return node


# --- shared data types ----------------------------------------------------


def test_global_reference_type_values_enum():
    defs = _load(EN, "tidas_data_types.json")["$defs"]
    assert defs["GlobalReferenceTypeValues"]["enum"] == [
        "source data set",
        "process data set",
        "flow data set",
        "flow property data set",
        "unit group data set",
        "contact data set",
        "LCIA method data set",
        "other external file",
    ]
    gr = defs["GlobalReferenceType"]["anyOf"]
    assert gr[0]["properties"]["@type"] == {"$ref": "#/$defs/GlobalReferenceTypeValues"}
    assert gr[1]["items"]["properties"]["@type"] == {
        "$ref": "#/$defs/GlobalReferenceTypeValues"
    }


def test_version_def_and_references():
    defs = _load(EN, "tidas_data_types.json")["$defs"]
    assert defs["Version"]["pattern"] == r"^\d{2}\.\d{2}(\.\d{3})?$"
    gr = defs["GlobalReferenceType"]["anyOf"]
    assert gr[0]["properties"]["@version"] == {"$ref": "#/$defs/Version"}
    assert gr[1]["items"]["properties"]["@version"] == {"$ref": "#/$defs/Version"}


def test_dataset_version_references_version_everywhere():
    for f in EN.glob("tidas_*.json"):
        text = f.read_text(encoding="utf-8")
        if '"common:dataSetVersion"' in text:
            assert "tidas_data_types.json#/$defs/Version" in text, f.name


def test_real_pattern_is_anchored():
    real = _load(EN, "tidas_data_types.json")["$defs"]["Real"]
    assert real["pattern"].startswith("^") and real["pattern"].endswith("$")


def test_perc_allows_over_100_and_caps_total_digits():
    import re

    perc = _load(EN, "tidas_data_types.json")["$defs"]["Perc"]["pattern"]
    rx = re.compile(perc)
    assert rx.fullmatch("150.5")  # >100 now allowed (matches eILCD decimal)
    assert rx.fullmatch("0.001")
    assert not rx.fullmatch("123.456")  # 6 total digits > totalDigits=5
    assert not rx.fullmatch("abc")


# --- lciamethods ----------------------------------------------------------


def test_lciamethods_normalisation_weighting_boolean():
    lm = _load(EN, "tidas_lciamethods.json")
    assert next(_find_all(lm, "normalisation"))["type"] == "boolean"
    assert next(_find_all(lm, "weighting"))["type"] == "boolean"
    # copyright stays a string enum (not in scope of the fix)
    assert next(_find_all(lm, "common:copyright"))["type"] == "string"


def test_lciamethods_uncertainty_field_renamed():
    text = (EN / "tidas_lciamethods.json").read_text(encoding="utf-8")
    assert '"uncertaintyType"' not in text
    assert '"uncertaintyDistributionType"' in text


def test_lciamethods_review_uses_lcia_specific_enums():
    lm = _load(EN, "tidas_lciamethods.json")
    scopes = next(_find_all(lm, "common:scope"))
    scope_enum = next(_find_all(scopes, "enum"))
    assert "Characterisation factors" in scope_enum
    assert "Raw data" not in scope_enum  # process-review value gone
    methods = next(_find_all(lm, "common:method"))
    method_enum = next(_find_all(methods, "enum"))
    assert "Cross-check with other LCIA method(ology)" in method_enum
    assert "Energy balance" not in method_enum


# --- locations ------------------------------------------------------------


def test_hk_mo_tw_two_letter_codes():
    codes = [o["const"] for o in _load(EN, "tidas_locations_category.json")["oneOf"]]
    for two in ("HK", "MO", "TW"):
        assert two in codes
    for cn in ("CN-HK", "CN-MO", "CN-TW"):
        assert cn not in codes
    assert "RoW" in codes


# --- lifecyclemodels ------------------------------------------------------


def test_reference_to_reference_process_is_integer():
    lcm = _load(EN, "tidas_lifecyclemodels.json")
    rrp = next(_find_all(lcm, "referenceToReferenceProcess"))
    assert rrp["type"] == "integer"
    assert "pattern" not in rrp


def test_lifecycle_output_and_downstream_require_version():
    lcm = _load(EN, "tidas_lifecyclemodels.json")
    seen_output = seen_downstream = False
    for node in _find_all(lcm, "properties"):
        if "@flowUUID" in node and "downstreamProcess" in node:
            assert "@version" in node, "outputExchange missing @version property"
            seen_output = True
        if "@id" in node and "@flowUUID" in node and "@location" in node:
            assert "@version" in node, "downstreamProcess missing @version property"
            seen_downstream = True
    assert seen_output and seen_downstream


# --- EN/ZH parity ---------------------------------------------------------


def test_en_zh_structural_parity_for_touched_schemas():
    for name in (
        "tidas_data_types.json",
        "tidas_lciamethods.json",
        "tidas_lifecyclemodels.json",
        "tidas_locations_category.json",
    ):
        en = _strip_descriptions(_load(EN, name))
        zh = _strip_descriptions(_load(ZH, name))
        assert en == zh, f"EN/ZH structural drift in {name}"
