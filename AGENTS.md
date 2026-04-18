---
title: tidas-tools AI Working Guide
docType: contract
scope: repo
status: active
authoritative: true
owner: tidas-tools
language: en
whenToUse:
  - when a task may change standalone TIDAS conversion, validation, export behavior, or the packaged schema and methodology assets
  - when routing work from the workspace root into tidas-tools
  - when deciding whether a change belongs here, in tidas-sdk, in tidas, or in lca-workspace
whenToUpdate:
  - when tool ownership, runtime prerequisites, or release automation change
  - when the SDK dispatch contract changes
  - when the repo-local AI bootstrap docs under ai/ change
checkPaths:
  - AGENTS.md
  - README.md
  - README_CN.md
  - ai/**/*.md
  - ai/**/*.yaml
  - pyproject.toml
  - src/tidas_tools/**
  - tests/**
  - test_data/**
  - .github/workflows/**
lastReviewedAt: 2026-04-18
lastReviewedCommit: 19d0d16cb047a533c9cc99f422eebff56e038ee4
related:
  - ai/repo.yaml
  - ai/task-router.md
  - ai/validation.md
  - ai/architecture.md
  - README.md
---

## Repo Contract

`tidas-tools` owns standalone TIDAS and eILCD conversion, validation, export behavior, and the packaged schema and methodology assets consumed by downstream SDK refreshes. Start here when the task may change how TIDAS data is validated, converted, exported, or versioned as tooling.

## AI Load Order

Load docs in this order:

1. `AGENTS.md`
2. `ai/repo.yaml`
3. `ai/task-router.md`
4. `ai/validation.md`
5. `ai/architecture.md`
6. `README.md` only for user-facing CLI examples

Do not start with the SDK repo if the change is really about standalone tooling or upstream schema assets.

## Repo Ownership

This repo owns:

- standalone CLI behavior in `src/tidas_tools/convert.py`, `validate.py`, and `export.py`
- packaged schema and methodology assets under `src/tidas_tools/tidas/**`
- packaged eILCD schemas and stylesheets under `src/tidas_tools/eilcd/**`
- version and export helpers such as `package_versions.py` and `validation_report.py`
- tests and the SDK dispatch workflow in `.github/workflows/dispatch-tidas-sdk-sync.yml`

This repo does not own:

- generated SDK package surfaces
- public spec/docs-site presentation
- workspace integration state after merge

Route those tasks to:

- `tidas-sdk` for generated package surfaces and package release automation
- `tidas` for public spec/docs-site content
- `lca-workspace` for root integration after merge

## Runtime Facts

- Python package manager and runner: `uv`
- Canonical local setup: `uv sync --dev`
- Canonical local test command: `uv run pytest`
- Common manual tool probes:
  - `uv run python src/tidas_tools/convert.py --help`
  - `uv run python src/tidas_tools/validate.py --help`
  - `uv run python src/tidas_tools/export.py --help`
- Release is tag-driven through `v<version>` tags

## Hard Boundaries

- Do not move standalone conversion, validation, or export logic into `tidas-sdk`
- Do not treat the public docs site as the executable upstream for packaged schemas and methodologies
- Do not treat a merged repo PR here as workspace-delivery complete if the root repo still needs a submodule bump

## Workspace Integration

A merged PR in `tidas-tools` is repo-complete, not delivery-complete.

If the change must ship through the workspace:

1. merge the child PR into `tidas-tools`
2. update the `lca-workspace` submodule pointer deliberately
3. complete any later workspace-level validation that depends on the updated tooling snapshot
