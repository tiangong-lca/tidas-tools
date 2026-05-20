"""Base source adapter contract."""

from __future__ import annotations

from abc import ABC, abstractmethod
from pathlib import Path

from ..report import ConversionReport
from ..store import MemoryCanonicalStore


class SourceAdapter(ABC):
    """Parse one external LCA format into the canonical store."""

    format_id: str

    @abstractmethod
    def read(
        self,
        source: str | Path,
        store: MemoryCanonicalStore,
        report: ConversionReport,
    ) -> None:
        """Read ``source`` into ``store`` and append report issues as needed."""
