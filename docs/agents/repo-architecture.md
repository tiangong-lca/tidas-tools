---
title: tidas-tools Architecture Notes
docType: guide
scope: repo
status: active
authoritative: false
owner: tidas-tools
language: en
whenToUse:
  - when building a mental model before changing domain logic, assets, validation, distribution, or dispatch automation
  - when deciding which crate or repository owns a behavior
whenToUpdate:
  - when crate ownership, asset locations, release architecture, or downstream dispatch changes
checkPaths:
  - docs/agents/repo-architecture.md
  - AGENTS.md
  - .docpact/config.yaml
  - Cargo.toml
  - Cargo.lock
  - crates/**
  - contracts/**
  - assets/**
  - packaging/**
  - migration/**
  - .github/workflows/**
  - .githooks/pre-push
  - scripts/**
lastReviewedAt: 2026-08-17
lastReviewedCommit: ddac8d99a4b3435f81d2a3c31e14930e71854ab1
lastReviewedNote: "Issue #153 phase 2 binds the immutable v0.1.3 Release Request to the qualified version-set merge commit without changing crate ownership or release architecture."
related:
  - ../../AGENTS.md
  - ../../.docpact/config.yaml
  - ./cli-contract.md
  - ./repo-validation.md
  - ../../README.md
---

# Repository architecture

## Product shape

The repository builds one executable, `tidas`, with seven top-level commands:
`convert`, `import`, `export`, `validate`, `release`, `ruleset`, and `version`.
The CLI parses and routes; reusable crates own all domain behavior. No
alternate executable or runtime fallback is part of the product.

| Path | Stable responsibility |
| --- | --- |
| `crates/tidas-cli` | unified executable, invocation context, output routing, completion, cancellation wiring, thin dispatch |
| `crates/tidas-contracts` | stable operation reports, diagnostics, artifacts, completeness, exit classes |
| `crates/tidas-runtime` | bounded queues, memory reservations, cancellation, deterministic spools |
| `crates/tidas-conversion` | bidirectional TIDAS JSON/eILCD XML transformation with schema-ordered ILCD output and atomic publication |
| `crates/tidas-import` | format detection, disk-backed canonicalization, TIDAS/ILCD publication, bundles, mapping |
| `crates/tidas-export` | repeatable-read PostgreSQL extraction, S3-compatible streaming, deterministic ZIP |
| `crates/tidas-validation` | offline TIDAS JSON and ILCD/XSD validation, semantic indexes, batch protocol |
| `crates/tidas-release` | exact closure, schema-ordered ILCD derivation, native gates, deterministic release packages |
| `crates/tidas-rulesets` | methodology catalog validation, profile selection, fingerprinting |
| `crates/tidas-references` | side-effect-free reference extraction |
| `crates/tidas-xml` | streaming XML inspection and serialized native XSD/XSLT boundary |
| `crates/tidas-dist` | internal deterministic archive/checksum/smoke/SBOM/package-manager tooling |
| root `tidas-assets` package | embedded assets, paired-schema validation, byte lock, fingerprint |
| `contracts/**` | authoritative stable machine schemas |
| `assets/**` | executable schemas, methodologies, validation indexes, XSD, XSLT, XML references |

## Stable contracts and runtime

Machine contracts use explicit `tidas.*.v1` identifiers, deny unknown fields
at stable typed boundaries, and emit deterministic LF-terminated UTF-8 JSON.
Breaking meaning requires a new schema version. Output does not depend on wall
clock, locale, checkout root, or unordered iteration.

Large-data domains receive one cancellation token, explicit memory budget, and
bounded queue capacity. Unbounded details stream to deterministic disk spools;
operation reports retain bounded summaries and hashes. Publication stages in a
sibling temporary path and commits atomically.

## Domain flow

Conversion traverses sorted package trees, rejects symlinks and invalid XML
text, uses deterministic envelope sidecars for top-level TIDAS metadata, locks
target assets, and reports a cross-platform output-tree hash. TIDAS-to-ILCD
conversion orders every known dataset object from the integrity-locked TIDAS
schema catalog before XML serialization, so JSON member order cannot violate
an ILCD XSD sequence; release conversion reuses the same ordering component.

Import detects EcoSpold 1/2, SimaPro CSV, openLCA JSON-LD, openLCA process
XLSX, and ILCD. Adapters stream into disk-backed canonical entities/exchanges;
typed normalization and preflight run before TIDAS and optional ILCD writers.
Requested outputs are validated before one atomic commit.

Export reads one repeatable-read, read-only PostgreSQL snapshot, streams
records through bounded workers, optionally retrieves S3-compatible object
bodies by chunk, and creates one deterministic archive without serializing
credentials.

Validation resolves only embedded assets. Draft 7 schema resources and ILCD
XSD contexts are compiled offline and reused. Issue details stream or are
discarded; batch evidence preflights content hashes and publishes a final
logical stream hash only after drift-free completion. Schema diagnostics
describe rejected instances through bounded summaries and content hashes
rather than embedding arbitrarily large instance values in JSONL events.

Release consumes finalized UUID/version decisions. It resolves exact
standalone/full closure, derives schema-ordered ILCD, runs native validation and
semantic round-trip gates, and publishes two TIDAS plus two ILCD archives.

## Asset ownership

`assets/tidas/schemas/**` and `assets/tidas/schemas_zh/**` must have identical
file sets and identical structure after removing localized `description`
members. All documents must be valid Draft 7 schemas and every local `$ref`
must resolve inside its language catalog.

`assets/tidas/schema.lock.json` records per-language content and contract hashes.
`assets/asset-lock.v1.json` records every executable asset path, kind, length,
and SHA-256. `cargo run -p tidas-assets --bin tidas-asset-lock -- write`
regenerates the paired lock first and the full lock second; `check` validates
both.

Owned schema/methodology changes may dispatch `tidas-sdk` refresh automation.
Generated SDK code remains downstream and never becomes source of truth here.

## XML/XSD/XSLT portability

- `quick-xml` owns strict streaming inspection.
- `libxml2` through `libxml` owns XSD validation.
- `libxslt` owns XSLT 1.0 compatibility for bundled eILCD stylesheets.
- Native schema/transform calls are serialized behind one process-wide lock.
- Development builds use platform libraries; release builds use pinned static
  inputs and reject build-machine runtime dependency leakage.
- Network and arbitrary filesystem resolution fail closed.

The supported product matrix is Linux x86_64/ARM64, macOS Intel/Apple Silicon,
and Windows x86_64. Windows ARM64 is not supported.

## Distribution and release

Pull requests run Rust CI across the five supported targets, verify reproducible
packages, and qualify the complete crates.io set without credentials. Public
crates share one exact version; `tidas-dist` stays internal.

A reviewed append-only `.github/releases/v<version>.json` binds a native
release to a full source commit. Its merge job creates/verifies the exact tag
and dispatches `rust-release.yml` at that tag. The tag run builds each native
archive twice, compares bytes, verifies checksums, executes packaged probes,
publishes the qualified crates, generates SBOM/attestation evidence, and then
creates the immutable GitHub Release.

Homebrew and Winget metadata derive from the same checksum set. External
package-manager submissions are separate approvals and do not rebuild.

The pre-cutover implementation is retained only in Git history and the tag
declared by `migration/final-python-line.json`; checked-in migration fixtures
remain immutable semantic evidence. They are not an active code, CI,
installation, release, or invocation surface.

## Repository boundaries

- `tiangong-lca/tidas` owns the public specification and human-facing schema
  source.
- `tiangong-lca/tidas-sdk` owns generated SDK packages.
- `tidas-tools` owns executable behavior and packaged runtime assets.
- `lca-workspace` owns multi-repo coordination and exact submodule integration.

A merged tidas-tools PR is not workspace integration. The root pointer must be
updated separately when the tracked delivery requires it.

## Local gate

The versioned pre-push hook runs strict Docpact, the Rust-only audit, paired and
full asset locks, formatting, clippy, and workspace tests. See
`docs/agents/repo-validation.md` for focused and scale proof.
