"""In-memory canonical store used by source adapters and writers."""

from __future__ import annotations

from collections import defaultdict
from collections.abc import Iterable

from ..model import CanonicalEntity


class MemoryCanonicalStore:
    """Small indexed store for canonical root entities."""

    def __init__(self) -> None:
        self._by_type: dict[str, dict[str, CanonicalEntity]] = defaultdict(dict)
        self._by_external_id: dict[tuple[str, str], CanonicalEntity] = {}

    def add(self, entity: CanonicalEntity) -> None:
        self._by_type[entity.entity_type][entity.internal_id] = entity
        if entity.external_id:
            self._by_external_id[(entity.entity_type, entity.external_id)] = entity

    def get(self, entity_type: str, internal_id: str) -> CanonicalEntity | None:
        return self._by_type.get(entity_type, {}).get(internal_id)

    def get_by_external_id(
        self, entity_type: str, external_id: str
    ) -> CanonicalEntity | None:
        return self._by_external_id.get((entity_type, external_id))

    def iter_type(self, entity_type: str) -> Iterable[CanonicalEntity]:
        return self._by_type.get(entity_type, {}).values()

    def counts(self) -> dict[str, int]:
        return {
            entity_type: len(entities)
            for entity_type, entities in sorted(self._by_type.items())
        }
