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
  - .docpact/**/*.yaml
  - docs/agents/**
  - pyproject.toml
  - src/tidas_tools/**
  - tests/**
  - test_data/**
  - .github/workflows/**
lastReviewedAt: 2026-04-24
lastReviewedCommit: d71f5dfa68aa49e10860ba6912a7c7ee91509989
related:
  - .docpact/config.yaml
  - docs/agents/repo-validation.md
  - docs/agents/repo-architecture.md
  - README.md
  - README_CN.md
---

## Repo Contract

`tidas-tools` owns standalone TIDAS and eILCD conversion, validation, export behavior, and the packaged schema and methodology assets consumed by downstream SDK refreshes. Start here when the task may change how TIDAS data is validated, converted, exported, or versioned as tooling.

## Documentation Roles

| Document | Owns | Does not own |
| --- | --- | --- |
| `AGENTS.md` | repo contract, branch and delivery rules, hard boundaries, minimal execution facts | full path map, proof matrix, or long setup prose |
| `.docpact/config.yaml` | machine-readable repo facts, routing intents, governed-doc rules, ownership, coverage, and freshness | explanatory prose or long-form walkthroughs |
| `docs/agents/repo-validation.md` | minimum proof by change type, manual CLI probes, PR validation note shape | repo contract, branch policy truth, or architecture explanations |
| `docs/agents/repo-architecture.md` | compact tool topology, stable path map, upstream asset chain, dispatch and release model | checklist-style proof guidance or user-facing CLI docs |
| `README.md` | English user-facing CLI examples and basic development commands | machine-readable routing or lint semantics |
| `README_CN.md` | Chinese user-facing CLI examples and basic development commands | machine-readable routing or lint semantics |

## Load Order

Read in this order:

1. `AGENTS.md`
2. `.docpact/config.yaml`
3. `docs/agents/repo-validation.md` or `docs/agents/repo-architecture.md`
4. `README.md` or `README_CN.md` only for user-facing CLI examples

## Operational Pointers

- path-level ownership, routing intents, governed-doc inventory, and lint rules live in `.docpact/config.yaml`
- minimum proof and manual CLI probe guidance live in `docs/agents/repo-validation.md`
- stable path groups, upstream asset handoffs, and release / dispatch topology live in `docs/agents/repo-architecture.md`
- repo-local documentation maintenance is enforced by `.github/workflows/ai-doc-lint.yml` with `docpact lint`
- the main routing intents are `tool-runtime`, `conversion`, `validation`, `export`, `packaged-assets`, `sdk-dispatch`, `release`, `proof`, `repo-docs`, and `root-integration`

## Minimal Execution Facts

Keep these entry-level facts in `AGENTS.md`. Use `README.md`, `README_CN.md`, and `docs/agents/repo-validation.md` for fuller command detail.

- Python package manager and runner: `uv`
- routine branch base: `main`
- routine PR base: `main`
- canonical setup: `uv sync --dev`
- canonical local test command: `uv run pytest`
- common manual probes:
  - `uv run python src/tidas_tools/convert.py --help`
  - `uv run python src/tidas_tools/validate.py --help`
  - `uv run python src/tidas_tools/export.py --help`
- release tag pattern: `v<version>`

## Ownership Boundaries

The authoritative path-level ownership map lives in `.docpact/config.yaml`.

At a human-readable level, this repo owns:

- standalone CLI behavior in `src/tidas_tools/convert.py`, `src/tidas_tools/validate.py`, and `src/tidas_tools/export.py`
- validation report and version/export helpers in `src/tidas_tools/validation_report.py` and `src/tidas_tools/package_versions.py`
- packaged TIDAS schemas and methodologies under `src/tidas_tools/tidas/**`
- packaged eILCD schemas and stylesheets under `src/tidas_tools/eilcd/**`
- tests and automation under `tests/**`, `.github/workflows/dispatch-tidas-sdk-sync.yml`, and `.github/workflows/python-package-deploy.yml`
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
- do not treat the public docs site as the executable upstream for packaged schemas and methodologies
- packaged assets under `src/tidas_tools/**` are executable tooling inputs, not just reference docs
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
