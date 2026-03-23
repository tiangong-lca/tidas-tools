from dataclasses import dataclass
from typing import Any


@dataclass(frozen=True)
class ValidationIssue:
    issue_code: str
    severity: str
    category: str
    file_path: str
    message: str
    location: str = "<root>"
    context: dict[str, Any] | None = None

    def to_dict(self) -> dict[str, Any]:
        return {
            "issue_code": self.issue_code,
            "severity": self.severity,
            "category": self.category,
            "file_path": self.file_path,
            "location": self.location,
            "message": self.message,
            "context": self.context or {},
        }


def summarize_issues(issues: list[ValidationIssue]) -> dict[str, int]:
    error_count = sum(1 for issue in issues if issue.severity == "error")
    warning_count = sum(1 for issue in issues if issue.severity == "warning")
    info_count = sum(1 for issue in issues if issue.severity == "info")
    return {
        "issue_count": len(issues),
        "error_count": error_count,
        "warning_count": warning_count,
        "info_count": info_count,
    }


def build_category_report(category: str, issues: list[ValidationIssue]) -> dict[str, Any]:
    return {
        "category": category,
        "ok": len(issues) == 0,
        "summary": summarize_issues(issues),
        "issues": [issue.to_dict() for issue in issues],
    }


def build_package_report(
    input_dir: str, category_reports: list[dict[str, Any]]
) -> dict[str, Any]:
    issues: list[dict[str, Any]] = []
    issue_count = 0
    error_count = 0
    warning_count = 0
    info_count = 0

    for report in category_reports:
        issues.extend(report["issues"])
        summary = report["summary"]
        issue_count += summary["issue_count"]
        error_count += summary["error_count"]
        warning_count += summary["warning_count"]
        info_count += summary["info_count"]

    return {
        "input_dir": input_dir,
        "ok": issue_count == 0,
        "summary": {
            "category_count": len(category_reports),
            "issue_count": issue_count,
            "error_count": error_count,
            "warning_count": warning_count,
            "info_count": info_count,
        },
        "categories": category_reports,
        "issues": issues,
    }
