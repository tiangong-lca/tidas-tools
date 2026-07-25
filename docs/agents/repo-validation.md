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
  - Cargo.toml
  - Cargo.lock
  - crates/**
  - contracts/**
  - assets/**
  - migration/**
  - pyproject.toml
  - src/tidas_tools/**
  - tests/**
  - test_data/**
  - .github/workflows/**
  - .githooks/pre-push
  - scripts/docpact
  - scripts/schema_lock.py
  - scripts/docpact-gate.sh
  - scripts/install-git-hooks.sh
lastReviewedAt: 2026-07-25
lastReviewedCommit: 1dd24944f3f076864121b7cb3eda7f3e184099e5
lastReviewedNote: "Issue #119 adds CLI help/completion/configuration/output-channel/parity contract proof while retaining the five-platform Rust and Python oracle gates."
related:
  - ../../AGENTS.md
  - ../../.docpact/config.yaml
  - ./repo-architecture.md
  - ../../README.md
---

## Default Baseline

Review note, 2026-07-17: Issue #112 keeps the existing proof contract and makes its four-platform release matrix authoritative for UTF-8 schema/index parity and LF release bytes. The 0.0.42 recovery still requires schema lock, Black, full pytest, release CLI help, Docpact, the complete publish matrix, and PyPI verification.

Unless the change is doc-only, the default local baseline during migration is:

```bash
cargo run --locked -p tidas-assets --bin tidas-asset-lock -- check
cargo fmt --all --check
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo test --locked --workspace --all-targets
uv sync --dev
uv run python scripts/schema_lock.py check
uv run black --check --target-version py313 src tests
uv run pytest
```

Use narrower manual probes only when the task touches one CLI surface and full tests would add no extra signal.

The local `pre-push` hook runs docpact, Rust, asset-lock, and frozen Python
oracle gates. `.github/workflows/rust-ci.yml` runs on relevant pull requests
across the initial five-platform matrix. The old Python `CI` workflow remains
manual-dispatch during migration; the PyPI workflow is transitional and must
be removed only by the final #126 cutover.

## Validation Matrix

| Change type | Minimum local proof | Additional proof when risk is higher | Notes |
| --- | --- | --- | --- |
| Rust workspace, shared contracts, or CLI adapter | asset lock; Rust fmt/clippy/test; root and command help; deterministic `tidas --format json version` and completion comparisons | exercise configuration precedence, report-file/stdout separation, completion shells, migration parity fixture, structured usage failure, and every affected exit class; validate JSON against the checked-in schema | The CLI remains thin, follows `docs/agents/cli-contract.md`, and incomplete commands fail `unavailable` without invoking Python. |
| `tidas-runtime` large-data primitives | Rust fmt/clippy/test including bounded queue, cancellation, memory-budget, and streaming spool tests | local large-package benchmark with elapsed time and peak RSS; never start on a Worker server | The target package is intentionally outside Git and issue counts must not cause linear memory growth. |
| `tidas-assets`, `.gitattributes`, or executable assets | asset-lock check, Rust tests, Python schema lock, full pytest; verify `git check-attr eol` returns `lf` for representative JSON/XSD/XSL files | regenerate the asset lock only after intentional review; compare asset fingerprint twice | The Rust asset lock covers more executable inputs than the legacy paired-schema lock; LF checkout and both lock gates remain during migration. |
| `tidas-xml` or native dependency workflow | focused `cargo test -p tidas-xml`; full Rust checks; all five CI matrix jobs | representative production XSD/XSLT fixtures; controlled static release-link proof and resolver security tests | `quick-xml` is the streaming reader; libxml2/libxslt native calls are serialized. |
| `convert.py`, `import_lca/**`, or eILCD asset changes | `uv run pytest`; `uv run python src/tidas_tools/convert.py --help`; `uv run python src/tidas_tools/import_lca/cli.py --help` when external import paths change | run one representative conversion or import path if the task explicitly changes data transformation behavior | Keep packaged asset, conversion logic, import detection, and staged adapters aligned. |
| `validate.py`, `validation_report.py`, TIDAS schema changes, or eILCD schema validation changes | `uv run python scripts/schema_lock.py check`; `uv run pytest`; `uv run python src/tidas_tools/validate.py --help` | run one representative TIDAS JSON or eILCD/ILCD XML validation path and record entity types touched | Validation categories, packaged JSON schemas, packaged XSD schemas, and the TIDAS schema parity lock all matter here. |
| `validation_batch.py` or `reference_extraction.py` | `uv run pytest tests/test_validation_batch.py tests/test_reference_extraction.py tests/test_validate.py -q`; `uv run python -m tidas_tools.validate --describe --format json`; Black | run the batch CLI against a valid manifest and replay the golden fixture in the downstream Rust consumer | Data issues must end with a valid final event and exit 0; protocol/system defects must not emit completion evidence. Explicit versions and roles must survive extraction unchanged. |
| validator-private projection index changes | `uv run pytest tests/test_validate.py -q`; `uv run python src/tidas_tools/validate.py --help` | compare the projection against its source schema and run a representative package validation path | Projection indexes may optimize validation only; they must stay derived from packaged schema contracts and must not replace them. |
| `export.py` or `package_versions.py` changes | `uv run pytest`; `uv run python src/tidas_tools/export.py --help` | if the task includes live export proof, record the DB and storage assumptions separately | Export behavior depends on external DB and object-storage state. |
| `release.py` changes | `uv run pytest`; `uv run python -m tidas_tools.release --help`; `uv run black --check --target-version py313 src tests` | run one four-package fixture and compare archive hashes across two builds; run ILCD XSD validation when canonical production-grade fixtures are available | Release packaging must fail closed on missing exact references and preserve deterministic member order, timestamps, modes, and bytes. |
| packaged methodology or schema asset changes | `uv run python scripts/schema_lock.py check`; `uv run pytest` | run `uv run python scripts/schema_lock.py update` before committing TIDAS schema changes; record whether `tidas-sdk` follow-up is required; run the relevant manual probe if a specific CLI surface depends on the asset | These paths are the current executable upstream for downstream package refresh. |
| workflow or release automation changes | `uv run python scripts/schema_lock.py check`; `uv run pytest` | inspect the touched workflow and record any tag, lock, or dispatch assumptions checked locally | Downstream dispatch and tag-based publish are separate from local tool tests. |
| repo contract or governed-doc changes only | `scripts/docpact validate-config --root . --strict` and `scripts/docpact lint --root . --staged --mode enforce` | run one focused route check such as `scripts/docpact route --root . --intent repo-docs --format text` or `sdk-dispatch` when the change touches dispatch docs | Refresh review evidence even when prose-only governed docs change. |

## Minimum PR Note Quality

A good PR note for this repo should say:

1. whether `uv run pytest` ran
2. whether Rust fmt, clippy, workspace tests, and the asset-lock check ran
3. whether `uv run python scripts/schema_lock.py check` ran when schema assets or workflows changed
4. which Rust and/or Python CLI probes ran, including the data format used
5. whether downstream `tidas-sdk` follow-up is required
6. whether any large-package, cross-platform, or live export proof is deferred

## Local Docpact Push Gate

Install the versioned local hook once per checkout:

```bash
./scripts/install-git-hooks.sh
```

The `pre-push` hook runs `scripts/docpact-gate.sh`, then the asset lock, Rust
format/lint/tests, and frozen Python schema-lock/Black/pytest oracle. The
wrapper checks `DOCPACT_BIN`, Cargo install locations, Homebrew install
locations, and then `PATH`. The default comparison base is `origin/main`;
override unusual stacks with `DOCPACT_BASE_REF=<ref>`.
