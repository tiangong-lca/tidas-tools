# Python-to-Rust ownership matrix

This inventory freezes the Python implementation as a migration oracle for
[tidas-tools#117](https://github.com/tiangong-lca/tidas-tools/issues/117). It is
not a promise to preserve Python module boundaries, command names, or argument
layouts. Each row assigns behavior to a stable Rust domain owner and a tracked
delivery slice. Private helpers move with their containing module unless a
later design extracts a more stable domain.

## Product entry points

| Frozen Python entry point | Final Rust surface | Owner | Delivery |
| --- | --- | --- | --- |
| `tidas-convert` | `tidas convert` | `tidas-convert` + thin `tidas-cli` adapter | #121 |
| `tidas-import` | `tidas import` | `tidas-import` + thin `tidas-cli` adapter | #122 |
| `tidas-export` | `tidas export` | `tidas-export` + thin `tidas-cli` adapter | #123 |
| `tidas-validate` | `tidas validate` | `tidas-validate` + thin `tidas-cli` adapter | #120 |
| `tidas-release-tool` | `tidas release` | `tidas-release` + thin `tidas-cli` adapter | #124 |
| `runtime_rulesets.main` | `tidas ruleset` | `tidas-validate` + thin `tidas-cli` adapter | #120 |
| package version reporting | `tidas version` | `tidas-contracts`, `tidas-assets`, `tidas-xml`, `tidas-cli` | #118/#119 |

No legacy executable name is registered by the Rust workspace. During the
migration the Python commands remain internal golden/parity oracles only.

## Module and public-symbol inventory

| Frozen Python module group | Public symbols captured at #118 | Rust owner | Delivery |
| --- | --- | --- | --- |
| `convert.py` | `convert_format`, `convert_directory`, `main` | `tidas-convert` | #121 |
| `export.py` | `zip_folder`, `process_record`, `process_common_record`, `export_common_records`, `export_category_records`, `download_external_docs`, `parse_arguments`, `main` | `tidas-export` | #123 |
| `package_versions.py` | `VersionedRecord`, `normalize_package_versions` | `tidas-export` | #123 |
| `release.py` | `ReleaseToolError`, `DatasetEntry`, `sha256_file`, `order_tidas_document_for_xml`, `load_dataset_index`, `resolve_profile_closure`, `convert_tidas_to_ilcd`, `semantic_roundtrip_report`, `validate_release_tree`, `deterministic_zip`, `build_release_packages`, `execute`, `main` | `tidas-release` | #124 |
| `validate.py` | `tidas_language_codes`, `TidasSchemaValidator`, `ClassificationIssueDetail`, `is_valid_cas_number`, hierarchy/localized-text validators, `retrieve_schema`, category/package/ILCD validators, `main` | `tidas-validate` | #120 |
| `validation_report.py` | `ValidationIssue`, `summarize_issues`, `build_category_report`, `build_package_report` | `tidas-contracts`, `tidas-validate` | #120 |
| `validation_batch.py` | `BatchProtocolError`, `BatchDocument`, `describe_document_validation`, `run_document_validation_batch`, `load_batch_manifest`, `canonical_json_line` | `tidas-contracts`, `tidas-runtime`, `tidas-validate` | #120 |
| `reference_extraction.py` | `ReferenceEdgeV1`, `ReferenceExtractionIssueV1`, `ReferenceExtractionResultV1`, `extract_references` | `tidas-contracts`, `tidas-validate` | #120 |
| `runtime_rulesets.py`, `validate_methodologies.py` | `load_runtime_rulesets`, `validate_runtime_rulesets`, `rules_for_ruleset`, `SchemaMethodologyValidator`, both `main` functions | `tidas-validate` | #120 |
| `import_lca/cli.py`, `detect.py`, `errors.py` | `build_parser`, `run_import`, `main`, `DetectedFormat`, `detect_format`, import error classes | `tidas-import` | #122 |
| `import_lca/model/**`, `store/**`, `report.py` | `EntityRef`, `CanonicalEntity`, `CanonicalExchange`, `MemoryCanonicalStore`, `ConversionIssue`, `ConversionReport` | `tidas-import` with bounded/spooled storage from `tidas-runtime` | #122 |
| `import_lca/adapters/base.py` and concrete adapters | `SourceAdapter`, `EcoSpold1Adapter`, `EcoSpold2Adapter`, `IlcdAdapter`, `OpenLcaJsonLdAdapter`, `OpenLcaProcessXlsxAdapter`, `SimaProCsvAdapter` | `tidas-import` adapter modules | #122 |
| `import_lca/adapters/xml_trace.py`, `mapping_csv.py`, `process_bundles.py` | `element_trace`, `write_mapping_csv`, `path_priority`, `write_process_bundles` | `tidas-import` | #122 |
| `import_lca/writers/**` | `write_ilcd_from_tidas`, `write_tidas_package`, `scan_conversion_gaps`, `ensure_tidas_package_dirs` | `tidas-import`, using `tidas-xml` where applicable | #122 |
| `tidas_log.py` | `ColoredFormatter`, `setup_logging` | deleted as a Python-specific adapter; Rust diagnostics use `tidas-contracts` | #119/#126 |
| package `__init__.py` files | no independent behavior | no Rust owner | #126 |
| `validation_indexes/**` | packaged private projection data | `tidas-assets`, consumed by `tidas-validate` | #118/#120 |
| `tidas/**`, `eilcd/**` | JSON Schema, methodology, XSD, XSLT, and XML reference assets | `tidas-assets`, consumed by domain crates | #118 |

## Foundation already owned by #118

| Rust crate | Stable responsibility |
| --- | --- |
| `tidas-contracts` | versioned reports, diagnostics, artifacts, completeness, and exit classes |
| `tidas-runtime` | cancellation, explicit memory reservations, bounded queues, and streaming JSONL spool summaries |
| `tidas-assets` | executable asset embedding, classification, integrity, and deterministic lock |
| `tidas-xml` | strict streaming XML inspection plus the serialized libxml2/libxslt compatibility boundary |
| `tidas-cli` | final command tree, output routing, parse guidance, and thin domain dispatch only |

The final removal slice (#126) must re-run this inventory against the active
tree and prove that no Python implementation, install path, CI path, release
path, workspace script, or agent guidance remains.
