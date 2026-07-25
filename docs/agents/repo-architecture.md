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
  - Cargo.toml
  - Cargo.lock
  - crates/**
  - contracts/**
  - assets/**
  - migration/**
  - pyproject.toml
  - src/tidas_tools/**
  - .github/workflows/**
  - .githooks/pre-push
  - scripts/docpact
  - scripts/schema_lock.py
  - scripts/docpact-gate.sh
  - scripts/install-git-hooks.sh
lastReviewedAt: 2026-07-25
lastReviewedCommit: 1dd24944f3f076864121b7cb3eda7f3e184099e5
lastReviewedNote: "Issue #118 establishes the Rust foundation, executable-asset lock, and XML portability boundary for the staged #117 migration."
related:
  - ../../AGENTS.md
  - ../../.docpact/config.yaml
  - ./repo-validation.md
  - ../../README.md
---

## Repo Shape

This repo is migrating its standalone tooling into a Cargo workspace that
builds one cross-platform `tidas` executable. The existing Python tree is
feature-frozen and remains only as the functional/deterministic parity oracle
until all #117 exit gates pass.

## Rust workspace

| Path | Stable responsibility |
| --- | --- |
| `crates/tidas-contracts` | versioned operation reports, diagnostics, artifact references, completeness, and exit-code classes |
| `crates/tidas-runtime` | cancellation, explicit memory reservations, bounded queues, and streaming JSONL spooling |
| `crates/tidas-assets` | offline executable-asset embedding, classification, integrity checking, and fingerprinting |
| `crates/tidas-xml` | strict streaming XML inspection plus the compatibility boundary to XSD/XSLT engines |
| `crates/tidas-cli` | the single `tidas` binary, final command tree, output routing, and thin domain dispatch |
| `contracts/**` | checked-in JSON Schema for stable machine contracts |
| `assets/asset-lock.v1.json` | exact path, kind, byte length, and SHA-256 ownership lock for every executable asset |
| `.gitattributes` | LF checkout contract for byte-identical assets and machine contracts on every platform |
| `migration/python-to-rust-owners.md` | frozen Python public-symbol inventory and dependency-ordered Rust owner map |

Later issues add `tidas-validate`, `tidas-convert`, `tidas-import`,
`tidas-export`, and `tidas-release` domain crates. The CLI crate must not absorb
their logic.

The command tree is fixed to `convert`, `import`, `export`, `validate`,
`release`, `ruleset`, and `version`. No old executable alias or Python fallback
is present. Until a domain slice lands, its Rust command returns the stable
`unavailable` exit class (69).

## Stable contract policy

Machine contracts use explicit `tidas.*.v1` identifiers, reject unknown fields
at typed Rust boundaries, emit LF-terminated JSON, and use ordered maps where
field order contributes to reproducibility. Additive evolution requires a new
optional field or a new schema version; existing field meaning and exit-code
classes cannot drift silently. Outputs contain no implicit wall-clock time,
locale-dependent values, or non-deterministically ordered collections.

Large-data domains must use the `tidas-runtime` cancellation token, bounded
queues, explicit memory reservations, and streaming spools rather than
collecting issue lists or complete packages in memory.

## XML/XSD/XSLT portability decision

- `quick-xml` owns strict, streaming, pure-Rust XML inspection.
- `libxml2` through `libxml` owns XSD validation.
- `libxslt` owns the current XSLT 1.0 compatibility layer required by packaged
  eILCD stylesheets.
- native XSD/XSLT calls are serialized behind one process-wide lock because the
  Rust wrapper does not establish safe concurrent schema use.
- development builds dynamically resolve system libraries; release packaging
  must use controlled, pinned native libraries and record their versions.
- production transforms must install a fail-closed resolver before untrusted
  input is accepted; network and arbitrary filesystem resolution are not part
  of the product contract.

This is intentionally a portability spike and boundary decision, not proof
that all production stylesheets are migrated. Functional conversion and release
coverage remains in #121 and #124.

Review note, 2026-07-17: Issue #112 remains inside the existing validation and release-packaging modules. Explicit UTF-8 reads and LF writes make the same packaged assets and report structures byte-stable on Windows; no tool family, asset source, downstream dispatch path, or release architecture changes.

## Stable Path Map

| Path group | Role |
| --- | --- |
| `Cargo.toml`, `Cargo.lock`, `crates/**` | Rust workspace and final product implementation |
| `contracts/**` | stable machine-readable Rust contract schemas |
| `assets/asset-lock.v1.json` | deterministic executable-asset ownership and integrity lock |
| `.gitattributes` | cross-platform LF checkout normalization for hashed inputs |
| `migration/**` | tracked migration inventory and ownership decisions |
| `src/tidas_tools/convert.py` | standalone conversion CLI |
| `src/tidas_tools/import_lca/**` | external LCA import CLI scaffolding, format detection, canonical import model, and staged source adapters |
| `src/tidas_tools/validate.py` | standalone validation CLI |
| `src/tidas_tools/validation_report.py` | structured validation-report rendering |
| `src/tidas_tools/validation_batch.py` | certificate-grade manifest validation, describe handshake, streamed issue events, and deterministic logical issue hash |
| `src/tidas_tools/reference_extraction.py` | pure version-preserving reference edge and extraction-issue contract |
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

## Frozen Python oracle families

### Conversion

`convert.py` owns standalone TIDAS and eILCD conversion behavior.

`import_lca/` owns staged external LCA source import behavior for `tidas-import`. The current foundation covers CLI dispatch, format detection, `.zolca` rejection, canonical store scaffolding, TIDAS package layout helpers, default per-process TIDAS dependency bundle output for parallel AI import workers, opt-in gzip-compressed expert mapping CSV output, ILCD bridging, and conversion reports. Validated adapters currently exist for openLCA JSON-LD, EcoSpold 1, SimaPro CSV, EcoSpold 2, and openLCA process XLSX, with canonical contact/source writing and generated unit group / flow property support for source units.

### Validation

`validate.py` plus `validation_report.py` own standalone validation semantics and structured reporting. The validator covers TIDAS JSON with packaged JSON schemas, validator-private projection indexes, and eILCD/ILCD XML with packaged XSD schemas. TIDAS JSON validation uses a compiled `fastjsonschema` fast path for files that pass schema validation, while falling back to `jsonschema` for complete error collection when the fast path detects a schema failure.

`validation_batch.py` adds `document-validation-batch.v1` for Worker orchestration. It validates exactly the regular files declared by a JSONL manifest, verifies their content hashes, streams canonical issue events, and ends with a small final event carrying the logical issue-stream hash and tool/engine/Schema-lock fingerprint. Data issues complete normally; malformed manifests, unsafe paths, drift, or missing final execution proof are protocol failures.

`reference_extraction.py` exposes `ReferenceExtractionResultV1` and `ReferenceEdgeV1`. It preserves explicit or omitted requested versions, stable reference roles, JSON paths, and malformed-reference issues. It does not look up targets or decide visibility, version winners, link readiness, Scope closure, or Certificates; those remain Worker/Database responsibilities. The process-bundle writer consumes this facade as a compatibility adapter while retaining its importer-shaped UUID file layout.

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
- `.github/workflows/rust-ci.yml` exercises Linux x86_64/ARM64, macOS
  Intel/Apple Silicon, and Windows x86_64; Windows ARM64 is a tracked
  second-phase target

This dispatch path is part of the repo architecture, not just a convenience automation.

## Common Misreads

- public docs changes do not replace executable tool changes here
- generated SDK output is downstream, not the upstream source of truth
- a merged child PR does not finish workspace delivery

## Local Docpact Push Gate

This repository has a versioned local `pre-push` hook under
`.githooks/pre-push`. It runs docpact, the asset lock, Rust
format/lint/tests, and the frozen Python schema-lock/Black/pytest parity oracle.
The gate resolves the CLI through `scripts/docpact`, so local agent shells do
not need bare `docpact` on `PATH`.
