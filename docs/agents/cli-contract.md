---
title: Unified tidas CLI Contract
docType: contract
scope: repo
status: active
authoritative: true
owner: tidas-tools
language: en
whenToUse:
  - when adding or changing a tidas command, global option, output mode, exit class, or completion script
  - when a downstream system parses tidas JSON or invokes the executable
  - when deciding whether a behavior belongs in the CLI adapter or a reusable domain crate
whenToUpdate:
  - when the public command tree, invocation context, configuration precedence, output channels, or runtime controls change
checkPaths:
  - docs/agents/cli-contract.md
  - Cargo.toml
  - Cargo.lock
  - crates/tidas-cli/**
  - crates/tidas-conversion/**
  - crates/tidas-import/**
  - crates/tidas-contracts/**
  - crates/tidas-runtime/**
  - contracts/**
  - README.md
  - README_CN.md
lastReviewedAt: 2026-07-26
lastReviewedCommit: 812f9f4
lastReviewedNote: "Reviewed for Issue #122 Windows bundle-publication hardening; the public command, report, output, warning, and exit-class contracts remain unchanged."
related:
  - ../../AGENTS.md
  - ../../.docpact/config.yaml
  - ./repo-architecture.md
  - ./repo-validation.md
  - ../../contracts/operation-report.v1.schema.json
---

# Unified `tidas` CLI Contract

## Product surface

The product ships one executable, `tidas`, with exactly seven top-level
commands:

- `convert`
- `import`
- `export`
- `validate`
- `release`
- `ruleset`
- `version`

Shell completion generation is a global action, not an eighth command:

```bash
tidas --completion bash > tidas.bash
```

The old Python executable names are not aliases. An incomplete Rust functional
slice returns `unavailable` (69) and never invokes Python.

## Adapter boundary

`crates/tidas-cli` owns parsing, configuration selection, invocation context,
process cancellation wiring, output routing, help, and completions. It does
not own conversion, import, export, validation, release, or ruleset domain
logic. Functional commands receive a cancellation token, an explicit memory
budget, a bounded queue capacity, and typed inputs before calling reusable
domain crates.

## Configuration precedence

The deterministic precedence order is:

1. explicit command-line option
2. matching `TIDAS_*` environment variable
3. documented built-in default

The runtime never searches the current directory or a home directory for an
implicit configuration file. `--config <PATH>` overrides `TIDAS_CONFIG`.
Configuration selection is recorded in `tidas.invocation-context.v1`.

| Option | Environment | Default |
| --- | --- | --- |
| `--config <PATH>` | `TIDAS_CONFIG` | none |
| `--log-level <LEVEL>` | `TIDAS_LOG` | `warn` |
| `--progress <MODE>` | `TIDAS_PROGRESS` | `auto` |
| `--memory-budget-mib <MIB>` | `TIDAS_MEMORY_BUDGET_MIB` | `512` |
| `--queue-capacity <COUNT>` | `TIDAS_QUEUE_CAPACITY` | `256` |

Zero memory budgets and queue capacities are usage errors.

## Streams and files

- stdin is used only when a future functional command explicitly receives `-`
  in a documented input option; it is never selected implicitly.
- stdout contains exactly one human report, one canonical JSON report, or one
  completion script.
- logs, progress, diagnostics outside a completed report, and file-write
  confirmations use stderr.
- `--report <PATH>` writes the complete report to a temporary sibling and then
  renames it into place; stdout remains empty.
- command-owned large artifacts use command-specific `--output` options,
  so the global report path intentionally uses `--report`.
- JSON mode never mixes logs, banners, or progress with stdout.

`--progress auto` enables progress only for human output attached to a terminal.
Native conversion and package/batch validation emit bounded start, periodic
file/document counts, and completion updates on stderr. `--progress always`
enables the same updates in JSON mode without contaminating stdout;
`--progress never` disables them.

## Machine contracts

`tidas.operation-report.v1` is the F3 envelope for every completed command
dispatch. Its optional `invocation` member is
`tidas.invocation-context.v1` and records:

- configuration source and selected path
- log and progress policy
- resolved progress enablement
- memory budget in bytes
- bounded queue capacity
- explicit-path-or-dash input policy
- report and diagnostic destinations

The report envelope, command names, diagnostics, artifacts, completeness, and
exit classes are F3 versioned contracts. The ordered `summary` object remains
an F2 extension point while individual domain slices are still discovering
their stable projections. Fields may be added compatibly; removal or semantic
change requires a new schema version.

Canonical JSON is UTF-8, LF-terminated, deterministic for identical inputs,
and contains no implicit timestamps, locale values, or unordered collections.

## Native conversion surface

Bidirectional package conversion uses:

```bash
tidas convert <INPUT_DIR> --output <OUTPUT_DIR> --to ilcd --format json
tidas convert <INPUT_DIR> --output <OUTPUT_DIR> --to tidas --format json
```

The command never infers direction. It mirrors input under `OUTPUT_DIR/data`,
copies non-domain package metadata, materializes the locked target assets, and
publishes the whole directory atomically. Symlinks, malformed JSON/XML,
multiple unknown roots, XML 1.0-invalid text, and malformed envelope sidecars
are data issues; nested output is a usage error; missing paths and commit
failures use the I/O class; cancellation uses 130.

The operation report summary contains one `conversion` member conforming to
`tidas.conversion-report.v1`. Its artifact is the output directory with total
bytes and a cross-platform tree SHA-256. A deterministic
`.tidas-envelope.json` sidecar preserves top-level TIDAS extension fields that
cannot appear beside the single eILCD XML root; reverse conversion consumes
and merges it. The report next action gives the exact `tidas validate
OUTPUT/data --input-format ...` command.

## Native import surface

External-format import uses:

```bash
tidas import <INPUT> --output <OUTPUT_DIR> --format json
tidas import <INPUT> --output <OUTPUT_DIR> \
  --from-format openlca-jsonld --target both \
  --write-mapping --format json
```

Supported `--from-format` values are `ecospold1`, `ecospold2`, `simapro-csv`,
`openlca-jsonld`, `openlca-process-xlsx`, and `ilcd`. Without that option,
bounded signature detection selects the format or returns a data issue for
unsupported/ambiguous input. `.zolca` is explicitly rejected. `--target`
defaults to `tidas`; `ilcd` and `both` request the validated ILCD bridge.
Per-process dependency bundles are written by default and
`--no-process-bundles` disables them. `--write-mapping` enables deterministic
`mapping.csv.gz`; `--max-entry-mib` limits each source entry.

The operation report summary contains one `import` member conforming to
`tidas.import-execution-report.v1`. Artifacts carry directory/file hashes and
byte counts, and next actions give exact native `tidas validate` commands for
the published targets. Source data findings, malformed input, and `.zolca`
return `data-issues`; nested output and invalid limits are usage errors;
required path/publication failures use I/O; cancellation uses 130.
`--fail-on-warning` converts an otherwise successful import with warnings to
`data-issues` without discarding the published, validated artifacts.

Adapters write to a disk-backed canonical store; exchanges and issues stream
to bounded spools. Requested TIDAS/ILCD outputs, process bundles, mapping CSV,
and reports are assembled in a sibling staging directory and become visible
only through one atomic commit.

## Native validation and ruleset surfaces

The first production domain path is:

```bash
tidas validate <PACKAGE_DIR> \
  --input-format tidas-json \
  --issues <ISSUES.jsonl> \
  --format json
```

`--input-format` is `tidas-json` by default; `ilcd-xml` selects native
namespace-aware XSD validation. `--issues` is optional. When present, every complete issue is written
in deterministic order to an atomically persisted
`tidas.validation-issue-event.v1` JSONL artifact. Without it, issues are
counted and discarded after validation so report memory remains bounded.

The operation report summary contains one `validation` member conforming to
`tidas.validation-summary.v1`. Data issues produce `completed-with-issues`
and exit 2. Missing input/spool paths use the I/O exit class; cancellation uses
130. The summary records category/document/issue counts, the locked asset
fingerprint, accounted peak memory, and the optional spool count/bytes/hash.

Worker-facing document batches use:

```bash
tidas validate <BATCH_DIR> \
  --protocol document-validation-batch.v1 \
  --input-manifest <MANIFEST.jsonl> \
  --events <EVENTS.jsonl> \
  --format json
```

The manifest is fully preflighted before evidence is emitted. Data findings
are a successful protocol run (exit 0) whose final event proves counts,
logical issue-stream hash, and validation fingerprints. Unsafe paths,
symlinks, missing files, hash drift, or malformed protocol input fail without
a final event. `tidas validate --describe --format json` returns the supported
protocol/profile and engine/asset handshake.

`tidas ruleset --format json` validates and returns the packaged methodology
catalog. `tidas ruleset --id <RULESET_ID> --format json` returns its ordered
rules; unknown ids use the usage exit class.

## Exit classes

| Class | Code | Meaning |
| --- | ---: | --- |
| `success` | 0 | operation completed successfully |
| `data-issues` | 2 | operation completed and found domain data issues |
| `usage` | 64 | command syntax or option value is invalid |
| `unavailable` | 69 | known Rust function is not yet available |
| `internal` | 70 | invariant, setup, or internal serialization failed |
| `io` | 74 | report or required I/O failed |
| `cancelled` | 130 | shared cancellation token stopped the operation |

Clap usage/help text is written to stderr for malformed invocation. Successful
help, version flags, and completion generation exit zero.

## Validation contract

Public CLI changes must prove:

- exactly seven top-level product commands and no legacy aliases
- help for the root and every product command
- deterministic completions for Bash, Elvish, Fish, PowerShell, and Zsh
- configuration precedence and invocation-context fields
- clean stdout for JSON, report-file, and completion modes
- deterministic repeated JSON
- all affected exit classes
- migration parity fixtures whose semantics are sourced from the frozen Python
  oracle without preserving legacy command names or flag layouts
- Rust 1.88 fmt, clippy, tests, and the five-platform CI matrix
