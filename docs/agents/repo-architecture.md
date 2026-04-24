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
  - pyproject.toml
  - src/tidas_tools/**
  - .github/workflows/**
lastReviewedAt: 2026-04-24
lastReviewedCommit: d71f5dfa68aa49e10860ba6912a7c7ee91509989
related:
  - ../../AGENTS.md
  - ../../.docpact/config.yaml
  - ./repo-validation.md
  - ../../README.md
---

## Repo Shape

This repo packages standalone tooling plus the schema, methodology, and stylesheet assets those tools execute against.

## Stable Path Map

| Path group | Role |
| --- | --- |
| `src/tidas_tools/convert.py` | standalone conversion CLI |
| `src/tidas_tools/validate.py` | standalone validation CLI |
| `src/tidas_tools/validation_report.py` | structured validation-report rendering |
| `src/tidas_tools/export.py` | standalone export CLI |
| `src/tidas_tools/package_versions.py` | version normalization and export package metadata logic |
| `src/tidas_tools/tidas/**` | packaged TIDAS schemas and methodologies |
| `src/tidas_tools/eilcd/**` | packaged eILCD schemas and stylesheets |
| `tests/**` | automated repo tests |
| `.github/workflows/dispatch-tidas-sdk-sync.yml` | downstream SDK refresh dispatch contract |
| `.github/workflows/python-package-deploy.yml` | tag-driven PyPI publish workflow |

## Current Tool Families

### Conversion

`convert.py` owns standalone TIDAS and eILCD conversion behavior.

### Validation

`validate.py` plus `validation_report.py` own standalone validation semantics and structured reporting.

### Export

`export.py` plus `package_versions.py` own database export semantics, version normalization, and archive packaging.

## Upstream Asset Chain

The practical executable chain today is:

`tidas-tools -> tidas-sdk`

Important consequences:

- `tidas-sdk` runtime assets and generated models depend on the packaged assets here
- schema or methodology changes here usually imply downstream SDK follow-up
- `tidas` remains the public docs surface, but it is not the executable upstream for tooling behavior

## Release And Dispatch Architecture

- tag `v<version>` publishes `tidas-tools`
- changes under packaged schema and methodology paths can dispatch downstream SDK refresh workflows

This dispatch path is part of the repo architecture, not just a convenience automation.

## Common Misreads

- public docs changes do not replace executable tool changes here
- generated SDK output is downstream, not the upstream source of truth
- a merged child PR does not finish workspace delivery
