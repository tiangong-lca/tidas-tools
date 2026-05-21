---
title: tidas-tools Validation Guide
docType: guide
scope: repo
status: active
authoritative: false
owner: tidas-tools
language: en
whenToUse:
  - when a tidas-tools change is ready for local validation
  - when deciding the minimum proof required for conversion, validation, export, asset, or automation changes
  - when writing PR validation notes for tidas-tools work
whenToUpdate:
  - when the repo gains new canonical test wrappers
  - when change categories require different proof
  - when release or dispatch behavior changes
checkPaths:
  - docs/agents/repo-validation.md
  - .docpact/config.yaml
  - pyproject.toml
  - src/tidas_tools/**
  - tests/**
  - test_data/**
  - .github/workflows/**
  - .githooks/pre-push
  - scripts/docpact
  - scripts/docpact-gate.sh
  - scripts/install-git-hooks.sh
lastReviewedAt: 2026-05-21
lastReviewedCommit: 2a93b8a63a04bb0c4ce05513faee6077972f82e3
related:
  - ../../AGENTS.md
  - ../../.docpact/config.yaml
  - ./repo-architecture.md
  - ../../README.md
---

## Default Baseline

Unless the change is doc-only, the default local baseline is:

```bash
uv sync --dev
uv run pytest
```

Use narrower manual probes only when the task touches one CLI surface and full tests would add no extra signal.

## Validation Matrix

| Change type | Minimum local proof | Additional proof when risk is higher | Notes |
| --- | --- | --- | --- |
| `convert.py`, `import_lca/**`, or eILCD asset changes | `uv run pytest`; `uv run python src/tidas_tools/convert.py --help`; `uv run python src/tidas_tools/import_lca/cli.py --help` when external import paths change | run one representative conversion or import path if the task explicitly changes data transformation behavior | Keep packaged asset, conversion logic, import detection, and staged adapters aligned. |
| `validate.py`, `validation_report.py`, TIDAS schema changes, or eILCD schema validation changes | `uv run pytest`; `uv run python src/tidas_tools/validate.py --help` | run one representative TIDAS JSON or eILCD/ILCD XML validation path and record entity types touched | Validation categories, packaged JSON schemas, and packaged XSD schemas all matter here. |
| `export.py` or `package_versions.py` changes | `uv run pytest`; `uv run python src/tidas_tools/export.py --help` | if the task includes live export proof, record the DB and storage assumptions separately | Export behavior depends on external DB and object-storage state. |
| packaged methodology or schema asset changes | `uv run pytest` | record whether `tidas-sdk` follow-up is required; run the relevant manual probe if a specific CLI surface depends on the asset | These paths are the current executable upstream for downstream package refresh. |
| workflow or release automation changes | `uv run pytest` | inspect the touched workflow and record any tag or dispatch assumptions checked locally | Downstream dispatch and tag-based publish are separate from local tool tests. |
| repo contract or governed-doc changes only | `scripts/docpact validate-config --root . --strict` and `scripts/docpact lint --root . --staged --mode enforce` | run one focused route check such as `scripts/docpact route --root . --intent repo-docs --format text` or `sdk-dispatch` when the change touches dispatch docs | Refresh review evidence even when prose-only governed docs change. |

## Minimum PR Note Quality

A good PR note for this repo should say:

1. whether `uv run pytest` ran
2. which manual CLI probes ran, if any, including the data format used for validation probes
3. whether downstream `tidas-sdk` follow-up is required
4. whether any export proof is deferred because it depends on live DB or storage state

## Local Docpact Push Gate

Install the versioned local hook once per checkout:

```bash
./scripts/install-git-hooks.sh
```

The `pre-push` hook runs `scripts/docpact-gate.sh`, which delegates CLI lookup to `scripts/docpact` and performs strict config validation plus enforced lint before the push leaves the machine. The wrapper checks `DOCPACT_BIN`, Cargo install locations, Homebrew install locations, and then `PATH`, so local agent shells should not fail only because bare `docpact` is unavailable. The default comparison base is `origin/main`. Override it for unusual stacks with `DOCPACT_BASE_REF=<ref>` or `scripts/docpact-gate.sh --base <ref>`. The gate writes its detailed report to a temporary file so normal pushes do not create `.docpact/runs/` artifacts.
