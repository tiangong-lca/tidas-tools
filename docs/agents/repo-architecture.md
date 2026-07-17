---
title: tidas-tools Architecture Notes
docType: guide
scope: repo
status: active
authoritative: false
owner: tidas-tools
language: en
whenToUse:
  - when you need a compact mental model of the repo before editing tooling logic, asset trees, or dispatch automation
  - when deciding which CLI or asset family owns a behavior change
  - when conversion, validation, export, or downstream SDK dispatch is mentioned without exact paths
whenToUpdate:
  - when major tool families or asset locations change
  - when downstream dispatch or release architecture changes
  - when stable versus packaged paths move
checkPaths:
  - docs/agents/repo-architecture.md
  - .docpact/config.yaml
  - pyproject.toml
  - src/tidas_tools/**
  - .github/workflows/**
  - .githooks/pre-push
  - scripts/docpact
  - scripts/schema_lock.py
  - scripts/docpact-gate.sh
  - scripts/install-git-hooks.sh
lastReviewedAt: 2026-06-30
lastReviewedCommit: 9b5030e6862d5aff7776794823c88b7812441233
related:
  - ../../AGENTS.md
  - ../../.docpact/config.yaml
  - ./repo-validation.md
  - ../../README.md
---

## Repo Shape

This repo packages standalone tooling plus the schema, methodology, and stylesheet assets those tools execute against.

Review note, 2026-07-17: Issue #112 remains inside the existing validation and release-packaging modules. Explicit UTF-8 reads and LF writes make the same packaged assets and report structures byte-stable on Windows; no tool family, asset source, downstream dispatch path, or release architecture changes.

## Stable Path Map

| Path group | Role |
| --- | --- |
| `src/tidas_tools/convert.py` | standalone conversion CLI |
| `src/tidas_tools/import_lca/**` | external LCA import CLI scaffolding, format detection, canonical import model, and staged source adapters |
| `src/tidas_tools/validate.py` | standalone validation CLI |
| `src/tidas_tools/validation_report.py` | structured validation-report rendering |
| `src/tidas_tools/validation_indexes/**` | validator-private projection indexes derived from packaged schema assets for fast runtime checks |
| `src/tidas_tools/export.py` | standalone export CLI |
| `src/tidas_tools/package_versions.py` | version normalization and export package metadata logic |
| `src/tidas_tools/release.py` | deterministic release-profile closure, TIDAS/ILCD conversion and validation orchestration, semantic round-trip, and byte-stable ZIP construction |
| `src/tidas_tools/runtime_rulesets.py` | loader and validator for packaged runtime ruleset metadata |
| `src/tidas_tools/tidas/schemas/**` | packaged English TIDAS schemas |
| `src/tidas_tools/tidas/schemas_zh/**` | packaged Chinese TIDAS schemas |
| `src/tidas_tools/tidas/schema.lock.json` | deterministic hash and parity lock for paired TIDAS schemas |
| `src/tidas_tools/tidas/methodologies/**` | packaged TIDAS methodologies |
| `src/tidas_tools/eilcd/**` | packaged eILCD schemas and stylesheets |
| `scripts/schema_lock.py` | schema asset parity checker and lock generator |
| `tests/**` | automated repo tests |
| `.github/workflows/ci.yml` | manual-dispatch remote reproduction of schema-lock and local tests |
| `.github/workflows/dispatch-tidas-sdk-sync.yml` | downstream SDK refresh dispatch contract |
| `.github/workflows/python-package-deploy.yml` | `main` version-bump and tag-driven PyPI publish workflow with a release test gate |

## Current Tool Families

### Conversion

`convert.py` owns standalone TIDAS and eILCD conversion behavior.

`import_lca/` owns staged external LCA source import behavior for `tidas-import`. The current foundation covers CLI dispatch, format detection, `.zolca` rejection, canonical store scaffolding, TIDAS package layout helpers, default per-process TIDAS dependency bundle output for parallel AI import workers, opt-in gzip-compressed expert mapping CSV output, ILCD bridging, and conversion reports. Validated adapters currently exist for openLCA JSON-LD, EcoSpold 1, SimaPro CSV, EcoSpold 2, and openLCA process XLSX, with canonical contact/source writing and generated unit group / flow property support for source units.

### Validation

`validate.py` plus `validation_report.py` own standalone validation semantics and structured reporting. The validator covers TIDAS JSON with packaged JSON schemas, validator-private projection indexes, and eILCD/ILCD XML with packaged XSD schemas. TIDAS JSON validation uses a compiled `fastjsonschema` fast path for files that pass schema validation, while falling back to `jsonschema` for complete error collection when the fast path detects a schema failure.

### Export

`export.py` plus `package_versions.py` own database export semantics, version normalization, and archive packaging.

### Release Packaging

`release.py` owns the offline `tidas-release-tool` surface used by the Release control plane. It consumes an already finalized canonical dataset tree and index; it does not assign UUIDs or versions. It resolves exact transitive references for `unit-process-full-closure.v1` and `standalone-lifecyclemodel-result-full-closure.v1`, proves the latter contains the former, converts the same datasets to ILCD, validates TIDAS/ILCD, checks normalized semantic round-trip, and writes byte-stable ZIPs with sorted members and fixed metadata.

## Upstream Asset Chain

The practical executable chain today is:

`tidas-tools -> tidas-sdk`

Important consequences:

- `tidas-sdk` runtime assets and generated models depend on the packaged assets here
- schema or methodology changes here usually imply downstream SDK follow-up
- validator-private projection indexes optimize `tidas-tools` validation only; they are not a substitute for the packaged schema contract
- English and Chinese schema files must have matching file sets and matching non-localized structure
- `schema.lock.json` stores content hashes plus contract hashes after removing localized `description` fields
- `tidas` remains the public docs surface, but it is not the executable upstream for tooling behavior

`src/tidas_tools/tidas/methodologies/runtime_rulesets.json` is the packaged
runtime metadata layer over the methodology YAML assets. It assigns stable rule
ids, severity, phases, blocker defaults, and source-rule references for CLI and
Foundry gates without moving gate execution logic into this repository.

## Release And Dispatch Architecture

- `main` pushes whose `pyproject.toml` project version changes create the matching `v<version>` tag and publish `tidas-tools`
- manual `v<version>` tag pushes and workflow-dispatch runs for existing release tags remain recovery/backfill paths
- changes under packaged English schema, Chinese schema, and methodology paths can dispatch downstream SDK refresh workflows

This dispatch path is part of the repo architecture, not just a convenience automation.

## Common Misreads

- public docs changes do not replace executable tool changes here
- generated SDK output is downstream, not the upstream source of truth
- a merged child PR does not finish workspace delivery

## Local Docpact Push Gate

This repository has a versioned local `pre-push` hook under `.githooks/pre-push` that delegates to `scripts/docpact-gate.sh` and then runs `uv run pytest`. The gate resolves the CLI through `scripts/docpact`, so local agent shells do not need bare `docpact` on `PATH`. The hook is the local guard for docpact config validation, enforced doc-governance linting, and tests; the GitHub `CI` workflow is manual-dispatch only.
