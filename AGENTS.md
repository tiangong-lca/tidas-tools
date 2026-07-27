---
title: tidas-tools Repo Contract
docType: contract
scope: repo
status: active
authoritative: true
owner: tidas-tools
language: en
whenToUse:
  - when a task may change standalone TIDAS conversion, validation, export behavior, or packaged schema and methodology assets
  - when routing work from the workspace root into tidas-tools
  - when deciding whether a change belongs here, in tidas-sdk, in tidas, or in lca-workspace
whenToUpdate:
  - when tool ownership, runtime prerequisites, or release automation change
  - when the SDK dispatch contract changes
  - when repo-local documentation governance changes
checkPaths:
  - AGENTS.md
  - README.md
  - README_CN.md
  - .gitattributes
  - .docpact/**/*.yaml
  - docs/agents/**
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
  - .githooks/**
  - scripts/docpact
  - scripts/schema_lock.py
  - scripts/docpact-gate.sh
  - scripts/install-git-hooks.sh
  - scripts/install.sh
  - scripts/install.ps1
  - scripts/publish-crates.sh
  - scripts/test-release-request.sh
  - scripts/validate-release-request.sh
  - scripts/sync-rust-package-assets.sh
lastReviewedAt: 2026-07-27
lastReviewedCommit: 522c4e86f6d6934fed3f2e0940cb3c46cf7569d6
lastReviewedNote: "Reviewed for Issue #142 phase 1: the exact v0.1.1 Rust version set, repository-private tidas-dist boundary, five supported native targets, and release-request separation preserve repo ownership and delivery gates."
related:
  - .docpact/config.yaml
  - docs/agents/cli-contract.md
  - docs/agents/repo-validation.md
  - docs/agents/repo-architecture.md
  - README.md
  - README_CN.md
---

## Repo Contract

`tidas-tools` owns the unified cross-platform `tidas` executable, its reusable
Rust domain crates, standalone TIDAS/eILCD behavior, and the packaged schema and
methodology assets consumed by downstream SDK refreshes. Issue #117 is moving
the frozen Python implementation to Rust in dependency-ordered slices. Until
the final cutover gate passes, Python is an internal golden/parity oracle and
must not receive new product features.

Review note, 2026-07-17: Issue #112 makes packaged schema reads explicitly UTF-8 and release JSON writes explicitly LF, adds Windows regression proof, and publishes the recovery as 0.0.42. Conversion/profile semantics, packaged assets, dependencies, release automation, immutable tag rules, and workspace-integration requirements are unchanged.

## Documentation Roles

| Document | Owns | Does not own |
| --- | --- | --- |
| `AGENTS.md` | repo contract, branch and delivery rules, hard boundaries, minimal execution facts | full path map, proof matrix, or long setup prose |
| `.docpact/config.yaml` | machine-readable repo facts, routing intents, governed-doc rules, ownership, coverage, and freshness | explanatory prose or long-form walkthroughs |
| `docs/agents/repo-validation.md` | minimum proof by change type, manual CLI probes, PR validation note shape | repo contract, branch policy truth, or architecture explanations |
| `docs/agents/repo-architecture.md` | compact tool topology, stable path map, upstream asset chain, dispatch and release model | checklist-style proof guidance or user-facing CLI docs |
| `docs/agents/cli-contract.md` | authoritative unified command, configuration, stream, machine-report, completion, and exit contract | domain conversion, validation, import, export, or release semantics |
| `README.md` | English user-facing CLI examples and basic development commands | machine-readable routing or lint semantics |
| `README_CN.md` | Chinese user-facing CLI examples and basic development commands | machine-readable routing or lint semantics |

## Load Order

Read in this order:

1. `AGENTS.md`
2. `.docpact/config.yaml`
3. `docs/agents/cli-contract.md` for public CLI work, otherwise `docs/agents/repo-validation.md` or `docs/agents/repo-architecture.md`
4. `README.md` or `README_CN.md` only for user-facing CLI examples

## Operational Pointers

- path-level ownership, routing intents, governed-doc inventory, and lint rules live in `.docpact/config.yaml`
- minimum proof and manual CLI probe guidance live in `docs/agents/repo-validation.md`
- stable path groups, upstream asset handoffs, and release / dispatch topology live in `docs/agents/repo-architecture.md`
- unified CLI behavior and machine-output stability live in `docs/agents/cli-contract.md`
- repo-local documentation maintenance is enforced locally by the pre-push docpact gate; `.github/workflows/ai-doc-lint.yml` is manual-dispatch fallback
- schema asset parity and lock validation are enforced by `scripts/schema_lock.py` and `.github/workflows/ci.yml`
- the main routing intents are `tool-runtime`, `conversion`, `validation`, `export`, `packaged-assets`, `sdk-dispatch`, `release`, `proof`, `repo-docs`, and `root-integration`

## Minimal Execution Facts

Keep these entry-level facts in `AGENTS.md`. Use `README.md`, `README_CN.md`, and `docs/agents/repo-validation.md` for fuller command detail.

- Rust workspace toolchain: Rust 1.88 or newer, Cargo resolver 3
- crates.io qualification/publication toolchain: Cargo 1.94.0, pinned separately
  from the Rust 1.88 product MSRV because coordinated workspace publication
  requires Cargo's stable multi-package publishing support
- final product entry point: `cargo run -p tidas --bin tidas -- <subcommand>`
- final command tree: `convert`, `import`, `export`, `validate`, `release`, `ruleset`, `version`
- native package conversion: `cargo run -p tidas --bin tidas -- convert <input-dir> --output <output-dir> --to ilcd|tidas --format json`
- native external import: `cargo run -p tidas --bin tidas -- import <input-file-or-dir> --output <output-dir> [--target tidas|ilcd|both] [--write-mapping] --format json`
- native package validation: `cargo run -p tidas --bin tidas -- validate <package-dir> --input-format tidas-json|ilcd-xml --issues <issues.jsonl> --format json`
- native batch validation: `cargo run -p tidas --bin tidas -- validate <batch-dir> --protocol document-validation-batch.v1 --input-manifest <manifest.jsonl> --events <events.jsonl> --format json`
- native ruleset inspection: `cargo run -p tidas --bin tidas -- ruleset [--id <ruleset-id>] --format json`
- configuration precedence: explicit CLI option, then matching `TIDAS_*` environment variable, then the documented built-in default; no implicit current-directory config
- stdout is reserved for one report or completion script; logs, progress, and file-write confirmations use stderr
- canonical Rust checks: `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, and `cargo test --workspace --all-targets`
- executable asset lock: `cargo run -p tidas-assets --bin tidas-asset-lock -- check`
- deterministic native distribution: `cargo run --locked -p tidas-dist -- package --binary <tidas-binary> --license LICENSE --target <target-triple> --output-dir <dir>`
- crates.io package gate: `scripts/publish-crates.sh check`
- source installation package: `cargo install tidas --version <version> --locked`
- all public Rust crates use the same exact workspace version; `tidas-dist` is never published
- supported native artifact matrix: Linux x86_64/ARM64, macOS Intel/Apple Silicon, and Windows x86_64; Windows ARM64 is not supported
- migration-oracle package manager and runner: `uv`
- routine branch base: `main`
- routine PR base: `main`
- canonical setup: `uv sync --dev`
- canonical local test command: `uv run pytest`
- after every code fix, run Black lint before final validation: `uv run black --check --target-version py313 src tests`
- canonical schema asset check: `uv run python scripts/schema_lock.py check`
- Python oracle probes during migration:
  - `uv run python src/tidas_tools/convert.py --help`
  - `uv run python src/tidas_tools/import_lca/cli.py --help`
  - `uv run python src/tidas_tools/validate.py --help`
  - `uv run python src/tidas_tools/export.py --help`
  - `uv run python -m tidas_tools.release --help`
- release tag pattern: `v<version>`
- native release request pattern: `.github/releases/v<version>.json`, containing
  exactly the release schema, workspace version, and full target commit
- pull requests validate release requests without write permissions or secrets;
  merging an append-only request to `main` creates or verifies the exact
  lightweight tag and explicitly dispatches `.github/workflows/rust-release.yml`
  at that tag so provenance is bound to the released commit
- request validation rejects version, target, ancestry, filename, request-diff,
  existing-tag, and existing-release mismatches before external publication
- canonical `main` branch pushes whose `pyproject.toml` project version changed create the matching `v<version>` tag when missing, run the release gate, test on the release matrix, and publish to PyPI in the same workflow run
- manual `v*` tag pushes and `workflow_dispatch` at an existing `v*` tag whose
  target commit is already on `main` remain native recovery/backfill paths

## Ownership Boundaries

The authoritative path-level ownership map lives in `.docpact/config.yaml`.

At a human-readable level, this repo owns:

- the Cargo workspace and final `tidas` binary under `Cargo.toml` and `crates/**`
- native bidirectional conversion, deterministic envelope sidecars, atomic publication, and `tidas.conversion-report.v1` under `crates/tidas-conversion`, `contracts/conversion-report.v1.schema.json`, and `tests/fixtures/conversion_v1/**`
- native external-format detection/import, disk-backed canonicalization, deterministic TIDAS/ILCD publication, process bundles, mapping CSV, and import reports under `crates/tidas-import` and `contracts/import-*.v1.schema.json`
- native exact release closure, schema-ordered ILCD derivation, validation and semantic round-trip gates, and four deterministic package publication under `crates/tidas-release` and `contracts/release-report.v1.schema.json`
- deterministic executable archives, checksum verification, package-manager metadata, packaged smoke proof, self-contained crate inputs, and coordinated crates.io publication under the root `tidas-assets` package allowlist, `crates/tidas-dist`, `crates/*/contracts`, `packaging/**`, `scripts/install.*`, `scripts/publish-crates.sh`, `scripts/sync-rust-package-assets.sh`, and `.github/workflows/rust-release.yml`
- stable machine contracts under `contracts/**`
- the complete executable asset inventory in `assets/asset-lock.v1.json`
- the Python-to-Rust owner inventory under `migration/**`
- standalone CLI behavior in `src/tidas_tools/convert.py`, `src/tidas_tools/import_lca/**`, `src/tidas_tools/validate.py`, and `src/tidas_tools/export.py`
- deterministic `document-validation-batch.v1` streaming validation and reproducibility handshake in `src/tidas_tools/validation_batch.py`
- the active pure `ReferenceExtractionResultV1` / `ReferenceEdgeV1` contract in `crates/tidas-references`, checked-in machine schema under `contracts/**`, and frozen Python golden oracle in `src/tidas_tools/reference_extraction.py` plus `tests/fixtures/reference_extraction_v1/**`
- the frozen release parity oracle in `src/tidas_tools/release.py`; active release behavior belongs to `crates/tidas-release`
- validation report and version/export helpers in `src/tidas_tools/validation_report.py` and `src/tidas_tools/package_versions.py`
- validator-private projection indexes under `src/tidas_tools/validation_indexes/**`
- packaged TIDAS schemas and methodologies under `src/tidas_tools/tidas/**`
- packaged eILCD schemas and stylesheets under `src/tidas_tools/eilcd/**`
- tests and automation under `tests/**`, `scripts/**`, `.github/workflows/ci.yml`, `.github/workflows/rust-release.yml`, `.github/workflows/dispatch-tidas-sdk-sync.yml`, and `.github/workflows/python-package-deploy.yml`
- `README.md`, `README_CN.md`, `docs/agents/**`, `.docpact/**`, and `.github/workflows/ai-doc-lint.yml` for repo-local governance and retained docs

This repo does not own:

- generated SDK package surfaces
- public spec/docs-site presentation
- workspace integration state after merge

Route those tasks to:

- `tidas-sdk` for generated package surfaces and package release automation
- `tidas` for public spec/docs-site content
- `lca-workspace` for root integration after merge

## Branch And Delivery Facts

- GitHub default branch: `main`
- true daily trunk: `main`
- routine branch base: `main`
- routine PR base: `main`
- branch model: `M1`

`tidas-tools` does not use a separate promote line. Normal implementation merges to `main`, and later workspace delivery still requires a root submodule bump when the updated tooling snapshot should ship through `lca-workspace`.

## Operational Invariants

- do not move standalone conversion, validation, or export logic into `tidas-sdk`
- keep `tidas-cli` thin; reusable behavior belongs in domain crates
- do not add legacy Rust executable aliases or a PyPI Rust wrapper
- do not invoke Python from Rust; incomplete Rust commands must fail with the stable unavailable exit class
- large-data paths must stream through bounded queues, explicit memory budgets, and cancellation-aware boundaries
- conversion must reject symlinks and XML 1.0-invalid characters, preserve package metadata, and consume deterministic envelope sidecars on reverse conversion
- native JSON Schema resolution is offline and may resolve only the embedded, integrity-locked TIDAS schema catalog
- `assets/asset-lock.v1.json` is the integrity authority for executable schemas, methodologies, rulesets, indexes, XSD, XSLT, and XML reference assets
- `.gitattributes` forces executable assets, machine contracts, source, and governed docs to LF so byte hashes are identical on Windows, macOS, and Linux
- native libxml2/libxslt access is serialized until thread-safety is independently proved; production XSLT must fail closed on external resource resolution
- native release archives must derive from one exact binary, use pinned static libxml2/libxslt inputs, carry fixed archive metadata and SHA-256, and pass packaged-binary smoke probes without a development toolchain
- GitHub Release publication must be tag/version exact and refuse mutation of an existing release; Homebrew and Winget metadata must use the same archive checksums and must not rebuild the binary
- public crates must be self-contained, exact-versioned as one release set, and package/dry-run clean; only the tag-context release workflow may read `CARGO_REGISTRY_TOKEN`
- crates.io publication must verify an existing version's archive checksum before a retry skips it, and the immutable GitHub Release must wait for the complete registry set
- Python remains frozen until functional parity, deterministic contracts, local performance/RSS targets, cross-platform artifacts, and downstream cutovers all pass; then #126 removes every active Python implementation/install/invocation path
- do not treat the public docs site as the executable upstream for packaged schemas and methodologies
- packaged assets under `src/tidas_tools/**` are executable tooling inputs, not just reference docs
- validator-private projection indexes may optimize standalone validation, but they must not replace or weaken packaged TIDAS schema contracts
- reference extraction preserves source constraints and extraction defects but does not resolve database targets, visibility, or exact-version winners
- English and Chinese TIDAS schema assets must stay structurally aligned through `src/tidas_tools/tidas/schema.lock.json`
- schema or methodology changes here can require downstream `tidas-sdk` follow-up through the dispatch contract
- merged repo PRs here are repo-complete, not workspace-delivery complete

## Documentation Update Rules

- if a machine-readable repo fact, routing intent, or governed-doc rule changes, update `.docpact/config.yaml`
- if a human-readable repo contract, branch rule, or hard boundary changes, update `AGENTS.md`
- if proof expectations or manual probe guidance change, update `docs/agents/repo-validation.md`
- if repo shape, asset groups, or cross-repo handoff explanation changes, update `docs/agents/repo-architecture.md`
- if user-facing English CLI examples or setup commands change, update `README.md`
- if user-facing Chinese CLI examples or setup commands change, update `README_CN.md`
- do not copy the same rule into multiple docs just to make it easier to find

## Hard Boundaries

- do not treat generated SDK output as the upstream source of truth when the cause lives in tooling logic or packaged assets here
- do not treat docs-site wording as executable tooling behavior
- do not treat export behavior as part of the SDK package
- do not treat a merged repo PR here as workspace-delivery complete if the root repo still needs a submodule bump

## Workspace Integration

A merged PR in `tidas-tools` is repo-complete, not delivery-complete.

If the change must ship through the workspace:

1. merge the child PR into `tidas-tools`
2. update the `lca-workspace` submodule pointer deliberately
3. complete any later workspace-level validation that depends on the updated tooling snapshot

## Local Docpact Push Gate

Install the versioned local hook once per checkout:

```bash
./scripts/install-git-hooks.sh
```

The `pre-push` hook runs `scripts/docpact-gate.sh`, the Rust formatting/lint/test
and asset-lock gates, then the frozen Python schema-lock/Black/pytest oracle.
The wrapper checks `DOCPACT_BIN`, Cargo install locations, Homebrew install
locations, and then `PATH`, so local agent shells should not fail only because
bare `docpact` is unavailable. The default comparison base is `origin/main`.
Override it for unusual stacks with `DOCPACT_BASE_REF=<ref>` or
`scripts/docpact-gate.sh --base <ref>`. The gate writes its detailed report to
a temporary file so normal pushes do not create `.docpact/runs/` artifacts.
`.github/workflows/rust-ci.yml` proves the initial five-platform matrix on
pull requests; the existing Python publish workflow remains transitional.
