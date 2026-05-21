"""Helpers for preserving source XML in import trace payloads."""

from __future__ import annotations

from typing import Any, Iterable

from lxml import etree

XML_NAMESPACE = "http://www.w3.org/XML/1998/namespace"


def element_trace(
    element,
    *,
    exclude_child_names: Iterable[str] = (),
) -> dict[str, Any]:
    """Return a compact, JSON/XML-safe trace for an XML element."""

    excluded = set(exclude_child_names)
    qname = etree.QName(element)
    trace: dict[str, Any] = {"name": qname.localname}
    if qname.namespace:
        trace["namespace"] = qname.namespace

    attributes = _attributes(element)
    if attributes:
        trace["attributes"] = attributes

    text = _clean_text(element.text)
    if text:
        trace["text"] = text

    children = []
    for child in element:
        if not isinstance(child.tag, str):
            continue
        if etree.QName(child).localname in excluded:
            continue
        children.append(element_trace(child, exclude_child_names=excluded))
    if children:
        trace["children"] = children

    return trace


def _attributes(element) -> list[dict[str, str]]:
    attributes = []
    for key, value in sorted(element.attrib.items()):
        qname = etree.QName(key)
        item = {"name": qname.localname, "value": value}
        if qname.namespace == XML_NAMESPACE:
            item["prefix"] = "xml"
        elif qname.namespace:
            item["namespace"] = qname.namespace
        attributes.append(item)
    return attributes


def _clean_text(value: str | None) -> str | None:
    if value is None:
        return None
    text = " ".join(value.split())
    return text or None
