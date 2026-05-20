"""Minimal canonical entities used before writing TIDAS packages."""

from __future__ import annotations

from dataclasses import dataclass, field
from decimal import Decimal
from typing import Any


@dataclass(frozen=True)
class EntityRef:
    """Reference to a canonical root entity."""

    target_type: str
    target_id: str | None = None
    source_label: str | None = None
    resolved: bool = False


@dataclass
class CanonicalEntity:
    """Canonical root entity shared by future source adapters."""

    entity_type: str
    internal_id: str
    external_id: str | None = None
    name: str | None = None
    category_path: list[str] = field(default_factory=list)
    raw: dict[str, Any] = field(default_factory=dict)


@dataclass
class CanonicalExchange:
    """Canonical process exchange surface needed by all source adapters."""

    internal_id: str
    flow_ref: EntityRef
    direction: str
    amount: Decimal | None = None
    formula: str | None = None
    unit_ref: EntityRef | None = None
    flow_property_ref: EntityRef | None = None
    provider_ref: EntityRef | None = None
    location: str | None = None
    uncertainty: dict[str, Any] | None = None
    dq_entry: str | None = None
    raw: dict[str, Any] = field(default_factory=dict)
