from hashlib import sha256
import json
from pathlib import Path
import sys

import pytest

from tidas_tools.validation_batch import (
    BatchProtocolError,
    canonical_json_line,
    describe_document_validation,
    run_document_validation_batch,
)
from tidas_tools import validate as validate_module


def test_describe_handshake_exposes_reproducibility_fingerprint():
    description = describe_document_validation()

    assert description["schema_version"] == "tidas.validation-describe.v1"
    assert description["protocols"] == ["document-validation-batch.v1"]
    assert description["profiles"] == ["tidas-document-conformance.v1"]
    assert description["report_schema_versions"] == ["tidas.validation-report.v1"]
    assert len(description["tidas_schema_lock_sha256"]) == 64
    assert description["package"]["version"]
    assert description["engines"]["python"]
    assert description["engines"]["fastjsonschema"]
    assert description["engines"]["jsonschema"]


def test_describe_cli_outputs_json(monkeypatch, capsys):
    monkeypatch.setattr(
        sys, "argv", ["tidas-validate", "--describe", "--format", "json"]
    )

    validate_module.main()

    assert json.loads(capsys.readouterr().out)["protocols"] == [
        "document-validation-batch.v1"
    ]


def test_batch_streams_issues_then_final_without_repeating_report(tmp_path):
    batch_root = tmp_path / "batch"
    source_path = batch_root / "sources" / "bad.json"
    source_path.parent.mkdir(parents=True)
    source_path.write_text("{}", encoding="utf-8")
    manifest = tmp_path / "manifest.jsonl"
    _write_manifest(manifest, source_path, batch_root)
    events = []

    final = run_document_validation_batch(
        input_dir=batch_root,
        input_manifest=manifest,
        emit_event=events.append,
    )

    assert [event["type"] for event in events] == ["issue", "final"]
    issue = events[0]
    assert issue["document_key"] == "source:test:01.00.000"
    assert issue["issue"]["file_path"] == "sources/bad.json"
    assert final["completed"] is True
    assert final["summary"]["document_count"] == 1
    assert final["summary"]["error_count"] == 1
    assert "issues" not in final
    assert (
        final["logical_issue_stream_sha256"]
        == sha256(canonical_json_line(issue)).hexdigest()
    )


def test_batch_cli_exits_zero_even_when_data_issues_exist(
    tmp_path, monkeypatch, capsys
):
    batch_root = tmp_path / "batch"
    source_path = batch_root / "sources" / "bad.json"
    source_path.parent.mkdir(parents=True)
    source_path.write_text("{}", encoding="utf-8")
    manifest = tmp_path / "manifest.jsonl"
    _write_manifest(manifest, source_path, batch_root)
    monkeypatch.setattr(
        sys,
        "argv",
        [
            "tidas-validate",
            "--protocol",
            "document-validation-batch.v1",
            "--input-dir",
            str(batch_root),
            "--input-manifest",
            str(manifest),
        ],
    )

    validate_module.main()

    events = [json.loads(line) for line in capsys.readouterr().out.splitlines()]
    assert events[-1]["type"] == "final"
    assert events[-1]["summary"]["error_count"] == 1


def test_batch_cli_protocol_failure_exits_two(tmp_path, monkeypatch):
    batch_root = tmp_path / "batch"
    batch_root.mkdir()
    manifest = tmp_path / "manifest.jsonl"
    manifest.write_text(
        json.dumps(
            {
                "document_key": "missing",
                "category": "sources",
                "relative_path": "missing.json",
                "content_sha256": "0" * 64,
            }
        )
        + "\n",
        encoding="utf-8",
    )
    monkeypatch.setattr(
        sys,
        "argv",
        [
            "tidas-validate",
            "--protocol",
            "document-validation-batch.v1",
            "--input-dir",
            str(batch_root),
            "--input-manifest",
            str(manifest),
        ],
    )

    with pytest.raises(SystemExit) as exit_info:
        validate_module.main()

    assert exit_info.value.code == 2


def test_batch_event_order_and_hash_are_deterministic(tmp_path):
    batch_root = tmp_path / "batch"
    source_path = batch_root / "sources" / "bad.json"
    source_path.parent.mkdir(parents=True)
    source_path.write_text("{}", encoding="utf-8")
    manifest = tmp_path / "manifest.jsonl"
    _write_manifest(manifest, source_path, batch_root)

    first = []
    second = []
    run_document_validation_batch(
        input_dir=batch_root, input_manifest=manifest, emit_event=first.append
    )
    run_document_validation_batch(
        input_dir=batch_root, input_manifest=manifest, emit_event=second.append
    )

    assert first == second


@pytest.mark.parametrize(
    "mutate,expected",
    [
        (lambda item: item.update(document_key=""), "document_key"),
        (lambda item: item.update(relative_path="../bad.json"), "escapes"),
        (lambda item: item.update(content_sha256="0" * 64), "hash mismatch"),
    ],
)
def test_batch_rejects_invalid_manifest_contract(tmp_path, mutate, expected):
    batch_root = tmp_path / "batch"
    source_path = batch_root / "sources" / "bad.json"
    source_path.parent.mkdir(parents=True)
    source_path.write_text("{}", encoding="utf-8")
    item = _manifest_item(source_path, batch_root)
    mutate(item)
    manifest = tmp_path / "manifest.jsonl"
    manifest.write_text(json.dumps(item) + "\n", encoding="utf-8")

    with pytest.raises(BatchProtocolError, match=expected):
        run_document_validation_batch(
            input_dir=batch_root,
            input_manifest=manifest,
            emit_event=lambda _event: None,
        )


def test_batch_rejects_duplicate_keys_and_paths(tmp_path):
    batch_root = tmp_path / "batch"
    source_path = batch_root / "sources" / "bad.json"
    source_path.parent.mkdir(parents=True)
    source_path.write_text("{}", encoding="utf-8")
    item = _manifest_item(source_path, batch_root)
    manifest = tmp_path / "manifest.jsonl"
    manifest.write_text(
        json.dumps(item) + "\n" + json.dumps(item) + "\n", encoding="utf-8"
    )

    with pytest.raises(BatchProtocolError, match="duplicate document_key"):
        run_document_validation_batch(
            input_dir=batch_root,
            input_manifest=manifest,
            emit_event=lambda _event: None,
        )


def test_batch_rejects_duplicate_path_with_distinct_document_keys(tmp_path):
    batch_root = tmp_path / "batch"
    source_path = batch_root / "sources" / "bad.json"
    source_path.parent.mkdir(parents=True)
    source_path.write_text("{}", encoding="utf-8")
    first = _manifest_item(source_path, batch_root)
    second = {**first, "document_key": "source:other:01.00.000"}
    manifest = tmp_path / "manifest.jsonl"
    manifest.write_text(
        json.dumps(first) + "\n" + json.dumps(second) + "\n", encoding="utf-8"
    )

    with pytest.raises(BatchProtocolError, match="duplicate relative_path"):
        run_document_validation_batch(
            input_dir=batch_root,
            input_manifest=manifest,
            emit_event=lambda _event: None,
        )


def test_batch_rejects_symlink_input(tmp_path):
    batch_root = tmp_path / "batch"
    outside = tmp_path / "outside.json"
    outside.write_text("{}", encoding="utf-8")
    source_path = batch_root / "sources" / "bad.json"
    source_path.parent.mkdir(parents=True)
    source_path.symlink_to(outside)
    manifest = tmp_path / "manifest.jsonl"
    _write_manifest(manifest, source_path, batch_root, content_path=outside)

    with pytest.raises(BatchProtocolError, match="symlink"):
        run_document_validation_batch(
            input_dir=batch_root,
            input_manifest=manifest,
            emit_event=lambda _event: None,
        )


def test_batch_refuses_content_drift_during_validation(tmp_path, monkeypatch):
    batch_root = tmp_path / "batch"
    source_path = batch_root / "sources" / "bad.json"
    source_path.parent.mkdir(parents=True)
    source_path.write_text("{}", encoding="utf-8")
    manifest = tmp_path / "manifest.jsonl"
    _write_manifest(manifest, source_path, batch_root)
    original = validate_module._collect_tidas_file_issues

    def mutate_after_validation(*args, **kwargs):
        result = original(*args, **kwargs)
        source_path.write_text('{"changed":true}', encoding="utf-8")
        return result

    monkeypatch.setattr(
        validate_module, "_collect_tidas_file_issues", mutate_after_validation
    )
    events = []
    with pytest.raises(BatchProtocolError, match="content hash mismatch"):
        run_document_validation_batch(
            input_dir=batch_root,
            input_manifest=manifest,
            emit_event=events.append,
        )

    assert events == []


def _write_manifest(
    manifest: Path,
    source_path: Path,
    batch_root: Path,
    *,
    content_path: Path | None = None,
) -> None:
    item = _manifest_item(source_path, batch_root)
    item["content_sha256"] = sha256(
        (content_path or source_path).read_bytes()
    ).hexdigest()
    manifest.write_text(json.dumps(item) + "\n", encoding="utf-8")


def _manifest_item(source_path: Path, batch_root: Path) -> dict:
    return {
        "document_key": "source:test:01.00.000",
        "category": "sources",
        "relative_path": source_path.relative_to(batch_root).as_posix(),
        "content_sha256": sha256(source_path.read_bytes()).hexdigest(),
        "identity": {
            "dataset_type": "source",
            "dataset_id": "11111111-1111-1111-1111-111111111111",
            "dataset_version": "01.00.000",
        },
    }
