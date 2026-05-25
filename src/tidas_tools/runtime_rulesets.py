from __future__ import annotations

import argparse
import json
from pathlib import Path
from typing import Any

METHODOLOGIES_DIR = Path(__file__).resolve().parent / "tidas" / "methodologies"
RUNTIME_RULESETS_FILE = METHODOLOGIES_DIR / "runtime_rulesets.json"

REQUIRED_RULESET_FIELDS = {"id", "version", "dataset_type", "phase", "rule_ids"}
REQUIRED_RULE_FIELDS = {
    "id",
    "dataset_type",
    "summary",
    "severity",
    "phases",
    "default_blocker",
    "field_paths",
    "source_rule_refs",
}
VALID_SEVERITIES = {"info", "warning", "blocker"}


def load_runtime_rulesets(path: str | Path | None = None) -> dict[str, Any]:
    ruleset_path = Path(path) if path is not None else RUNTIME_RULESETS_FILE
    return json.loads(ruleset_path.read_text(encoding="utf-8"))


def validate_runtime_rulesets(metadata: dict[str, Any]) -> list[str]:
    errors: list[str] = []
    rulesets = metadata.get("rulesets", [])
    rules = metadata.get("rules", [])

    if not isinstance(rulesets, list) or not rulesets:
        errors.append("rulesets must be a non-empty array")
    if not isinstance(rules, list) or not rules:
        errors.append("rules must be a non-empty array")
        return errors

    rule_ids: set[str] = set()
    for index, rule in enumerate(rules):
        missing = sorted(REQUIRED_RULE_FIELDS - set(rule))
        if missing:
            errors.append(f"rules[{index}] missing fields: {', '.join(missing)}")
            continue
        rule_id = str(rule["id"])
        if rule_id in rule_ids:
            errors.append(f"duplicate rule id: {rule_id}")
        rule_ids.add(rule_id)
        if rule.get("severity") not in VALID_SEVERITIES:
            errors.append(f"{rule_id} has invalid severity: {rule.get('severity')}")
        if not isinstance(rule.get("phases"), list) or not rule["phases"]:
            errors.append(f"{rule_id} phases must be a non-empty array")
        if not isinstance(rule.get("field_paths"), list) or not rule["field_paths"]:
            errors.append(f"{rule_id} field_paths must be a non-empty array")
        if (
            not isinstance(rule.get("source_rule_refs"), list)
            or not rule["source_rule_refs"]
        ):
            errors.append(f"{rule_id} source_rule_refs must be a non-empty array")

    ruleset_ids: set[str] = set()
    for index, ruleset in enumerate(rulesets):
        missing = sorted(REQUIRED_RULESET_FIELDS - set(ruleset))
        if missing:
            errors.append(f"rulesets[{index}] missing fields: {', '.join(missing)}")
            continue
        ruleset_id = str(ruleset["id"])
        if ruleset_id in ruleset_ids:
            errors.append(f"duplicate ruleset id: {ruleset_id}")
        ruleset_ids.add(ruleset_id)
        unknown_ids = sorted(set(ruleset["rule_ids"]) - rule_ids)
        if unknown_ids:
            errors.append(
                f"{ruleset_id} references unknown rule ids: {', '.join(unknown_ids)}"
            )

    return errors


def rules_for_ruleset(
    metadata: dict[str, Any], ruleset_id: str
) -> list[dict[str, Any]]:
    rules_by_id = {rule["id"]: rule for rule in metadata.get("rules", [])}
    for ruleset in metadata.get("rulesets", []):
        if ruleset.get("id") == ruleset_id:
            return [rules_by_id[rule_id] for rule_id in ruleset.get("rule_ids", [])]
    raise KeyError(f"Unknown ruleset id: {ruleset_id}")


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(
        description="Inspect packaged TIDAS runtime rulesets."
    )
    parser.add_argument("--ruleset", help="Print only the rules for one ruleset id.")
    parser.add_argument(
        "--validate", action="store_true", help="Validate the packaged metadata."
    )
    args = parser.parse_args(argv)

    metadata = load_runtime_rulesets()
    if args.validate:
        errors = validate_runtime_rulesets(metadata)
        print(json.dumps({"ok": not errors, "errors": errors}, indent=2))
        return 1 if errors else 0

    payload: Any = metadata
    if args.ruleset:
        payload = {
            "schema_version": metadata.get("schema_version"),
            "ruleset": args.ruleset,
            "rules": rules_for_ruleset(metadata, args.ruleset),
        }
    print(json.dumps(payload, indent=2, ensure_ascii=False))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
