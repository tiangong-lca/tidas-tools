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
  - packaging/**
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
  - scripts/install.sh
  - scripts/install.ps1
  - scripts/publish-crates.sh
  - scripts/sync-rust-package-assets.sh
lastReviewedAt: 2026-07-26
lastReviewedCommit: eed5ed2
lastReviewedNote: "Issue #136 adds self-contained multi-package verification, crates.io dry-run, exact public-set/version checks, and checksum-safe publish retry proof."
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
| `tidas-conversion` or `tidas convert` | both-direction Python golden fixture; representative process/flow/flow property/unit group/contact/source/LCIA method/lifecycle model round-trip; report-schema, deterministic hash, envelope-sidecar, manifest-copy, symlink, cancellation, memory-budget, progress, and atomic rollback tests | run the local 237 MiB package in both directions, compare a second output-tree hash, record wall time/RSS, and perform per-document semantic comparison against the frozen Python oracle; malformed XML 1.0 data must fail without publishing output | Conversion keeps the strict XML single-root boundary, preserves known top-level extension metadata through deterministic sidecars, and never accumulates the package in memory. |
| `tidas-import` or `tidas import` | detect and import EcoSpold 1/2, SimaPro CSV, openLCA JSON-LD, openLCA process XLSX, and ILCD; replay frozen Python semantic fixtures; validate TIDAS and requested ILCD; compare package, mapping, and process-bundle hashes across repeated runs and different source roots; prove malformed input, `.zolca`, cancellation, memory-budget, exit-class, and atomic-publication behavior | exercise large exchange streams and issue spools locally; record wall time and peak RSS; confirm source-relative identifiers and gzip bytes are cross-platform deterministic | Canonical entities and exchanges stay disk-backed; issues stream to JSONL; requested outputs are validated before one atomic commit. |
| `tidas-export` or `tidas export` | replay Python package-version golden cases; focused crate and CLI tests; validate `tidas.export-report.v1`; prove secret redaction, unsafe-path failure, cancellation, memory budget, skipped-document warning, full version suffixes, deterministic ZIP bytes, and atomic replacement | run disposable local PostgreSQL and S3-compatible fixtures twice; compare archive membership/hash/bytes; run a large local record set with wall time and peak RSS | Database reads use a repeatable-read snapshot and bounded queue; object bodies stream by chunk; never begin connector or scale tests on a Worker server. |
| `tidas-release` or `tidas release` | replay the frozen Python closure/order/round-trip cases; validate `tidas.release-report.v1`; prove missing/inexact references fail closed, standalone contains unit closure, native TIDAS/ILCD validation, four stored ZIPs, fixed metadata, repeatable bytes, cancellation, memory budget, bounded report samples, and atomic whole-directory publication | run the local 237 MiB package twice; record wall time and peak RSS; compare all four archive hashes and membership; keep this local until the target is met | The release layer consumes finalized UUID/version decisions, never assigns them, and never invokes Python. |
| `tidas-dist`, native installers, package metadata, or Rust release automation | focused `tidas-dist` fmt/clippy/tests; package the local release binary twice and compare archive/checksum bytes; verify and run packaged `version`, help, JSON `version`, and `ruleset`; `bash -n scripts/install.sh`; actionlint; strict docpact | all five release jobs; Linux clean-container execution; macOS runtime dependency inspection; Windows packaged smoke and `winget validate`; SPDX SBOM generation; provenance/SBOM attestation on canonical dispatch/tag | Archives must derive from the supplied binary, pinned static XML libraries, fixed metadata, and the same SHA-256 values used by installers, Homebrew, and Winget. No public tag or external package-manager submission is required to review the pipeline. |
| public crate metadata, packaged contracts, root asset include allowlist, or crates.io automation | `scripts/sync-rust-package-assets.sh check`; `scripts/publish-crates.sh check`; confirm the public set is exactly version-synchronized and `tidas-dist` is excluded; `bash -n` both scripts; actionlint | inspect every `.crate` size/checksum; verify `cargo install --path crates/tidas-cli --locked`; exercise registry checksum lookup against absent and existing package records without a real token | Pull requests must not read `CARGO_REGISTRY_TOKEN`. The tag job may publish only after package qualification and must skip an existing version only when its crates.io checksum matches the locally qualified archive. |
| `tidas-validation` native package paths | focused crate and CLI tests; compile all eight JSON schemas and all used ILCD roots offline; compare frozen Python parity fixtures; validate issue, summary, describe, and batch payloads against checked-in schemas | run the local 237 MiB benchmark twice with wall time and peak RSS; prove cancellation, XSD import resolution, semantic index parity, and deterministic spool bytes/hash | Issue details must stream or be discarded, never accumulate in the operation report. |
| `tidas-rulesets` or `tidas ruleset` | schema validation, unique id/reference tests, warning/blocker preservation, catalog and selected-profile CLI probes | compare catalog fingerprint twice and confirm unknown ids use the usage exit class | Packaged methodology metadata remains executable, integrity-locked input; gate execution still belongs to its consumers. |
| `tidas-references` pure extraction | replay the frozen Python golden cases byte-for-structure; validate every result/edge/issue against the checked-in schema; prove role vocabulary and input failures are closed | exercise repeated/cyclic occurrences, invalid UUID/version/type/id, URI aliases, and explicit-versus-omitted versions | Extraction preserves source constraints and defects but never performs target lookup, visibility, winner selection, or closure. |
| `document-validation-batch.v1` Rust protocol | valid manifest, data-issue completion, malformed/unsafe/hash-drift failure, describe handshake, deterministic event order and logical hash | cancellation and mutation-between-preflight-and-validation probes; validate every emitted event against checked-in schemas | Data issues end with a final event and exit 0; protocol/system defects must not publish completion evidence. |
| `tidas-assets`, `.gitattributes`, or executable assets | asset-lock check, Rust tests, Python schema lock, full pytest; verify `git check-attr eol` returns `lf` for representative JSON/XSD/XSL files | regenerate the asset lock only after intentional review; compare asset fingerprint twice | The Rust asset lock covers more executable inputs than the legacy paired-schema lock; LF checkout and both lock gates remain during migration. |
| `tidas-xml` or native dependency workflow | focused `cargo test -p tidas-xml`; full Rust checks; all five CI matrix jobs | representative production XSD/XSLT fixtures; controlled static release-link proof and resolver security tests | `quick-xml` is the streaming reader; libxml2/libxslt native calls are serialized. |
| `convert.py`, `import_lca/**`, or eILCD asset changes | `uv run pytest`; `uv run python src/tidas_tools/convert.py --help`; `uv run python src/tidas_tools/import_lca/cli.py --help` when external import paths change | run one representative conversion or import path if the task explicitly changes data transformation behavior | Keep packaged asset, conversion logic, import detection, and staged adapters aligned. |
| `validate.py`, `validation_report.py`, TIDAS schema changes, or eILCD schema validation changes | `uv run python scripts/schema_lock.py check`; `uv run pytest`; `uv run python src/tidas_tools/validate.py --help` | run one representative TIDAS JSON or eILCD/ILCD XML validation path and record entity types touched | Validation categories, packaged JSON schemas, packaged XSD schemas, and the TIDAS schema parity lock all matter here. |
| `validation_batch.py` or `reference_extraction.py` | `uv run pytest tests/test_validation_batch.py tests/test_reference_extraction.py tests/test_validate.py -q`; `uv run python -m tidas_tools.validate --describe --format json`; Black | run the batch CLI against a valid manifest and replay the golden fixture in the downstream Rust consumer | Data issues must end with a valid final event and exit 0; protocol/system defects must not emit completion evidence. Explicit versions and roles must survive extraction unchanged. |
| validator-private projection index changes | `uv run pytest tests/test_validate.py -q`; `uv run python src/tidas_tools/validate.py --help` | compare the projection against its source schema and run a representative package validation path | Projection indexes may optimize validation only; they must stay derived from packaged schema contracts and must not replace them. |
| `export.py` or `package_versions.py` changes | `uv run pytest`; `uv run python src/tidas_tools/export.py --help` | if the task includes live export proof, record the DB and storage assumptions separately | Export behavior depends on external DB and object-storage state. |
| frozen `release.py` oracle changes | `uv run pytest`; `uv run python -m tidas_tools.release --help`; `uv run black --check --target-version py313 src tests`; focused `tidas-release` parity tests | run the same fixture through Python and Rust and compare closure membership, schema element order, normalized semantics, and deterministic archive properties | Python is parity evidence only; active release behavior and new features belong to Rust. |
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
