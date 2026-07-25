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
  - crates/tidas-contracts/**
  - crates/tidas-runtime/**
  - contracts/**
  - README.md
  - README_CN.md
lastReviewedAt: 2026-07-25
lastReviewedCommit: 8e0a5e39342403c8b38da530ff3c776fd729765e
lastReviewedNote: "Issue #119 defines the unified command, invocation, output, completion, cancellation, and bounded-runtime contract."
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
- future command-owned large artifacts use command-specific `--output` options,
  so the global report path intentionally uses `--report`.
- JSON mode never mixes logs, banners, or progress with stdout.

`--progress auto` enables progress only for human output attached to a terminal.
No current placeholder command emits progress.

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
