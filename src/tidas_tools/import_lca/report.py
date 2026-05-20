"""Machine-readable conversion report models."""

from __future__ import annotations

from dataclasses import dataclass, field
from datetime import datetime, timezone
import json
from pathlib import Path
from typing import Any


@dataclass(frozen=True)
class ConversionIssue:
    """A single conversion issue, warning, or note."""

    severity: str
    code: str
    message: str
    source_object: str | None = None
    context: dict[str, Any] = field(default_factory=dict)

    def to_dict(self) -> dict[str, Any]:
        data: dict[str, Any] = {
            "severity": self.severity,
            "code": self.code,
            "message": self.message,
        }
        if self.source_object:
            data["source_object"] = self.source_object
        if self.context:
            data["context"] = self.context
        return data


@dataclass
class ConversionReport:
    """Report emitted by tidas-import."""

    source_path: str
    detected_format: str | None = None
    target: str = "tidas"
    tidas_dir: str | None = None
    ilcd_dir: str | None = None
    detection_evidence: list[str] = field(default_factory=list)
    object_counts: dict[str, int] = field(default_factory=dict)
    issues: list[ConversionIssue] = field(default_factory=list)
    validation: dict[str, Any] = field(default_factory=dict)
    started_at: str = field(
        default_factory=lambda: datetime.now(timezone.utc).isoformat()
    )

    def add_issue(
        self,
        *,
        severity: str,
        code: str,
        message: str,
        source_object: str | None = None,
        context: dict[str, Any] | None = None,
    ) -> None:
        self.issues.append(
            ConversionIssue(
                severity=severity,
                code=code,
                message=message,
                source_object=source_object,
                context=context or {},
            )
        )

    @property
    def warning_count(self) -> int:
        return sum(1 for issue in self.issues if issue.severity == "warning")

    @property
    def error_count(self) -> int:
        return sum(1 for issue in self.issues if issue.severity == "error")

    def to_dict(self) -> dict[str, Any]:
        counts = {
            "contacts": 0,
            "sources": 0,
            "unitgroups": 0,
            "flowproperties": 0,
            "flows": 0,
            "processes": 0,
            "lciamethods": 0,
            "lifecyclemodels": 0,
            **self.object_counts,
        }
        return {
            "tool": "tidas-tools",
            "operation": "external-to-tidas",
            "started_at": self.started_at,
            "source": {
                "path": self.source_path,
                "detected_format": self.detected_format,
                "evidence": self.detection_evidence,
            },
            "target": {
                "requested": self.target,
                "tidas_dir": self.tidas_dir,
                "ilcd_dir": self.ilcd_dir,
            },
            "summary": {
                **counts,
                "warnings": self.warning_count,
                "errors": self.error_count,
            },
            "issues": [issue.to_dict() for issue in self.issues],
            "validation": self.validation,
        }

    def write_json(self, output_path: str | Path) -> None:
        path = Path(output_path)
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(
            json.dumps(self.to_dict(), indent=2, ensure_ascii=False) + "\n",
            encoding="utf-8",
        )
