---
title: tidas-tools Task Router
docType: router
scope: repo
status: active
authoritative: false
owner: tidas-tools
language: en
whenToUse:
  - when you already know the task belongs in tidas-tools but need the right next file or next doc
  - when deciding whether a change belongs in conversion, validation, export, packaged assets, or another repo
  - when routing between tooling work and handoffs to tidas-sdk, tidas, or lca-workspace
whenToUpdate:
  - when new tool families or automation paths appear
  - when cross-repo boundaries change
  - when validation routing becomes misleading
checkPaths:
  - AGENTS.md
  - ai/repo.yaml
  - ai/task-router.md
  - ai/validation.md
  - ai/architecture.md
  - pyproject.toml
  - src/tidas_tools/**
  - tests/**
  - .github/workflows/**
lastReviewedAt: 2026-04-18
lastReviewedCommit: 19d0d16cb047a533c9cc99f422eebff56e038ee4
related:
  - ../AGENTS.md
  - ./repo.yaml
  - ./validation.md
  - ./architecture.md
  - ../README.md
---

## Repo Load Order

When working inside `tidas-tools`, load docs in this order:

1. `AGENTS.md`
2. `ai/repo.yaml`
3. this file
4. `ai/validation.md` or `ai/architecture.md`
5. `README.md` only for user-facing CLI examples

## High-Frequency Task Routing

| Task intent | First code paths to inspect | Next docs to load | Notes |
| --- | --- | --- | --- |
| Change standalone conversion behavior | `src/tidas_tools/convert.py`, then `src/tidas_tools/eilcd/**` or `src/tidas_tools/tidas/**` as needed | `ai/validation.md`, `ai/architecture.md` | Conversion logic belongs here, not in the SDK repo. |
| Change standalone validation behavior | `src/tidas_tools/validate.py`, `src/tidas_tools/validation_report.py`, and `src/tidas_tools/tidas/schemas/**` | `ai/validation.md`, `ai/architecture.md` | Validation semantics and report structure live here. |
| Change export behavior or version normalization | `src/tidas_tools/export.py`, `src/tidas_tools/package_versions.py` | `ai/validation.md`, `ai/architecture.md` | Export-only DB and S3 behavior belongs here. |
| Change TIDAS schemas or methodologies that feed downstream packages | `src/tidas_tools/tidas/schemas/**`, `src/tidas_tools/tidas/methodologies/**` | `ai/architecture.md`, `ai/validation.md` | These paths are the current executable upstream for `tidas-sdk` refreshes. |
| Change eILCD schemas or stylesheets | `src/tidas_tools/eilcd/schemas/**`, `src/tidas_tools/eilcd/stylesheets/**` | `ai/validation.md`, `ai/architecture.md` | These are packaged tooling assets, not the public docs site. |
| Change downstream SDK dispatch behavior | `.github/workflows/dispatch-tidas-sdk-sync.yml` | `ai/validation.md`, `ai/architecture.md` | This repo owns the dispatch contract that tells `tidas-sdk` to refresh. |
| Change generated SDK package surfaces | `tidas-sdk`, not this repo | root `ai/task-router.md` | Generated package output belongs downstream in `tidas-sdk`. |
| Change public spec/docs wording only | `tidas`, not this repo | root `ai/task-router.md` | Public site content belongs in `tidas`. |
| Decide whether work is delivery-complete after merge | root workspace docs, not repo code paths | root `AGENTS.md`, `_docs/workspace-branch-policy-contract.md` | Root integration remains a separate phase. |

## Wrong Turns To Avoid

### Fixing generated SDK output without fixing the upstream asset or script

If the root cause lives in schemas, methodologies, or tooling logic here, fix it here first and then refresh `tidas-sdk`.

### Treating packaged assets as docs-site only content

The asset trees under `src/tidas_tools/**` are executable tooling inputs, not just published reference docs.

### Treating export behavior as part of the SDK

Database export and zip packaging stay in `tidas-tools`.

## Cross-Repo Handoffs

Use these handoffs when work crosses boundaries:

1. upstream tooling change requires SDK refresh
   - start here
   - then update `tidas-sdk`
2. public explanatory docs need to change as well
   - update `tidas`
3. merged repo PR still needs to ship through the workspace
   - return to `lca-workspace`
   - do the submodule pointer bump there
