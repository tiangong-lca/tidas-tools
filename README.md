---
title: tidas-tools README
docType: guide
scope: repo
status: active
authoritative: false
owner: tidas-tools
language: en
whenToUse:
  - when you need English user-facing CLI examples, native installation, or basic development commands
whenToUpdate:
  - when English CLI examples, installation, development commands, or release notes change
checkPaths:
  - README.md
  - AGENTS.md
  - .docpact/config.yaml
  - docs/agents/**
  - Cargo.toml
  - crates/**
  - packaging/**
  - scripts/install.*
  - .github/workflows/**
lastReviewedAt: 2026-07-26
lastReviewedCommit: 84dc90f
lastReviewedNote: "Issue #125 documents five-platform native artifacts, reproducible archives, checksums, SBOM/attestation, installers, and package-manager metadata."
related:
  - AGENTS.md
  - .docpact/config.yaml
  - docs/agents/repo-validation.md
  - docs/agents/repo-architecture.md
  - README_CN.md
---

# TianGong TIDAS Tools User Guide

[![PyPI](https://img.shields.io/pypi/v/tidas-tools.svg)][pypi status]
[![Python Version](https://img.shields.io/pypi/pyversions/tidas-tools)][pypi status]

[pypi status]: https://pypi.org/project/tidas-tools/

[English](https://github.com/tiangong-lca/tidas-tools/blob/main/README.md) | [中文](https://github.com/tiangong-lca/tidas-tools/blob/main/README_CN.md)

This repository is migrating its conversion, import, export, validation,
release, and ruleset behavior to one cross-platform Rust executable named
`tidas`.

## Rust migration preview

The current Rust implementation establishes the Cargo workspace, stable
machine and invocation contracts, bounded runtime primitives, executable-asset
integrity lock, XML/XSD/XSLT portability boundary, the unified CLI adapter,
and native TIDAS/ILCD validation, reference extraction, batch evidence,
ruleset inspection, bidirectional TIDAS/eILCD conversion, external-format
import, database export, deterministic release control, and reproducible native
distribution:

```bash
cargo build --workspace
cargo run -p tidas-cli --bin tidas -- --help
cargo run -p tidas-cli --bin tidas -- --format json version
cargo run -p tidas-cli --bin tidas -- convert <tidas-package-dir> \
  --output <eilcd-package-dir> --to ilcd --format json
cargo run -p tidas-cli --bin tidas -- convert <eilcd-data-dir> \
  --output <tidas-package-dir> --to tidas --format json
cargo run -p tidas-cli --bin tidas -- import <source-file-or-dir> \
  --output <import-output-dir> --target both --write-mapping --format json
cargo run -p tidas-cli --bin tidas -- export \
  --output <package.zip> --skip-external-docs --format json
cargo run -p tidas-cli --bin tidas -- validate <package-dir> \
  --issues <issues.jsonl> --format json
cargo run -p tidas-cli --bin tidas -- validate <ilcd-dir> \
  --input-format ilcd-xml --issues <issues.jsonl> --format json
cargo run -p tidas-cli --bin tidas -- release build-packages \
  --tidas-dir <canonical-tidas-dir> \
  --dataset-index <canonical-dataset-index.json> \
  --output-dir <release-dir> --format json
cargo run -p tidas-cli --bin tidas -- ruleset --format json
cargo run -p tidas-cli --bin tidas -- --completion bash > tidas.bash
cargo run -p tidas-assets --bin tidas-asset-lock -- check
cargo run -p tidas-dist -- version
```

The final command tree is `convert`, `import`, `export`, `validate`, `release`,
`ruleset`, and `version`. All seven commands are implemented in Rust and none
invokes Python.

Native import accepts EcoSpold 1/2, SimaPro CSV, openLCA JSON-LD, openLCA
process XLSX, and ILCD files, directories, or ZIP packages. It detects the
source format by default; use `--from-format` to resolve ambiguous inputs.
The command always writes and validates TIDAS internally, optionally publishes
ILCD with `--target ilcd|both`, writes per-process dependency bundles by
default, and enables deterministic `mapping.csv.gz` with `--write-mapping`.
`.zolca` is rejected. Parsing, exchanges, and issue reporting use bounded,
cancel-aware, disk-backed streams, and no partial output is published on
failure.

Native conversion mirrors input under `OUTPUT/data`, preserves package
metadata, materializes the locked target schemas/stylesheets/methodologies,
and publishes the entire output directory atomically. TIDAS documents with
top-level extension metadata use deterministic `.tidas-envelope.json`
sidecars so eILCD remains single-root XML and the reverse conversion restores
the original envelope. Traversal rejects symlinks and XML 1.0-invalid
characters; repeated successful runs report the same output-tree SHA-256.

Native export reads active PostgreSQL records from one repeatable-read,
read-only snapshot, streams them through a bounded queue, normalizes TIDAS
package versions, optionally streams S3-compatible external documents, and
publishes one deterministic ZIP atomically. Set `TIDAS_DATABASE_URL`; storage
credentials are accepted only through `TIDAS_S3_ACCESS_KEY_ID`,
`TIDAS_S3_SECRET_ACCESS_KEY`, and optional `TIDAS_S3_SESSION_TOKEN`. Credential
values never appear in reports or diagnostics.

Native validation resolves only embedded integrity-locked schemas. Complete
issues can be written atomically as deterministic JSONL with `--issues`; the
operation report retains bounded counts and the spool hash instead of an
in-memory issue array. ILCD XML uses the same bounded report contract with
offline reusable XSD contexts. `document-validation-batch.v1` adds manifest
preflight, drift-proof issue events, and a deterministic final evidence hash.
Validation progress is bounded and written only to stderr; use
`--progress always` for non-interactive runs.

Global runtime options follow `CLI > TIDAS_* environment > built-in default`
precedence. No configuration file is loaded implicitly. Stdout contains only
the requested human/JSON report or completion script; logs, progress,
diagnostics, and report-file confirmations use stderr. Use `--report <PATH>`
to persist the report without occupying stdout. The default accounted memory
budget is 512 MiB and the default bounded queue capacity is 256. The normative
contract is [docs/agents/cli-contract.md](docs/agents/cli-contract.md).

## Native distribution

The native release workflow qualifies one exact `tidas` binary for Linux
x86_64/ARM64, macOS Intel/Apple Silicon, and Windows x86_64. It builds every
archive twice, compares the bytes, verifies SHA-256, runs packaged `version`,
help, JSON `version`, and `ruleset` probes, generates an SPDX SBOM, and creates
GitHub OIDC provenance/SBOM attestations. Pinned static libxml2/libxslt inputs
keep the archives independent of Homebrew, vcpkg, Python, Java, Node.js, or a
development toolchain at runtime.

After a native version is published, install an explicit immutable version:

```bash
curl --proto '=https' --tlsv1.2 -fsSLO \
  https://raw.githubusercontent.com/tiangong-lca/tidas-tools/main/scripts/install.sh
sh install.sh --version 0.1.0 --prefix "$HOME/.local"
```

```powershell
.\scripts\install.ps1 -Version 0.1.0
```

Every GitHub Release also carries generated Homebrew formula and Winget
manifests that reference the same archive hashes. External tap creation or a
Winget community submission is a separate publication approval; those paths
never rebuild the executable. Windows ARM64 is a tracked second-phase target.

The Python package described below is feature-frozen and remains temporarily
available as an internal golden/parity oracle. It is not the final product and
its legacy executable names and argument layout will not be preserved. Python
will be removed only after Rust functional parity, deterministic output,
performance/RSS targets, initial cross-platform artifacts, downstream
cutovers, and workspace cleanup have all passed. Progress is tracked in
[Issue #117](https://github.com/tiangong-lca/tidas-tools/issues/117).

---

## Frozen Python oracle reference

The following documentation describes the transitional Python oracle used to
verify migration parity.

---

## 1. Oracle scope

This toolkit contains these independent tools:

- **TIDAS and eILCD Data Format Conversion Tool**
- **External LCA Format Import Tool**
- **TIDAS and eILCD/ILCD Data Validation Tool**
- **TIDAS and eILCD Data Export Tool**

---

## 2. TIDAS and eILCD Data Format Conversion Tool Usage

### (1) Installation Instructions

```bash
# Install this toolkit
pip install tidas-tools
```

### (2) Tool Functionalities

This tool supports mutual conversion between the following two data formats:

- TIDAS data format → eILCD data format (default mode)
- eILCD data format → TIDAS data format

### (3) Command-line Arguments

| Argument | Short form | Description |
|----------|------------|-------------|
| `--help` | `-h` | Display help message |
| `--input-dir` | `-i` | Directory containing data files to be converted (note: this directory must directly contain the data files, not their parent directory) |
| `--output-dir` | `-o` | Output directory for converted data (the program will automatically generate the complete schema-compatible directory structure) |
| `--to-eilcd` | | Convert data from TIDAS format to eILCD format (default mode) |
| `--to-tidas` | | Convert data from eILCD format to TIDAS format |
| `--verbose` | `-v` | Enable verbose logging |

### (4) Usage Examples

```bash
# Convert TIDAS data to eILCD format
tidas-convert --input-dir <TIDAS_data_directory> --output-dir <eILCD_output_directory> --to-eilcd

# Convert eILCD data to TIDAS format
tidas-convert --input-dir <eILCD_data_directory> --output-dir <TIDAS_output_directory> --to-tidas
```

---

## 3. External LCA Format Import Tool Usage

### (1) Current Scope

`tidas-import` is the staged entry point for importing external LCA formats into TIDAS and optionally ILCD/eILCD. The current implementation provides CLI dispatch, source format detection, `.zolca` rejection, machine-readable conversion reports, and minimal validated adapters for openLCA JSON-LD, EcoSpold 1, SimaPro CSV, EcoSpold 2, and openLCA process XLSX.

Current source status:

- openLCA JSON-LD zip/directory: minimal import to TIDAS and ILCD/eILCD
- EcoSpold 1 XML/zip: minimal import to TIDAS and ILCD/eILCD
- SimaPro CSV block format: minimal import to TIDAS and ILCD/eILCD
- EcoSpold 2 `.spold`/zip: minimal import to TIDAS and ILCD/eILCD
- openLCA process XLSX: minimal import to TIDAS and ILCD/eILCD

`.zolca` is intentionally out of scope.

Imported JSON-LD actors and sources are written as TIDAS contacts and sources.
Source units from EcoSpold, SimaPro CSV, and process XLSX inputs are propagated
into generated unit groups and flow properties when no explicit reference data
is available.

When downstream AI/import workers need to handle each TIDAS process
independently, the importer writes per-process bundles by default. The normal
`<output_directory>/tidas` package is still written unchanged; the importer
also writes
`<output_directory>/process-bundles/<process_uuid>/` folders containing the
process JSON plus referenced flow, flow property, unit group, contact, and
source JSON files. `--process-bundles-dir <dir>` overrides the bundle location,
and `--no-process-bundles` disables bundle output.

The expert mapping CSV is disabled by default because large imports can produce
very large field-level mapping files. Use `--write-mapping-csv` to write
`<output_directory>/mapping.csv.gz`.

### (2) Usage Example

```bash
tidas-import --input <source_file_or_dir> --output-dir <output_directory> --detect-only
tidas-import --input <source_file_or_dir> --output-dir <output_directory> --target both --validation-jobs 0
tidas-import --input <source_file_or_dir> --output-dir <output_directory> --no-process-bundles
tidas-import --input <source_file_or_dir> --output-dir <output_directory> --write-mapping-csv
```

---

## 4. Deterministic TIDAS/ILCD Release Packaging

`tidas release` consumes a finalized canonical TIDAS dataset tree plus its `tiangong.release.canonical-dataset-index.v1`. It never assigns UUIDs or versions. The reusable Rust release domain validates exact transitive references, derives and validates schema-ordered ILCD, checks normalized semantic round-trip, and builds the two self-contained release profiles with byte-stable ZIP metadata.

```bash
tidas release validate-tidas --input-dir <canonical-tidas-dir>
tidas release convert-ilcd --input-dir <canonical-tidas-dir> --output-dir <ilcd-dir>
tidas release validate-ilcd --input-dir <ilcd-dir>
tidas release semantic-roundtrip --tidas-dir <canonical-tidas-dir> --ilcd-dir <ilcd-dir>
tidas release validate-closure \
  --input-dir <canonical-tidas-dir> \
  --dataset-index <canonical-dataset-index.json> \
  --profile unit-process-full-closure.v1
tidas release build-packages \
  --tidas-dir <canonical-tidas-dir> \
  --dataset-index <canonical-dataset-index.json> \
  --output-dir <package-dir> \
  --format json
```

The package command runs all native validation, conversion, closure, containment, and round-trip gates before atomically publishing exactly four archives: canonical TIDAS and derived ILCD variants for `unit-process-full-closure.v1` and `standalone-lifecyclemodel-result-full-closure.v1`. Missing UUID/version references fail closed. Members are sorted with fixed timestamps and permissions. JSON stdout conforms to `tidas.release-report.v1`; `--report <path>` atomically persists the same operation report instead.

---

## 5. TIDAS and eILCD/ILCD Data Validation Tool Usage

### (1) Tool Functionalities

This tool validates whether TIDAS JSON data or eILCD/ILCD XML data complies with the packaged schema standards. TIDAS JSON validation uses a compiled schema fast path and falls back to complete error collection when a schema issue is found.

### (2) Unified CLI Arguments

| Argument | Short form | Description |
|----------|------------|-------------|
| `--help` | `-h` | Display help message |
| `<INPUT>` | | Directory containing the package or batch documents |
| `--input-format` | | Input format: `tidas-json` (default) or `ilcd-xml` |
| `--issues` | | Persist deterministic package issue events as JSONL |
| `--describe --format json` | | Report supported validation protocols and package/engine/Schema-lock fingerprints |
| `--protocol document-validation-batch.v1` | | Validate exactly the JSONL manifest documents and stream issue/final events |
| `--input-manifest` | | Batch JSONL manifest containing opaque document keys, safe relative paths, exact identities, and SHA-256 hashes |
| `--events` | | Persist deterministic batch issue/final events as JSONL |

### (3) Usage Example

```bash
# Validate TIDAS data format
tidas validate <TIDAS_data_directory> --input-format tidas-json --format json

# Validate eILCD/ILCD XML data format
tidas validate <eILCD_data_directory> --input-format ilcd-xml --format json

# Inspect the reproducibility handshake used by closure-preflight workers
tidas validate --describe --format json

# Stream deterministic validation evidence for exactly the manifest documents
tidas validate <batch_root> \
  --protocol document-validation-batch.v1 \
  --input-manifest <document-validation-batch.v1.jsonl> \
  --events <validation-events.jsonl> \
  --format json

# Inspect or select the integrity-locked native ruleset catalog
tidas ruleset --format json
tidas ruleset --id process-authoring/strict --format json
```

The batch protocol treats document issues as a completed scan: it emits one
`issue` event per finding, a final summary/hash event, and exits zero. Unsafe
paths, duplicate keys/paths, symlinks, content-hash drift, malformed input, or
missing execution proof are protocol failures. Reference target existence and
database visibility are intentionally outside this document-validation layer.

## 6. TIDAS Export Tool Documentation

### (1) Tool Functionalities

This tool exports data records in either TIDAS or eILCD format. It also optionally downloads supplementary files and bundles them into a final zip archive.

### (2) Command-line Arguments and Environment Variables

| Parameter                 | Short | Description                                     |
|---------------------------|-------|-------------------------------------------------|
| `--help`                  | `-h`  | Display help information                        |
| `--to-tidas`              | -     | Export data in TIDAS format (default)           |
| `--to-eilcd`              | None  | Export data in eILCD format                     |
| `--input-dir`             | `-i`  | Input directory containing files to export      |
| `--output-zip`            | `-z`  | Output path for the zip file                    |
| `--env-file`              | `-e`  | Path to .env file containing DB and AWS credentials|
| `--skip-external-docs`    |       | Skip downloading external supplementary files   |
| `--to-tidas`              |       | Export in TIDAS format (default option)         |
| `--to-eilcd`              |       | Export in eILCD format (mutually exclusive)     |
| `--db-user`               |       | Database username                               |
| `--db-password`           |       | Database password                               |
| `--db-host`               |       | Database host                                   |
| `--db-port`               |       | Database port (default: 5432)                   |
| `--db-name`               |       | Database name                                   |
| `--aws-access-key-id`     |       | AWS access key ID                               |
| `--aws-secret-access-key` |       | AWS secret access key                           |
| `--aws-region`            |       | AWS region                                      |
| `--verbose`               | `-v`  | Enable verbose logging                          |

Credentials can also be set via environment variables (defaults to the .env file in the current directory):

```env
DB_USER=
DB_PASSWORD=
DB_HOST=
DB_PORT=5432
DB_NAME=postgres
AWS_REGION=
AWS_ENDPOINT=
```

### (3) Usage Example

```bash
# Export records to TIDAS format and produce a ZIP archive.
tidas-export -i <TIDAS_input_directory> -z <TIDAS_ZIP_File> --to-tidas

# Export records to eILCD format without downloading supplementary files
tidas-export -z <eILCD_ZIP_File> --to-eilcd --skip-external-docs
```

---

## 7. Log File Information

Both data conversion and validation tools will automatically generate execution logs. The log file name is:

```
tidas-{function_name}.log
```

---

## 8. Development Environment Setup and Contribution Guide

If you wish to participate in development, you can set up your environment following these steps:

### (1) Ubuntu System Environment Preparation

```bash
# Update repositories and install software management tools
sudo apt update
sudo apt install software-properties-common

# Add the official PPA repository for the latest Python version and install Python 3.12
sudo add-apt-repository ppa:deadsnakes/ppa
sudo apt install -y python3.12

# Install necessary dependency packages
sudo apt install libxml2-dev libxslt-dev
sudo apt-get install build-essential python3-dev

# Upgrade software packages on the system
sudo apt upgrade
```

### (2) Manage Python Environment with uv

```bash
# Install uv (if not already available)
curl -LsSf https://astral.sh/uv/install.sh | sh

# Synchronize dependencies (including development tools)
uv sync --dev

# Activate the virtual environment created by uv (optional)
source .venv/bin/activate

# Run project commands without activating the environment
uv run python src/tidas_tools/convert.py --help
```

---

## 9. Code Standards and Testing

### (1) Code Formatting Tool (black recommended)

```bash
# Automatically format code using black
uv run black .
```

### (2) Testing Instructions

To test data conversion and validation functionalities, run the following commands:

```bash
# Test converting TIDAS data to eILCD format
uv run python src/tidas_tools/convert.py -i <TIDAS_data_directory> -o <eILCD_data_directory> --to-eilcd

# Test converting eILCD data to TIDAS format
uv run python src/tidas_tools/convert.py --input-dir <eILCD_data_directory> --output-dir <TIDAS_data_directory> --to-tidas

# Test external LCA format detection
uv run python src/tidas_tools/import_lca/cli.py --input <source_file_or_dir> --output-dir <output_directory> --detect-only

# Test TIDAS and eILCD/ILCD data validation functionality
# Execute automated tests
uv run pytest

# Validate TIDAS data
uv run python src/tidas_tools/validate.py -i <TIDAS_data_directory> --data-format tidas

# Validate eILCD/ILCD data
uv run python src/tidas_tools/validate.py -i <eILCD_data_directory> --data-format ilcd
```

---

## 10. Automatic Building and Publishing (CI/CD)

This project supports automatic building and publishing. When you push a git tag named with the `v<version>` format to the repository, it will trigger the workflow automatically. For example:

```bash
# List existing tags
git tag

# Create a new tag (e.g., version v0.0.1)
git tag v0.0.1

# Push the newly created tag to the remote repository to trigger automatic workflow
git push origin v0.0.1
```

Schema and methodology updates on `main` can also trigger a cross-repository SDK sync into `tiangong-lca/tidas-sdk` through `.github/workflows/dispatch-tidas-sdk-sync.yml`.

That automation requires the repository secret `TIDAS_SDK_AUTOMATION_TOKEN`.

---

## 11. Contribution

We welcome your contributions! You can participate in the project by submitting issues or pull requests.
