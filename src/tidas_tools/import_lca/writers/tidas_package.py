"""TIDAS package layout helpers."""

from __future__ import annotations

from pathlib import Path

TIDAS_CATEGORIES = (
    "contacts",
    "sources",
    "unitgroups",
    "flowproperties",
    "flows",
    "processes",
    "lciamethods",
    "lifecyclemodels",
)


def ensure_tidas_package_dirs(output_dir: str | Path) -> Path:
    """Create the category directories expected by the TIDAS validator."""

    root = Path(output_dir)
    root.mkdir(parents=True, exist_ok=True)
    for category in TIDAS_CATEGORIES:
        (root / category).mkdir(parents=True, exist_ok=True)
    return root
