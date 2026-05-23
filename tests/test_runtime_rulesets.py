import json

from jsonschema import Draft202012Validator

from tidas_tools.runtime_rulesets import (
    METHODOLOGIES_DIR,
    load_runtime_rulesets,
    rules_for_ruleset,
    validate_runtime_rulesets,
)


def test_runtime_rulesets_validate_against_schema():
    metadata = load_runtime_rulesets()
    schema = json.loads(
        (METHODOLOGIES_DIR / "runtime_rulesets.schema.json").read_text(encoding="utf-8")
    )

    errors = list(Draft202012Validator(schema).iter_errors(metadata))

    assert errors == []
    assert validate_runtime_rulesets(metadata) == []


def test_runtime_rulesets_cover_required_automated_lca_profiles():
    metadata = load_runtime_rulesets()
    rulesets = {ruleset["id"]: ruleset for ruleset in metadata["rulesets"]}

    for ruleset_id in [
        "process-authoring/strict",
        "process-authoring/repair",
        "process-publish/default",
        "process-dedup/default",
        "flow-authoring/strict",
        "flow-publish/default",
        "flow-dedup/default",
    ]:
        assert ruleset_id in rulesets
        assert rulesets[ruleset_id]["version"] == "1"
        assert rulesets[ruleset_id]["rule_ids"]


def test_runtime_rulesets_include_process_authoring_and_evidence_rules():
    metadata = load_runtime_rulesets()
    strict_process_rules = rules_for_ruleset(metadata, "process-authoring/strict")
    rule_ids = {rule["id"] for rule in strict_process_rules}

    assert "tidas.process.name.base-name.align-reference-flow" in rule_ids
    assert "tidas.process.quantitative-reference.required" in rule_ids
    assert "tidas.process.exchange.amount.required" in rule_ids
    assert "tidas.process.evidence.field-bindings.required" in rule_ids
    assert all(
        rule["default_blocker"]
        for rule in strict_process_rules
        if rule["severity"] == "blocker"
    )


def test_runtime_rulesets_include_flow_property_and_unit_rules():
    metadata = load_runtime_rulesets()
    flow_rules = rules_for_ruleset(metadata, "flow-publish/default")
    rule_ids = {rule["id"] for rule in flow_rules}

    assert "tidas.flow.type.required" in rule_ids
    assert "tidas.flow.reference-property-unit.required" in rule_ids
    assert "tidas.flow.flow-property.mean-value.positive" in rule_ids
    assert "tidas.flow.evidence.field-bindings.required" in rule_ids


def test_runtime_rulesets_reject_unknown_profile():
    metadata = load_runtime_rulesets()

    try:
        rules_for_ruleset(metadata, "missing/default")
    except KeyError as error:
        assert "missing/default" in str(error)
    else:
        raise AssertionError("missing/default should not resolve")
