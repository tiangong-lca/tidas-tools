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
  - packaging/**
  - migration/**
  - pyproject.toml
  - src/tidas_tools/**
  - .github/workflows/**
  - .githooks/pre-push
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
lastReviewedAt: 2026-07-26
lastReviewedCommit: eed5ed2
lastReviewedNote: "Issue #138 makes an append-only Release Request PR the reviewed authorization for an exact tag and tag-context release dispatch."
related:
  - ../../AGENTS.md
  - ../../.docpact/config.yaml
  - ./cli-contract.md
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
| root package `tidas-assets`, source in `crates/tidas-assets` | offline executable-asset embedding, classification, integrity checking, and fingerprinting; the package include allowlist carries the authoritative root assets directly |
| `crates/tidas-conversion` | deterministic bidirectional TIDAS JSON/eILCD XML transformation, envelope sidecars, atomic directory publication, and conversion reports |
| `crates/tidas-import` | bounded external-format detection/parsing, disk-backed canonical storage, deterministic TIDAS/ILCD publication, process bundles, expert mapping CSV, and import reports |
| `crates/tidas-export` | repeatable-read PostgreSQL extraction, TIDAS/eILCD serialization, version normalization, S3-compatible streaming, and deterministic atomic ZIP publication |
| `crates/tidas-release` | exact UUID/version release closure, schema-ordered ILCD derivation, native validation and semantic round-trip gates, and four deterministic ZIPs |
| `crates/tidas-dist` | deterministic native executable archives, SHA-256 verification, packaged smoke probes, and Homebrew/Winget metadata rendered from the same artifact set |
| `crates/tidas-references` | side-effect-free, version-preserving reference extraction, role classification, and malformed-reference contracts |
| `crates/tidas-rulesets` | schema-validated methodology/ruleset catalog, referential integrity, selection, and deterministic fingerprinting |
| `crates/tidas-validation` | offline TIDAS JSON and ILCD/XSD validation, semantic indexes, batch protocol, deterministic traversal, and bounded issue/event spooling |
| `crates/tidas-xml` | strict streaming XML inspection plus the compatibility boundary to XSD/XSLT engines |
| `crates/tidas-cli` | crates.io package `tidas`, the single `tidas` binary, final command tree, output routing, and thin domain dispatch |
| `docs/agents/cli-contract.md` | authoritative command, configuration precedence, stream, completion, invocation-context, and exit behavior |
| `contracts/**` | checked-in JSON Schema for stable machine contracts |
| `crates/*/contracts` | generated, checked contract copies that make each non-root published crate self-contained; sync checks keep root `contracts/**` authoritative |
| `assets/asset-lock.v1.json` | exact path, kind, byte length, and SHA-256 ownership lock for every executable asset |
| `.gitattributes` | LF checkout contract for byte-identical assets and machine contracts on every platform |
| `migration/python-to-rust-owners.md` | frozen Python public-symbol inventory and dependency-ordered Rust owner map |

The complete conversion domain now lives in `tidas-conversion`; native external
import lives in `tidas-import`; native database/package export lives in
`tidas-export`; and the complete validation domain lives in
`tidas-validation`, with packaged methodology metadata isolated in
`tidas-rulesets` and reusable reference extraction isolated in
`tidas-references`. Native data-release control lives in `tidas-release`;
executable distribution lives in `tidas-dist`. The CLI crate must not absorb
domain or distribution logic.

The command tree is fixed to `convert`, `import`, `export`, `validate`,
`release`, `ruleset`, and `version`. No old executable alias or Python fallback
is present. Until a domain slice lands, its Rust command returns the stable
`unavailable` exit class (69). All seven product commands now have native Rust
implementations. `convert` atomically transforms TIDAS JSON and
eILCD XML package trees; `import` detects and maps supported external LCA
formats into validated TIDAS and optional ILCD outputs; `export` streams
PostgreSQL records and optional S3-compatible documents into deterministic
TIDAS/eILCD ZIPs; `release` consumes a finalized canonical tree/index and
publishes the two TIDAS plus two ILCD closure packages after all native gates;
`validate` accepts
native TIDAS JSON packages, ILCD XML packages, and
`document-validation-batch.v1`; `ruleset` validates and inspects the
integrity-locked methodology catalog. None invokes Python.

The CLI records the resolved configuration source, log/progress policy, memory
budget, queue capacity, and I/O policy in `tidas.invocation-context.v1`.
Configuration never depends on an implicit working-directory file. Completion
scripts are generated with `tidas --completion <shell>` without adding another
product command. See `docs/agents/cli-contract.md` for the authoritative public
behavior.

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

## Native conversion

`tidas-conversion` traverses package trees in sorted order without following
symlinks, reserves an explicit per-file memory estimate before reading, and
checks cancellation during conversion, asset materialization, and hashing.
The target is assembled in a sibling temporary directory, seeded from an
existing output when present, and renamed into place with rollback; a failed
conversion never publishes a partial tree.

TIDAS JSON maps `@attribute`, `#text`, repeated children, namespaces, null
elements, and scalar text to the frozen `xmltodict 1.0.4` semantics. A strict
single-root transformer remains the reusable format boundary. Package
documents that contain one known dataset root plus top-level `version` or
`json_tg` metadata place only those extra fields in a deterministic
`.tidas-envelope.json` sidecar; reverse conversion consumes the sidecar and
restores the envelope. Root `manifest.json` is package metadata and is copied.
XML 1.0-invalid characters and unknown/missing dataset roots fail as data
issues instead of producing invalid eILCD.

`tidas.conversion-report.v1` records direction, converted/copied/asset/sidecar
counts, input and output bytes, the locked asset fingerprint, cross-platform
output-tree SHA-256, and peak accounted memory. It contains no paths or wall
clock values, so the same input and assets produce the same report summary in
different directories.

## Native import

`tidas-import` detects EcoSpold 1/2, SimaPro CSV, openLCA JSON-LD, openLCA
process XLSX, and ILCD without invoking Python; an explicit `--from-format`
remains available for ambiguous sources. Directory and ZIP traversal is
ordered, rejects symlinks and unsafe archive paths, and applies per-entry size
limits, cancellation checks, and explicit memory reservations.

Adapters stream into a disk-backed canonical store with separate process
exchange spools. The writer publishes a schema-valid TIDAS package and can
bridge the same package to ILCD, validating every requested target before an
atomic directory commit. Per-process dependency bundles are enabled by
default; deterministic gzip mapping CSV is opt-in. Generated identifiers use
portable source-relative keys, so equivalent inputs under different checkout
roots produce the same package, mapping, and bundle hashes.

Flow publication follows one enforced boundary: source adapters capture
evidence, typed Flow normalization resolves name/classification/property
contracts, import preflight rejects missing source facts, the Flow writer only
serializes normalized values, and schema validation is the final gate.
Elementary Flow names require only `baseName`; Product, Waste, and Other Flow
names require source-backed `baseName`, `treatmentStandardsRoutes`, and
`mixAndLocationTypes`. Missing or placeholder qualifiers fail before any
package is staged.

Elementary classifications use the immutable 55-node official ILCD reference
plus the locked `tidas-ef-extension` v1 overlay: ten added nodes represent nine
exact source paths. Deterministic non-exact matching records source evidence;
unmatched elementary paths fall back to air-unspecified with an import warning.
All source flow-property assignments survive normalization. The reference
property is written first, remaining properties retain source order, UUID,
version, and exact decimal factor, and ambiguous multi-property references fail
preflight.

The retained semantic layer includes openLCA unit/property normalization,
allocation, uncertainty, pedigree/data quality, and documentation fields;
EcoSpold 1/2 time, geography, technology, classification, source, and exchange
trace fields; and ILCD versions, contacts, sources, digital-file provenance,
reference properties/exchanges, and elementary compartment categories. Frozen
Python fixtures cover all six adapters, and each Rust replay validates both
TIDAS and ILCD output plus repeated output-tree hashes.

`tidas.import-execution-report.v1` records detection evidence, object and issue
counts, artifact reports, native validation counts, and accounted peak memory.
Detailed issues stream to `issues.jsonl`; source errors, `.zolca`, malformed
input, or generated-package validation failures never publish a partial
output.

## Native validation and rulesets

`tidas-validation` compiles each Draft 7 category schema once per package run.
Relative `$ref` values resolve only against the in-memory embedded schema
catalog; the JSON Schema dependency is built without HTTP or filesystem
resolvers. The `cas-number` format checker is implemented in Rust. The same
pipeline runs localized-language and classification hierarchy checks against
the locked language and product-category projection assets.

`tidas-rulesets` validates `runtime_rulesets.json` against its bundled schema,
checks unique ids and profile-to-rule references, and exposes stable catalog
fingerprints and ordered profile selection.

`tidas-references` reproduces the frozen Python reference golden contract,
including explicit, omitted, and invalid version state; canonical UUID
diagnostics; stable reference roles and JSON paths; and occurrence-preserving
edge order. It deliberately does not resolve targets, visibility, version
winners, closure, or certificates.

Category and file traversal is sorted. File/path memory is explicitly
reserved against the invocation budget, schema errors are consumed as an
iterator, and full issue details are discarded or written immediately to an
atomically persisted JSONL spool. The operation report retains bounded counts,
the asset fingerprint, accounted peak memory, and the spool byte/hash summary
instead of retaining an issue array.

ILCD validation recursively traverses either `<root>/data` or the package root,
skips packaged schema/stylesheet helpers, resolves 12 supported root
namespace/type pairs, and reuses XSD contexts compiled from a temporary
materialization of all locked XSD assets. Relative imports work without
network access; schema diagnostics and flow CAS checksum findings stream into
the same bounded issue contract.

`document-validation-batch.v1` preflights every manifest path and content hash
before emitting evidence, rejects symlinks and non-portable traversal paths,
rehashes around validation, and uses a per-document temporary spool so content
drift cannot publish partial issue evidence. The final event carries counts,
the logical issue-stream hash, and the asset/engine handshake.

## XML/XSD/XSLT portability decision

- `quick-xml` owns strict, streaming, pure-Rust XML inspection.
- `libxml2` through `libxml` owns XSD validation.
- `libxslt` owns the current XSLT 1.0 compatibility layer required by packaged
  eILCD stylesheets.
- native XSD/XSLT calls are serialized behind one process-wide lock because the
  Rust wrapper does not establish safe concurrent schema use.
- development builds dynamically resolve system libraries; release packaging
  uses the pinned vcpkg baseline with static libxml2/libxslt and rejects
  build-machine runtime library paths before packaging.
- production transforms must install a fail-closed resolver before untrusted
  input is accepted; network and arbitrary filesystem resolution are not part
  of the product contract.

This remains the native XSD/XSLT portability boundary. Native format
conversion is implemented by #121 without executing XSLT; release transform
orchestration and production resolver closure remain in #124.

Review note, 2026-07-17: Issue #112 remains inside the existing validation and release-packaging modules. Explicit UTF-8 reads and LF writes make the same packaged assets and report structures byte-stable on Windows; no tool family, asset source, downstream dispatch path, or release architecture changes.

## Stable Path Map

| Path group | Role |
| --- | --- |
| `Cargo.toml`, `Cargo.lock`, `crates/**` | Rust workspace and final product implementation |
| `crates/tidas-dist`, `packaging/**`, `scripts/install.*` | deterministic executable archives, checksum-first installers, and generated Homebrew/Winget metadata |
| `scripts/publish-crates.sh`, `scripts/sync-rust-package-assets.sh` | public crate-set qualification, exact-version publication, checksum-safe retries, and packaged-contract parity |
| `contracts/**` | stable machine-readable Rust report, invocation, conversion, import, export, release, asset-lock, and spool contract schemas |
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
| `src/tidas_tools/release.py` | frozen Python release parity oracle retained until final cutover |
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
| `.github/workflows/release-request.yml` | secret-free request review plus merge-only exact-tag creation and tag-context release dispatch |
| `.github/workflows/rust-release.yml` | five-platform native artifact qualification, SPDX SBOM, GitHub OIDC attestations, clean runtime smoke, metadata validation, and immutable tag publication |

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

`tidas-release` owns the active offline release-control domain used by the
Release control plane, and `tidas release` is its only executable adapter. It
consumes an already finalized canonical dataset tree/index and never assigns
UUIDs or versions. `build-packages` validates TIDAS, resolves exact transitive
references for `unit-process-full-closure.v1` and
`standalone-lifecyclemodel-result-full-closure.v1`, proves the latter contains
the former, derives schema-ordered ILCD, validates ILCD, proves normalized
semantic round-trip, and atomically publishes four stored ZIPs with sorted
members, fixed timestamps, and fixed modes. Reports carry full counts/hashes,
bounded inline samples, explicit truncation flags, and accounted peak memory.
`release.py` is frozen parity evidence only.

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

- `.github/workflows/rust-release.yml` builds Linux x86_64/ARM64, macOS
  Intel/Apple Silicon, and Windows x86_64 from one tag/commit and one pinned
  native dependency baseline
- `.github/releases/v<version>.json` is an append-only release authorization:
  it fixes the exact workspace version and full target commit; its pull request
  has read-only validation and no publication credentials
- merging a valid request to `main` creates or verifies its lightweight tag,
  then uses the allowed `workflow_dispatch` exception at that tag; the native
  release run therefore has the tagged commit as `GITHUB_SHA` and the tag as
  `GITHUB_REF`, including its provenance claims
- every platform archive is built twice and compared byte-for-byte, verified
  by its SHA-256 sidecar, smoke-tested after extraction, scanned into an SPDX
  SBOM, and attested through GitHub OIDC/Sigstore
- tag publication requires `v<workspace-version>`, refuses to modify an
  existing GitHub Release, and publishes only the already-qualified archives
- package `tidas` and all reusable domain crates publish as one exact-version
  crates.io set; `tidas-dist` remains unpublished repository tooling
- pull requests verify every generated crate from its packaged contents and
  perform a multi-package dry-run without credentials; only the tag-context
  release workflow reads `CARGO_REGISTRY_TOKEN`
- partial tag reruns skip only an existing byte-identical crate version; any
  crates.io checksum mismatch fails closed, and GitHub Release publication
  waits for the complete registry set
- `tidas-dist metadata` renders Homebrew and Winget manifests from those exact
  checksum sidecars; external tap creation and Winget community submission
  remain separately approved publication actions
- the transitional Python workflow remains active only until #126 removes the
  frozen oracle and PyPI release path
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
