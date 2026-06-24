"""Write canonical entities as validation-compatible TIDAS JSON packages."""

from __future__ import annotations

from collections.abc import Iterator
from copy import deepcopy
from decimal import Decimal, InvalidOperation
from functools import lru_cache
import json
from pathlib import Path
import re
from typing import Any
import uuid

from ..model import CanonicalEntity
from ..store import MemoryCanonicalStore
from .tidas_package import ensure_tidas_package_dirs

DEFAULT_VERSION = "00.00.001"
IMPORT_ID_NAMESPACE = "tidas-tools/import"
IMPORT_TRACE_NAMESPACE = "https://tiangong.earth/tidas/import-trace/1.0"
IMPORT_TRACE_MARKER = "TIDAS_IMPORT_TRACE_V1"
IMPORT_GAP_MARKER = "TIDAS_IMPORT_GAP_V1"
# Deterministic, obviously-fake timestamp used when the source declares none.
# Fixed (never wall-clock) so re-importing the same bundle is idempotent, and
# recognisable so it can be told apart from a real timeStamp and repaired.
PLACEHOLDER_TIMESTAMP = "1900-01-01T00:00:00Z"
IMPORT_TOOL_CONTACT_NAME = "TianGong LCA import tooling"
COMPLIANCE_NOT_DECLARED = "External LCA source compliance context"
DATA_SOURCE_NOT_DECLARED = "External LCA source metadata"
SOURCE_VALUE_NOT_DECLARED = "Not declared in source package"
CONVERTED_EXTERNAL_LCA_DATA = "Converted from external LCA package"
REVIEW_NOT_DECLARED = "No source review record provided"
ANNUAL_VOLUME_UNAVAILABLE = "source production volume unavailable"
IMPORT_CONTACT_ID = str(
    uuid.uuid5(uuid.NAMESPACE_URL, f"{IMPORT_ID_NAMESPACE}/contact")
)
FORMAT_SOURCE_ID = str(
    uuid.uuid5(uuid.NAMESPACE_URL, f"{IMPORT_ID_NAMESPACE}/ilcd-format")
)
COMPLIANCE_SOURCE_ID = str(
    uuid.uuid5(uuid.NAMESPACE_URL, f"{IMPORT_ID_NAMESPACE}/not-defined-compliance")
)
DEFAULT_UNIT_GROUP_ID = str(
    uuid.uuid5(uuid.NAMESPACE_URL, f"{IMPORT_ID_NAMESPACE}/default-unit-group")
)
DEFAULT_FLOW_PROPERTY_ID = str(
    uuid.uuid5(uuid.NAMESPACE_URL, f"{IMPORT_ID_NAMESPACE}/default-flow-property")
)
PERMANENT_URI_BASE = "https://lcdn.tiangong.earth"
PERMANENT_URI_PATHS = {
    "contacts": "datasetdetail/contact.xhtml",
    "flowproperties": "datasetdetail/flowproperty.xhtml",
    "flows": "datasetdetail/productFlow.xhtml",
    "lifecyclemodels": "datasetdetail/lifecyclemodel.xhtml",
    "processes": "datasetdetail/process.xhtml",
    "sources": "datasetdetail/source.xhtml",
}
REAL_PATTERN = re.compile(r"[+-]?(\d+(\.\d*)?|\.\d+)([Ee][+-]?\d+)?$")
PERCENT_PATTERN = re.compile(r"100(\.0{1,3})?|([0-9]|[1-9][0-9])(\.\d{1,3})?")

# Schema-required classification slots the importer cannot derive from the
# source at import time: the source only carries its own native taxonomy text
# (e.g. {"category": "chemicals"}), never a CPC/ISIC code, and the real code is
# assigned by a downstream classification step. These are deliberate
# placeholders; every emission is tagged with an IMPORT_GAP_MARKER
# conversionGap in common:other so it is machine-distinguishable from a real
# classification and can be picked up for repair (see scan_conversion_gaps).
PLACEHOLDER_PRODUCT_CLASSIFICATION = [
    {
        "@level": "0",
        "@classId": "9",
        "#text": "Community, social and personal services",
    },
    {
        "@level": "1",
        "@classId": "94",
        "#text": "Sewage and waste collection, treatment and disposal and other environmental protection services",
    },
    {
        "@level": "2",
        "@classId": "949",
        "#text": "Other environmental protection services n.e.c.",
    },
    {
        "@level": "3",
        "@classId": "9490",
        "#text": "Other environmental protection services n.e.c.",
    },
    {
        "@level": "4",
        "@classId": "94900",
        "#text": "Other environmental protection services n.e.c.",
    },
]
PLACEHOLDER_PROCESS_CLASSIFICATION = [
    {"@level": "0", "@classId": "T", "#text": "Other service activities"},
    {
        "@level": "1",
        "@classId": "94",
        "#text": "Activities of membership organizations",
    },
    {
        "@level": "2",
        "@classId": "949",
        "#text": "Activities of other membership organizations",
    },
    {
        "@level": "3",
        "@classId": "9499",
        "#text": "Activities of other membership organizations n.e.c.",
    },
]
PLACEHOLDER_ELEMENTARY_CATEGORY = [
    {"@level": "0", "@catId": "1", "#text": "Emissions"},
    {"@level": "1", "@catId": "1.3", "#text": "Emissions to air"},
    {"@level": "2", "@catId": "1.3.4", "#text": "Emissions to air, unspecified"},
]


def write_tidas_package(store: MemoryCanonicalStore, output_dir: str | Path) -> None:
    root = ensure_tidas_package_dirs(output_dir)
    _write_json(root / "contacts" / f"{IMPORT_CONTACT_ID}.json", _default_contact())
    _write_json(root / "sources" / f"{FORMAT_SOURCE_ID}.json", _format_source())
    _write_json(root / "sources" / f"{COMPLIANCE_SOURCE_ID}.json", _compliance_source())
    for entity in store.iter_type("contacts"):
        if entity.internal_id == IMPORT_CONTACT_ID:
            continue
        _write_json(
            root / "contacts" / f"{entity.internal_id}.json",
            _contact_dataset(entity),
        )
    for entity in store.iter_type("sources"):
        if entity.internal_id in {FORMAT_SOURCE_ID, COMPLIANCE_SOURCE_ID}:
            continue
        _write_json(
            root / "sources" / f"{entity.internal_id}.json",
            _imported_source_dataset(entity),
        )

    generated_units = _generated_unit_entities(store)
    unitgroups = list(store.iter_type("unitgroups")) + [
        item[0] for item in generated_units.values()
    ]
    if not unitgroups:
        unitgroups = [
            CanonicalEntity(
                entity_type="unitgroups",
                internal_id=DEFAULT_UNIT_GROUP_ID,
                name="Units of mass",
                raw={"units": [{"name": "kg", "conversionFactor": 1.0}]},
            )
        ]
    for entity in unitgroups:
        _write_json(
            root / "unitgroups" / f"{entity.internal_id}.json",
            _unit_group_dataset(entity),
        )

    generated_flowproperties = [item[1] for item in generated_units.values()]
    flowproperties = list(store.iter_type("flowproperties")) + generated_flowproperties
    if not flowproperties:
        flowproperties = [
            CanonicalEntity(
                entity_type="flowproperties",
                internal_id=DEFAULT_FLOW_PROPERTY_ID,
                name="Mass",
                raw={"unitGroupRefId": unitgroups[0].internal_id},
            )
        ]
    for entity in flowproperties:
        _write_json(
            root / "flowproperties" / f"{entity.internal_id}.json",
            _flow_property_dataset(entity, unitgroups[0]),
        )

    for entity in store.iter_type("flows"):
        _write_json(
            root / "flows" / f"{entity.internal_id}.json",
            _flow_dataset(entity, flowproperties[0], generated_units),
        )

    for entity in store.iter_type("processes"):
        _write_json(
            root / "processes" / f"{entity.internal_id}.json",
            _process_dataset(entity),
        )

    for entity in store.iter_type("lifecyclemodels"):
        _write_json(
            root / "lifecyclemodels" / f"{entity.internal_id}.json",
            _lifecycle_model_dataset(entity),
        )


def _write_json(path: Path, payload: dict[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(payload, indent=2, ensure_ascii=False) + "\n")


def _ml(text: str, lang: str = "en") -> dict[str, str]:
    value = str(text or "").strip() or SOURCE_VALUE_NOT_DECLARED
    return {"@xml:lang": lang, "#text": value}


def _ml_500(text: Any, lang: str = "en") -> dict[str, str]:
    value = str(text or "").strip() or SOURCE_VALUE_NOT_DECLARED
    return _ml(_truncate(value, 500), lang=lang)


def _truncate(text: str, max_length: int) -> str:
    if len(text) <= max_length:
        return text
    if max_length <= 3:
        return text[:max_length]
    return text[: max_length - 3].rstrip() + "..."


def _ref(
    *,
    ref_type: str,
    ref_id: str,
    short_description: str,
    category: str | None = None,
) -> dict[str, Any]:
    uri = f"../{category}/{ref_id}.json" if category else f"../{ref_id}.json"
    return {
        "@type": ref_type,
        "@refObjectId": ref_id,
        "@version": DEFAULT_VERSION,
        "@uri": uri,
        "common:shortDescription": _ml(short_description),
    }


def _format_ref() -> dict[str, Any]:
    return _ref(
        ref_type="source data set",
        ref_id=FORMAT_SOURCE_ID,
        short_description="ILCD format",
        category="sources",
    )


def _compliance_ref() -> dict[str, Any]:
    return _ref(
        ref_type="source data set",
        ref_id=COMPLIANCE_SOURCE_ID,
        short_description=COMPLIANCE_NOT_DECLARED,
        category="sources",
    )


def _owner_ref() -> dict[str, Any]:
    return _ref(
        ref_type="contact data set",
        ref_id=IMPORT_CONTACT_ID,
        short_description=IMPORT_TOOL_CONTACT_NAME,
        category="contacts",
    )


def _contact_ref_from_raw(value: Any) -> dict[str, Any] | None:
    if not isinstance(value, dict):
        return None
    ref_id = value.get("id")
    if not _uuid_text(ref_id):
        return None
    return _ref(
        ref_type="contact data set",
        ref_id=str(ref_id),
        short_description=str(value.get("name") or ref_id),
        category="contacts",
    )


def _permanent_dataset_uri(
    category: str, dataset_id: str, version: str = DEFAULT_VERSION
) -> str:
    if category == "unitgroups":
        return f"{PERMANENT_URI_BASE}/unitgroups/{dataset_id}?version={version}"
    path = PERMANENT_URI_PATHS[category]
    return f"{PERMANENT_URI_BASE}/{path}?uuid={dataset_id}&version={version}"


def _admin_info(
    *,
    category: str,
    dataset_id: str,
    version: str = DEFAULT_VERSION,
    process: bool = False,
    timestamp: Any = None,
    data_entry_ref: dict[str, Any] | None = None,
    owner_ref: dict[str, Any] | None = None,
    copyright_value: Any = None,
    access_restrictions: Any = None,
) -> dict[str, Any]:
    data_entry = {
        "common:timeStamp": _date_time(timestamp) or PLACEHOLDER_TIMESTAMP,
        "common:referenceToDataSetFormat": _format_ref(),
    }
    if process:
        data_entry["common:referenceToPersonOrEntityEnteringTheData"] = (
            data_entry_ref or _owner_ref()
        )

    publication = {
        "common:dataSetVersion": version,
        "common:permanentDataSetURI": _permanent_dataset_uri(
            category, dataset_id, version
        ),
    }
    if process:
        publication.update(
            {
                "common:referenceToOwnershipOfDataSet": owner_ref or _owner_ref(),
                "common:copyright": _bool_text(copyright_value),
                "common:licenseType": "Free of charge for all users and uses",
            }
        )
        if _text(access_restrictions):
            publication["common:accessRestrictions"] = _ml(
                str(access_restrictions).strip()
            )
    else:
        publication["common:referenceToOwnershipOfDataSet"] = owner_ref or _owner_ref()

    return {
        "dataEntryBy": data_entry,
        "publicationAndOwnership": publication,
    }


def _compliance_declarations(process: bool = False) -> dict[str, Any]:
    compliance = {
        "common:referenceToComplianceSystem": _compliance_ref(),
        "common:approvalOfOverallCompliance": "Not defined",
    }
    if process:
        compliance.update(
            {
                "common:nomenclatureCompliance": "Not defined",
                "common:methodologicalCompliance": "Not defined",
                "common:reviewCompliance": "Not defined",
                "common:documentationCompliance": "Not defined",
                "common:qualityCompliance": "Not defined",
            }
        )
    return {"compliance": compliance}


def _default_contact() -> dict[str, Any]:
    return _contact_payload(
        contact_id=IMPORT_CONTACT_ID,
        name=IMPORT_TOOL_CONTACT_NAME,
        short_name=IMPORT_TOOL_CONTACT_NAME,
    )


def _contact_dataset(entity: CanonicalEntity) -> dict[str, Any]:
    raw = entity.raw or {}
    return _contact_payload(
        contact_id=entity.internal_id,
        name=entity.name or "Imported contact",
        short_name=raw.get("shortName") or entity.name or "Imported contact",
        email=raw.get("email"),
        telephone=raw.get("telephone"),
        website=raw.get("website") or raw.get("url"),
        address=raw.get("address"),
        description=raw.get("description"),
        source_trace=raw.get("sourceTrace"),
    )


def _contact_payload(
    *,
    contact_id: str,
    name: str,
    short_name: str,
    email: Any = None,
    telephone: Any = None,
    website: Any = None,
    address: Any = None,
    description: Any = None,
    source_trace: Any = None,
) -> dict[str, Any]:
    dataset_info = {
        "common:UUID": contact_id,
        "common:shortName": _ml_500(short_name),
        "common:name": _ml_500(name),
        "classificationInformation": {
            "common:classification": {
                "common:class": {
                    "@level": "0",
                    "@classId": "5",
                    "#text": "Other",
                }
            }
        },
    }
    if _email(email):
        dataset_info["email"] = str(email).strip()
    if _text(telephone):
        dataset_info["telephone"] = str(telephone).strip()
    if _text(website):
        dataset_info["WWWAddress"] = str(website).strip()
    if _text(address):
        dataset_info["contactAddress"] = _ml(str(address).strip())
    if _text(description):
        dataset_info["contactDescriptionOrComment"] = _ml(str(description).strip())
    common_other = _common_other_trace(source_trace)
    if common_other:
        dataset_info["common:other"] = common_other

    return {
        "contactDataSet": {
            "@xmlns:common": "http://lca.jrc.it/ILCD/Common",
            "@xmlns": "http://lca.jrc.it/ILCD/Contact",
            "@xmlns:xsi": "http://www.w3.org/2001/XMLSchema-instance",
            "@version": "1.1",
            "@xsi:schemaLocation": "http://lca.jrc.it/ILCD/Contact ../../schemas/ILCD_ContactDataSet.xsd",
            "contactInformation": {
                "dataSetInformation": dataset_info,
            },
            "administrativeInformation": _admin_info(
                category="contacts", dataset_id=contact_id
            ),
        }
    }


def _source_dataset(source_id: str, short_name: str, class_id: str, class_text: str):
    return _source_payload(
        source_id=source_id,
        short_name=short_name,
        class_id=class_id,
        class_text=class_text,
    )


def _imported_source_dataset(entity: CanonicalEntity) -> dict[str, Any]:
    raw = entity.raw or {}
    return _source_payload(
        source_id=entity.internal_id,
        short_name=raw.get("shortName") or entity.name or "Imported source",
        class_id="6",
        class_text="Other source types",
        citation=raw.get("textReference")
        or raw.get("sourceCitation")
        or raw.get("citation")
        or entity.name,
        publication_type=raw.get("publicationType"),
        description=raw.get("description"),
        url=raw.get("url") or raw.get("externalUrl"),
        source_trace=raw.get("sourceTrace"),
    )


def _source_payload(
    *,
    source_id: str,
    short_name: str,
    class_id: str,
    class_text: str,
    citation: Any = None,
    publication_type: Any = None,
    description: Any = None,
    url: Any = None,
    source_trace: Any = None,
) -> dict[str, Any]:
    dataset_info = {
        "common:UUID": source_id,
        "common:shortName": _ml_500(short_name),
        "classificationInformation": {
            "common:classification": {
                "common:class": {
                    "@level": "0",
                    "@classId": class_id,
                    "#text": class_text,
                }
            }
        },
    }
    if _text(citation):
        dataset_info["sourceCitation"] = str(citation).strip()
    if _publication_type(publication_type):
        dataset_info["publicationType"] = str(publication_type).strip()
    if _text(description):
        dataset_info["sourceDescriptionOrComment"] = _ml(str(description).strip())
    if _uri(url):
        dataset_info["referenceToDigitalFile"] = {"@uri": str(url).strip()}
    common_other = _common_other_trace(source_trace)
    if common_other:
        dataset_info["common:other"] = common_other

    return {
        "sourceDataSet": {
            "@xmlns:common": "http://lca.jrc.it/ILCD/Common",
            "@xmlns": "http://lca.jrc.it/ILCD/Source",
            "@xmlns:xsi": "http://www.w3.org/2001/XMLSchema-instance",
            "@version": "1.1",
            "@xsi:schemaLocation": "http://lca.jrc.it/ILCD/Source ../../schemas/ILCD_SourceDataSet.xsd",
            "sourceInformation": {
                "dataSetInformation": dataset_info,
            },
            "administrativeInformation": _admin_info(
                category="sources", dataset_id=source_id
            ),
        }
    }


def _format_source() -> dict[str, Any]:
    return _source_dataset(FORMAT_SOURCE_ID, "ILCD format", "1", "Data set formats")


def _compliance_source() -> dict[str, Any]:
    return _source_dataset(
        COMPLIANCE_SOURCE_ID,
        COMPLIANCE_NOT_DECLARED,
        "3",
        "Compliance systems",
    )


def _unit_group_dataset(entity: CanonicalEntity) -> dict[str, Any]:
    units = entity.raw.get("units") or [{"name": entity.name or "unit", "meanValue": 1}]
    unit_items = []
    for idx, unit in enumerate(units, start=1):
        unit_items.append(
            {
                "@dataSetInternalID": str(idx),
                "name": str(unit.get("name") or unit.get("refUnit") or "unit"),
                "meanValue": _real(
                    unit.get("conversionFactor", unit.get("meanValue", 1))
                ),
            }
        )
    dataset_info = {
        "common:UUID": entity.internal_id,
        "common:name": _ml_500(entity.name or "Unit group"),
        "classificationInformation": {
            "common:classification": {
                "common:class": {
                    "@level": "0",
                    "@classId": "4",
                    "#text": "Other unit groups",
                }
            }
        },
    }
    common_other = _common_other_trace(entity.raw.get("sourceTrace"))
    if common_other:
        dataset_info["common:other"] = common_other

    return {
        "unitGroupDataSet": {
            "@xmlns": "http://lca.jrc.it/ILCD/UnitGroup",
            "@xmlns:common": "http://lca.jrc.it/ILCD/Common",
            "@xmlns:xsi": "http://www.w3.org/2001/XMLSchema-instance",
            "@version": "1.1",
            "@xsi:schemaLocation": "http://lca.jrc.it/ILCD/UnitGroup ../../schemas/ILCD_UnitGroupDataSet.xsd",
            "unitGroupInformation": {
                "dataSetInformation": dataset_info,
                "quantitativeReference": {"referenceToReferenceUnit": "1"},
            },
            "modellingAndValidation": {
                "complianceDeclarations": _compliance_declarations()
            },
            "administrativeInformation": _admin_info(
                category="unitgroups", dataset_id=entity.internal_id
            ),
            "units": {"unit": unit_items},
        }
    }


def _flow_property_dataset(
    entity: CanonicalEntity, default_unit_group: CanonicalEntity
) -> dict[str, Any]:
    unit_group_id = entity.raw.get("unitGroupRefId") or default_unit_group.internal_id
    unit_group_name = (
        entity.raw.get("unitGroupName") or default_unit_group.name or "Unit group"
    )
    dataset_info = {
        "common:UUID": entity.internal_id,
        "common:name": _ml_500(entity.name or "Flow property"),
        "classificationInformation": {
            "common:classification": {
                "common:class": {
                    "@level": "0",
                    "@classId": "4",
                    "#text": "Other flow properties",
                }
            }
        },
    }
    common_other = _common_other_trace(entity.raw.get("sourceTrace"))
    if common_other:
        dataset_info["common:other"] = common_other
    return {
        "flowPropertyDataSet": {
            "@xmlns": "http://lca.jrc.it/ILCD/FlowProperty",
            "@xmlns:common": "http://lca.jrc.it/ILCD/Common",
            "@xmlns:xsi": "http://www.w3.org/2001/XMLSchema-instance",
            "@version": "1.1",
            "@xsi:schemaLocation": "http://lca.jrc.it/ILCD/FlowProperty ../../schemas/ILCD_FlowPropertyDataSet.xsd",
            "flowPropertiesInformation": {
                "dataSetInformation": dataset_info,
                "quantitativeReference": {
                    "referenceToReferenceUnitGroup": _ref(
                        ref_type="unit group data set",
                        ref_id=unit_group_id,
                        short_description=unit_group_name,
                        category="unitgroups",
                    )
                },
            },
            "modellingAndValidation": {
                "complianceDeclarations": _compliance_declarations()
            },
            "administrativeInformation": _admin_info(
                category="flowproperties", dataset_id=entity.internal_id
            ),
        }
    }


def _flow_dataset(
    entity: CanonicalEntity,
    default_flow_property: CanonicalEntity,
    generated_units: dict[str, tuple[CanonicalEntity, CanonicalEntity]] | None = None,
):
    flow_type = _flow_type(entity.raw.get("flowType"))
    classification = _flow_classification(flow_type, entity.raw)
    dataset_info = {
        "common:UUID": entity.internal_id,
        "name": _name_parts(entity.name or "Flow"),
    }
    if _text(entity.raw.get("synonyms")):
        dataset_info["common:synonyms"] = _ml(str(entity.raw["synonyms"]).strip())
    dataset_info["classificationInformation"] = classification
    cas_number = _cas_number(entity.raw.get("CASNumber"))
    if cas_number:
        dataset_info["CASNumber"] = cas_number
    if _text(entity.raw.get("sumFormula")):
        dataset_info["sumFormula"] = str(entity.raw["sumFormula"]).strip()
    common_other = _common_other_trace(entity.raw.get("sourceTrace"))
    if common_other:
        dataset_info["common:other"] = common_other
    generated_flow_property = _generated_flow_property_for(entity, generated_units)
    flow_property_id = (
        entity.raw.get("flowPropertyRefId")
        or (
            generated_flow_property.internal_id
            if generated_flow_property is not None
            else None
        )
        or default_flow_property.internal_id
    )
    flow_property_name = (
        entity.raw.get("flowPropertyName")
        or (
            generated_flow_property.name
            if generated_flow_property is not None
            else None
        )
        or default_flow_property.name
        or "Flow property"
    )
    return {
        "flowDataSet": {
            "@xmlns": "http://lca.jrc.it/ILCD/Flow",
            "@xmlns:common": "http://lca.jrc.it/ILCD/Common",
            "@xmlns:ecn": "http://eplca.jrc.ec.europa.eu/ILCD/Extensions/2018/ECNumber",
            "@xmlns:xsi": "http://www.w3.org/2001/XMLSchema-instance",
            "@version": "1.1",
            "@locations": "../ILCDLocations.xml",
            "@xsi:schemaLocation": "http://lca.jrc.it/ILCD/Flow ../../schemas/ILCD_FlowDataSet.xsd",
            "flowInformation": {
                "dataSetInformation": dataset_info,
                "quantitativeReference": {"referenceToReferenceFlowProperty": "1"},
            },
            "modellingAndValidation": {
                "LCIMethod": {"typeOfDataSet": flow_type},
                "complianceDeclarations": _compliance_declarations(),
            },
            "administrativeInformation": _admin_info(
                category="flows", dataset_id=entity.internal_id
            ),
            "flowProperties": {
                "flowProperty": {
                    "@dataSetInternalID": "1",
                    "referenceToFlowPropertyDataSet": _ref(
                        ref_type="flow property data set",
                        ref_id=flow_property_id,
                        short_description=flow_property_name,
                        category="flowproperties",
                    ),
                    "meanValue": "1",
                }
            },
        }
    }


def _process_dataset(entity: CanonicalEntity):
    exchanges = entity.raw.get("exchanges") or []
    exchange_items = []
    reference_flow = None
    for idx, exchange in enumerate(exchanges, 1):
        exchange_items.append(_exchange_item(exchange, idx))
        if exchange.get("isQuantitativeReference") and reference_flow is None:
            reference_flow = str(idx)
    _apply_exchange_allocations(entity, exchanges, exchange_items)
    if reference_flow is None:
        reference_flow = _reference_flow_id(exchange_items)
    functional_unit = _functional_unit_or_other(
        exchanges, exchange_items, reference_flow
    )
    reference_year = _reference_year(entity.raw.get("referenceYear"))
    valid_until = _optional_year(entity.raw.get("dataSetValidUntil"))
    location = _location_code(entity.raw.get("location"))
    data_set_information = {
        "common:UUID": entity.internal_id,
        "name": _name_parts(
            entity.name or "Process",
            location=location,
            source_classification=entity.raw.get("sourceClassification"),
        ),
        "classificationInformation": _default_process_classification(entity.raw),
        "common:generalComment": _ml(
            entity.raw.get("description") or CONVERTED_EXTERNAL_LCA_DATA
        ),
    }
    common_other = _common_other_trace(
        entity.raw.get("sourceTrace"), _location_gap(entity.raw.get("location"))
    )
    if common_other:
        data_set_information["common:other"] = common_other

    time = {}
    if reference_year is not None:
        time["common:referenceYear"] = reference_year
    if valid_until is not None:
        time["common:dataSetValidUntil"] = valid_until
    if _text(entity.raw.get("timeDescription")):
        time["common:timeRepresentativenessDescription"] = _ml(
            str(entity.raw["timeDescription"]).strip()
        )

    location_item = {"@location": location}
    if _text(entity.raw.get("locationDescription")):
        location_item["descriptionOfRestrictions"] = _ml(
            str(entity.raw["locationDescription"]).strip()
        )
    process_information = {
        "dataSetInformation": data_set_information,
        "quantitativeReference": {
            "@type": "Reference flow(s)",
            "referenceToReferenceFlow": reference_flow,
            "functionalUnitOrOther": _ml_500(functional_unit),
        },
        "time": time,
        "geography": {
            "locationOfOperationSupplyOrProduction": location_item,
        },
    }
    technology = _technology_section(entity)
    if technology:
        process_information["technology"] = technology

    modelling_and_validation = {"LCIMethodAndAllocation": _process_lci_method(entity)}
    data_sources = _data_sources_treatment_and_representativeness(entity)
    if data_sources:
        modelling_and_validation["dataSourcesTreatmentAndRepresentativeness"] = (
            data_sources
        )
    modelling_and_validation["validation"] = {"review": _review_item(entity)}
    modelling_and_validation["complianceDeclarations"] = _compliance_declarations(
        process=True
    )
    commissioner_and_goal = {
        "common:referenceToCommissioner": _process_commissioner_ref(entity),
        "common:intendedApplications": _ml(
            str(entity.raw.get("intendedApplications") or CONVERTED_EXTERNAL_LCA_DATA)
        ),
    }
    if _text(entity.raw.get("project")):
        commissioner_and_goal["common:project"] = _ml_500(
            str(entity.raw["project"]).strip()
        )
    administrative_information = {
        "common:commissionerAndGoal": commissioner_and_goal,
        **_admin_info(
            category="processes",
            dataset_id=entity.internal_id,
            version=_dataset_version(entity.raw.get("version")),
            process=True,
            timestamp=entity.raw.get("lastChange") or entity.raw.get("creationDate"),
            data_entry_ref=_contact_ref_from_raw(entity.raw.get("dataEntryByRef")),
            owner_ref=_contact_ref_from_raw(entity.raw.get("dataSetOwnerRef")),
            copyright_value=entity.raw.get("copyright"),
            access_restrictions=entity.raw.get("accessRestrictions"),
        ),
    }
    data_generator_ref = _contact_ref_from_raw(entity.raw.get("dataGeneratorRef"))
    if data_generator_ref:
        administrative_information["dataGenerator"] = {
            "common:referenceToPersonOrEntityGeneratingTheDataSet": data_generator_ref
        }

    return {
        "processDataSet": {
            "@xmlns": "http://lca.jrc.it/ILCD/Process",
            "@xmlns:common": "http://lca.jrc.it/ILCD/Common",
            "@xmlns:xsi": "http://www.w3.org/2001/XMLSchema-instance",
            "@version": "1.1",
            "@locations": "../ILCDLocations.xml",
            "@xsi:schemaLocation": "http://lca.jrc.it/ILCD/Process ../../schemas/ILCD_ProcessDataSet.xsd",
            "processInformation": process_information,
            "modellingAndValidation": modelling_and_validation,
            "administrativeInformation": administrative_information,
            "exchanges": {"exchange": exchange_items},
        }
    }


def _lifecycle_model_dataset(entity: CanonicalEntity) -> dict[str, Any]:
    raw = entity.raw or {}
    process_instances = _lifecycle_process_instances(entity)
    reference_instance = _lifecycle_reference_instance(raw, process_instances)
    dataset_info = {
        "common:UUID": entity.internal_id,
        "name": _name_parts(entity.name or "Life cycle model"),
        "classificationInformation": _default_process_classification(),
        "common:generalComment": _ml(
            raw.get("description") or CONVERTED_EXTERNAL_LCA_DATA
        ),
    }
    common_other = _common_other_trace(raw.get("sourceTrace"))
    if common_other:
        dataset_info["common:other"] = common_other

    return {
        "lifeCycleModelDataSet": {
            "@xmlns": "http://eplca.jrc.ec.europa.eu/ILCD/LifeCycleModel/2017",
            "@xmlns:acme": "http://acme.com/custom",
            "@xmlns:common": "http://lca.jrc.it/ILCD/Common",
            "@xmlns:xsi": "http://www.w3.org/2001/XMLSchema-instance",
            "@locations": "../ILCDLocations.xml",
            "@version": "1.1",
            "@xsi:schemaLocation": "http://eplca.jrc.ec.europa.eu/ILCD/LifeCycleModel/2017 ../../schemas/ILCD_LifeCycleModelDataSet.xsd",
            "lifeCycleModelInformation": {
                "dataSetInformation": dataset_info,
                "quantitativeReference": {
                    "referenceToReferenceProcess": reference_instance
                },
                "technology": {
                    "processes": {"processInstance": process_instances},
                },
            },
            "modellingAndValidation": {
                "validation": {
                    "review": {
                        "common:referenceToNameOfReviewerAndInstitution": _owner_ref(),
                        "common:otherReviewDetails": _ml(REVIEW_NOT_DECLARED),
                    }
                },
                "complianceDeclarations": _compliance_declarations(process=True),
            },
            "administrativeInformation": {
                "common:commissionerAndGoal": {
                    "common:referenceToCommissioner": _owner_ref(),
                    "common:intendedApplications": _ml(CONVERTED_EXTERNAL_LCA_DATA),
                },
                **_admin_info(
                    category="lifecyclemodels",
                    dataset_id=entity.internal_id,
                    version=_dataset_version(raw.get("version")),
                    process=True,
                ),
            },
        }
    }


def _lifecycle_process_instances(entity: CanonicalEntity) -> list[dict[str, Any]]:
    raw = entity.raw or {}
    refs = [
        ref
        for ref in raw.get("processRefs") or []
        if isinstance(ref, dict) and _uuid_text(ref.get("id"))
    ]
    instance_by_process = {
        str(ref["id"]): str(index) for index, ref in enumerate(refs, start=1)
    }
    connections_by_provider: dict[str, dict[str, list[dict[str, Any]]]] = {}
    for connection in raw.get("connections") or []:
        if not isinstance(connection, dict):
            continue
        provider_id = str(connection.get("providerProcessId") or "")
        consumer_id = str(connection.get("consumerProcessId") or "")
        flow_id = str(connection.get("flowRefId") or "")
        if (
            provider_id not in instance_by_process
            or consumer_id not in instance_by_process
            or not _uuid_text(flow_id)
        ):
            continue
        downstream = {
            "@id": instance_by_process[consumer_id],
            "@flowUUID": flow_id,
        }
        location = _location_code(connection.get("location"))
        if connection.get("location") and location != "GLO":
            downstream["@location"] = location
        connections_by_provider.setdefault(provider_id, {}).setdefault(
            flow_id, []
        ).append(downstream)

    instances = []
    for ref in refs:
        process_id = str(ref["id"])
        instance = {
            "@dataSetInternalID": instance_by_process[process_id],
            "@multiplicationFactor": "1",
            "referenceToProcess": _ref(
                ref_type="process data set",
                ref_id=process_id,
                short_description=str(ref.get("name") or "Process"),
                category="processes",
            ),
        }
        output_exchanges = []
        for flow_id, downstream_items in sorted(
            connections_by_provider.get(process_id, {}).items()
        ):
            output_exchanges.append(
                {
                    "@flowUUID": flow_id,
                    "@dominant": "true",
                    "downstreamProcess": (
                        downstream_items[0]
                        if len(downstream_items) == 1
                        else downstream_items
                    ),
                }
            )
        if output_exchanges:
            instance["connections"] = {
                "outputExchange": (
                    output_exchanges[0]
                    if len(output_exchanges) == 1
                    else output_exchanges
                )
            }
        trace = _common_other_trace(
            {
                "sourceProcessType": ref.get("processType"),
                "sourceProcessId": process_id,
            }
        )
        if trace:
            instance["common:other"] = trace
        instances.append(instance)
    return instances


def _lifecycle_reference_instance(
    raw: dict[str, Any], process_instances: list[dict[str, Any]]
) -> str:
    reference_process_id = raw.get("referenceProcessId")
    process_refs = raw.get("processRefs") or []
    for index, ref in enumerate(process_refs, start=1):
        if isinstance(ref, dict) and ref.get("id") == reference_process_id:
            return str(index)
    if process_instances:
        return str(process_instances[0].get("@dataSetInternalID") or "1")
    return "1"


def _default_process_classification(
    raw: dict[str, Any] | None = None,
) -> dict[str, Any]:
    classification = {
        "common:classification": {
            "common:class": deepcopy(PLACEHOLDER_PROCESS_CLASSIFICATION)
        }
    }
    other = _common_other_trace(
        _classification_payload(raw),
        _conversion_gap(
            "classificationInformation.common:classification",
            "unclassified-pending",
            detail="process classification not derivable from source; pending ISIC assignment",
        ),
    )
    if other:
        classification["common:classification"]["common:other"] = other
    return classification


def _process_lci_method(entity: CanonicalEntity) -> dict[str, Any]:
    raw = entity.raw
    section: dict[str, Any] = {
        "typeOfDataSet": _process_type(raw.get("sourceProcessType")),
    }
    allocation = _allocation_method(raw.get("sourceDefaultAllocationMethod"))
    if allocation:
        section["LCIMethodApproaches"] = allocation
    if _text(raw.get("deviationsFromLCIMethodPrinciple")):
        section["deviationsFromLCIMethodPrinciple"] = _ml(
            str(raw["deviationsFromLCIMethodPrinciple"]).strip()
        )
    if _text(raw.get("modellingConstants")):
        section["modellingConstants"] = _ml(str(raw["modellingConstants"]).strip())
    common_other = _common_other_trace(
        {
            "sourceProcessType": raw.get("sourceProcessType"),
            "sourceDefaultAllocationMethod": raw.get("sourceDefaultAllocationMethod"),
        }
    )
    if common_other:
        section["common:other"] = common_other
    return section


def _process_type(value: Any) -> str:
    text = str(value or "").strip().upper()
    if text == "LCI_RESULT":
        return "LCI result"
    if text == "PARTLY_TERMINATED_SYSTEM":
        return "Partly terminated system"
    if text == "AVOIDED_PRODUCT_SYSTEM":
        return "Avoided product system"
    return "Unit process, single operation"


def _allocation_method(value: Any) -> str | None:
    text = str(value or "").strip().upper()
    return {
        "ECONOMIC_ALLOCATION": "Allocation - market value",
        "PHYSICAL_ALLOCATION": "Allocation - physical causality",
        "CAUSAL_ALLOCATION": "Allocation - physical causality",
        "NO_ALLOCATION": "Not applicable",
    }.get(text)


def _process_commissioner_ref(entity: CanonicalEntity) -> dict[str, Any]:
    return (
        _contact_ref_from_raw(entity.raw.get("dataSetOwnerRef"))
        or _contact_ref_from_raw(entity.raw.get("dataGeneratorRef"))
        or _owner_ref()
    )


def _data_quality_indicators(entity: CanonicalEntity) -> dict[str, Any] | None:
    indicators = [
        indicator
        for indicator in entity.raw.get("dataQualityIndicators") or []
        if isinstance(indicator, dict)
        and indicator.get("@name")
        and indicator.get("@value")
    ]
    if not indicators:
        return None
    payload = indicators if len(indicators) > 1 else indicators[0]
    return {"common:dataQualityIndicator": payload}


def _review_item(entity: CanonicalEntity) -> dict[str, Any]:
    reviews = [
        review
        for review in entity.raw.get("sourceReviews") or []
        if isinstance(review, dict)
    ]
    quality_indicators = _data_quality_indicators(entity)
    if not reviews:
        item = {"@type": "Not reviewed"}
        if quality_indicators:
            item["common:dataQualityIndicators"] = quality_indicators
        return item

    primary = reviews[0]
    review_type = _review_type(primary.get("reviewType"))
    if review_type is None:
        item = {
            "@type": "Not reviewed",
            "common:other": _common_other_trace(
                {
                    "sourceReviews": reviews,
                    "mappingNote": (
                        "Source review records exist, but reviewType could not "
                        "be mapped to a TIDAS review enum without inference."
                    ),
                }
            ),
        }
        if quality_indicators:
            item["common:dataQualityIndicators"] = quality_indicators
        return item

    report_ref = (
        _source_ref_from_raw(primary.get("reportRef"))
        or _source_ref_from_raw(entity.raw.get("publicationRef"))
        or _ref(
            ref_type="source data set",
            ref_id=FORMAT_SOURCE_ID,
            short_description=DATA_SOURCE_NOT_DECLARED,
            category="sources",
        )
    )
    details = str(primary.get("details") or REVIEW_NOT_DECLARED).strip()
    item = {
        "@type": review_type,
        "common:scope": {
            "@name": "Documentation",
            "common:method": {"@name": "Documentation"},
        },
        "common:reviewDetails": _ml(details),
        "common:referenceToNameOfReviewerAndInstitution": _process_commissioner_ref(
            entity
        ),
        "common:referenceToCompleteReviewReport": report_ref,
    }
    other = _common_other_trace(
        {
            "sourceReviews": reviews,
            "mappingNote": (
                "openLCA JSON-LD review records were mapped by rule. Review "
                "scope/method defaults to Documentation because openLCA review "
                "details do not use the ILCD scope/method controlled lists."
            ),
        }
    )
    if other:
        item["common:other"] = other
    if quality_indicators:
        item["common:dataQualityIndicators"] = quality_indicators
    return item


def _review_type(value: Any) -> str | None:
    text = str(value or "").strip()
    allowed = {
        "Dependent internal review",
        "Independent internal review",
        "Independent external review",
        "Accredited third party review",
        "Independent review panel",
    }
    if text in allowed:
        return text
    normalized = text.casefold()
    if "accredited" in normalized and "third" in normalized:
        return "Accredited third party review"
    if "panel" in normalized:
        return "Independent review panel"
    if "external" in normalized:
        return "Independent external review"
    if "internal" in normalized and "independent" in normalized:
        return "Independent internal review"
    if "internal" in normalized and "dependent" in normalized:
        return "Dependent internal review"
    return None


def _source_ref_from_raw(value: Any) -> dict[str, Any] | None:
    if not isinstance(value, dict) or not _uuid_text(value.get("id")):
        return None
    return _ref(
        ref_type="source data set",
        ref_id=value["id"],
        short_description=str(value.get("name") or DATA_SOURCE_NOT_DECLARED),
        category="sources",
    )


def _technology_section(entity: CanonicalEntity) -> dict[str, Any] | None:
    description = entity.raw.get("technologyDescription")
    applicability = entity.raw.get("technologicalApplicability")
    if not _text(description) and not _text(applicability):
        return None
    section = {
        "technologyDescriptionAndIncludedProcesses": _ml(
            str(description).strip()
            if _text(description)
            else SOURCE_VALUE_NOT_DECLARED
        )
    }
    if _text(applicability):
        section["technologicalApplicability"] = _ml(str(applicability).strip())
    return section


def _data_sources_treatment_and_representativeness(
    entity: CanonicalEntity,
) -> dict[str, Any] | None:
    raw = entity.raw
    source_refs = raw.get("sourceRefs") or []
    relevant_keys = {
        "productionVolume",
        "samplingProcedure",
        "uncertaintyAdjustments",
        "useAdviceForDataSet",
        "dataCollectionPeriod",
        "dataSelectionAndCombinationPrinciples",
        "dataTreatmentAndExtrapolationsPrinciples",
        "dataCutOffAndCompletenessPrinciples",
    }
    # dataSourcesTreatmentAndRepresentativeness (and its annualSupplyOrProductionVolume)
    # is schema-required, so always emit the block — the fields below already fall back
    # to format-source / placeholder values when the source has no documentation.

    source_refs_payload = [
        _ref(
            ref_type="source data set",
            ref_id=source_ref.get("id"),
            short_description=str(
                source_ref.get("name")
                or raw.get("sourceLabel")
                or DATA_SOURCE_NOT_DECLARED
            ),
            category="sources",
        )
        for source_ref in source_refs
        if _uuid_text(source_ref.get("id"))
    ]
    if not source_refs_payload:
        source_refs_payload = [
            _ref(
                ref_type="source data set",
                ref_id=FORMAT_SOURCE_ID,
                short_description=DATA_SOURCE_NOT_DECLARED,
                category="sources",
            )
        ]
    section: dict[str, Any] = {
        "dataCutOffAndCompletenessPrinciples": _ml(
            str(
                raw.get("dataCutOffAndCompletenessPrinciples")
                or raw.get("technologyDescription")
                or CONVERTED_EXTERNAL_LCA_DATA
            ).strip()
        )
    }
    if source_refs_payload:
        section["referenceToDataSource"] = (
            source_refs_payload[0]
            if len(source_refs_payload) == 1
            else source_refs_payload
        )
    annual_supply = _annual_supply_or_production_volume(
        raw.get("productionVolume"),
        fallback_unit=_reference_flow_unit(raw.get("exchanges") or []),
    )
    if annual_supply:
        section["annualSupplyOrProductionVolume"] = annual_supply
    if _text(raw.get("dataSelectionAndCombinationPrinciples")):
        section["dataSelectionAndCombinationPrinciples"] = _ml(
            str(raw["dataSelectionAndCombinationPrinciples"]).strip()
        )
    if _text(raw.get("dataTreatmentAndExtrapolationsPrinciples")):
        section["dataTreatmentAndExtrapolationsPrinciples"] = _ml(
            str(raw["dataTreatmentAndExtrapolationsPrinciples"]).strip()
        )
    if _text(raw.get("samplingProcedure")):
        section["samplingProcedure"] = _ml(str(raw["samplingProcedure"]).strip())
    if _text(raw.get("dataCollectionPeriod")):
        section["dataCollectionPeriod"] = _ml_500(
            str(raw["dataCollectionPeriod"]).strip()
        )
    if _text(raw.get("uncertaintyAdjustments")):
        section["uncertaintyAdjustments"] = _ml(
            str(raw["uncertaintyAdjustments"]).strip()
        )
    if _text(raw.get("useAdviceForDataSet")):
        section["useAdviceForDataSet"] = _ml(str(raw["useAdviceForDataSet"]).strip())

    return section


def _exchange_item(exchange: dict[str, Any], idx: int) -> dict[str, Any]:
    flow = exchange.get("flow") if isinstance(exchange.get("flow"), dict) else {}
    flow_id = (
        _extract_ref_id(flow) or exchange.get("flowRefId") or DEFAULT_FLOW_PROPERTY_ID
    )
    flow_name = _extract_name(flow) or exchange.get("flowName") or "Flow"
    amount = _real(exchange.get("amount", 1))
    is_input = bool(exchange.get("isInput", False))
    derivation_type = _data_derivation_type(exchange.get("dataDerivationTypeStatus"))
    if derivation_type is None:
        derivation_type = "Unknown derivation"
    item = {
        "@dataSetInternalID": str(idx),
        "referenceToFlowDataSet": _ref(
            ref_type="flow data set",
            ref_id=flow_id,
            short_description=flow_name,
            category="flows",
        ),
        "exchangeDirection": "Input" if is_input else "Output",
        "meanAmount": amount,
        "resultingAmount": amount,
    }
    location = _location_code(exchange.get("location"))
    if exchange.get("location") and location != "GLO":
        item["location"] = location
    minimum_amount = _optional_real(exchange.get("minimumAmount"))
    if minimum_amount is not None:
        item["minimumAmount"] = minimum_amount
    maximum_amount = _optional_real(exchange.get("maximumAmount"))
    if maximum_amount is not None:
        item["maximumAmount"] = maximum_amount
    uncertainty_type = _uncertainty_distribution_type(
        exchange.get("uncertaintyDistributionType")
    )
    if uncertainty_type:
        item["uncertaintyDistributionType"] = uncertainty_type
    relative_std_dev = _percentage(exchange.get("relativeStandardDeviation95In"))
    if relative_std_dev:
        item["relativeStandardDeviation95In"] = relative_std_dev
    item["dataDerivationTypeStatus"] = derivation_type

    comments = []
    if exchange.get("sourceExchangeNumber"):
        comments.append(
            f"Source EcoSpold1 exchange number: {exchange['sourceExchangeNumber']}."
        )
    elif exchange.get("sourceExchangeId"):
        comments.append(
            f"Source EcoSpold2 exchange id: {exchange['sourceExchangeId']}."
        )
    elif exchange.get("internalId") is not None:
        comments.append(f"Source exchange internal id: {exchange['internalId']}.")
    if _text(exchange.get("generalComment")):
        comments.append(str(exchange["generalComment"]).strip())
    if comments:
        item["generalComment"] = _ml_500(" ".join(comments))

    common_other = _common_other_trace(
        _exchange_trace_payload(exchange),
    )
    if common_other:
        item["common:other"] = common_other
    return item


def _allocation_fraction(value: Any) -> str | None:
    """openLCA allocationFactor.value (0..1 fraction) -> TIDAS Perc (% 0..100)."""
    try:
        amount = value if isinstance(value, Decimal) else Decimal(str(value))
    except (InvalidOperation, TypeError, ValueError):
        return None
    return _percentage(str((amount * 100).quantize(Decimal("0.001"))))


def _apply_exchange_allocations(
    entity: CanonicalEntity,
    exchanges: list[dict[str, Any]],
    exchange_items: list[dict[str, Any]],
) -> None:
    """Map openLCA process allocationFactors onto the TIDAS exchange allocations.

    eILCD allows a list of <allocation> per exchange (maxOccurs unbounded), so a
    multifunctional process whose inputs/elementary flows are split across several
    co-products (causal/physical/economic allocation) is represented losslessly:
    each allocated exchange carries one allocation entry per co-product with the
    co-product output's @dataSetInternalID and the allocated fraction. Sparse
    zero-fraction cells are omitted (the listed non-zero fractions sum to 100%).
    """
    factors = entity.raw.get("allocationFactors")
    if not isinstance(factors, list) or not factors:
        return

    def flow_id(exchange: dict[str, Any]) -> str | None:
        flow = exchange.get("flow")
        return flow.get("@id") if isinstance(flow, dict) else None

    # openLCA allocationFactor.exchange is an Exchange object (internalId + flow);
    # match the allocated exchange by its source internalId. The co-product is a
    # Flow ref matched to the output exchange carrying that flow.
    item_by_internal: dict[Any, dict[str, Any]] = {}
    coproduct_internal: dict[str, str] = {}
    for exchange, item in zip(exchanges, exchange_items):
        internal = exchange.get("internalId")
        if internal is not None:
            item_by_internal[internal] = item
        fid = flow_id(exchange)
        if fid and not exchange.get("isInput", False):
            coproduct_internal.setdefault(fid, item.get("@dataSetInternalID"))

    allocations_by_item: dict[int, list[dict[str, str]]] = {}
    for factor in factors:
        if not isinstance(factor, dict):
            continue
        allocated = factor.get("exchange")
        product = factor.get("product")
        allocated_internal = (
            allocated.get("internalId") if isinstance(allocated, dict) else None
        )
        product_fid = product.get("@id") if isinstance(product, dict) else None
        target = item_by_internal.get(allocated_internal)
        coproduct = coproduct_internal.get(product_fid or "")
        fraction = _allocation_fraction(factor.get("value"))
        if target is None or coproduct is None or fraction is None:
            continue
        if fraction in ("0", "0.0", "0.00", "0.000"):
            continue  # sparse zero cell; the listed non-zero fractions sum to 100%
        allocations_by_item.setdefault(id(target), []).append(
            {
                "@internalReferenceToCoProduct": coproduct,
                "@allocatedFraction": fraction,
            }
        )

    for item in exchange_items:
        entries = allocations_by_item.get(id(item))
        if entries:
            item["allocations"] = {
                "allocation": entries if len(entries) > 1 else entries[0]
            }


def _reference_flow_id(exchange_items: list[dict[str, Any]]) -> str:
    for exchange in exchange_items:
        if exchange.get("exchangeDirection") == "Output":
            return str(exchange.get("@dataSetInternalID") or "1")
    if exchange_items:
        return str(exchange_items[0].get("@dataSetInternalID") or "1")
    return "1"


def _functional_unit_or_other(
    exchanges: list[dict[str, Any]],
    exchange_items: list[dict[str, Any]],
    reference_flow: str,
) -> str:
    for index, item in enumerate(exchange_items):
        if str(item.get("@dataSetInternalID") or "") != str(reference_flow):
            continue
        source = exchanges[index] if index < len(exchanges) else {}
        amount = (
            item.get("meanAmount")
            or item.get("resultingAmount")
            or source.get("amount")
            or "1"
        )
        unit = str(source.get("unitName") or source.get("unit") or "").strip()
        ref = item.get("referenceToFlowDataSet")
        name = ""
        if isinstance(ref, dict):
            description = ref.get("common:shortDescription")
            if isinstance(description, dict):
                name = str(description.get("#text") or "").strip()
            elif description is not None:
                name = str(description).strip()
        parts = [str(amount).strip()]
        if unit:
            parts.append(unit)
        if name:
            parts.append(name)
        return " ".join(part for part in parts if part) or "1 reference flow"
    return "1 reference flow"


def _exchange_trace_payload(exchange: dict[str, Any]) -> dict[str, Any] | None:
    source_trace = exchange.get("sourceTrace")
    extra = {
        "activityLinkId": exchange.get("activityLinkId"),
        "productionVolumeAmount": exchange.get("productionVolumeAmount"),
        "isCalculatedAmount": exchange.get("isCalculatedAmount"),
        "amountFormula": exchange.get("amountFormula"),
        "isQuantitativeReference": exchange.get("isQuantitativeReference"),
        "isAvoidedProduct": exchange.get("isAvoidedProduct"),
        "unitId": exchange.get("unitId"),
        "unitName": exchange.get("unitName"),
        "flowPropertyRefId": exchange.get("flowPropertyRefId"),
        "flowPropertyName": exchange.get("flowPropertyName"),
        "providerRefId": exchange.get("providerRefId"),
        "providerName": exchange.get("providerName"),
        "provider": exchange.get("provider"),
        "dqEntry": exchange.get("dqEntry"),
        "sourceLocation": exchange.get("sourceLocation"),
    }
    if not source_trace and not any(value is not None for value in extra.values()):
        return None
    return {
        "sourceTrace": source_trace,
        "exchangeMetadata": {
            key: value for key, value in extra.items() if value is not None
        },
    }


def _name_parts(
    name: str,
    *,
    location: str | None = None,
    source_classification: Any = None,
) -> dict[str, Any]:
    route = _route_name_part(name)
    mix = _mix_location_name_part(name, location, source_classification)
    return {
        "baseName": _ml_500(name),
        "treatmentStandardsRoutes": _ml_500(route),
        "mixAndLocationTypes": _ml_500(mix),
    }


def _route_name_part(name: str) -> str:
    text = str(name or "").strip()
    lowered = text.casefold()
    if " at plant" in lowered:
        return "at plant"
    if lowered.startswith("market for ") or " market " in lowered:
        return "market"
    if " production" in lowered or "production of " in lowered:
        return "production"
    return "source-described route"


def _mix_location_name_part(
    name: str,
    location: str | None,
    source_classification: Any,
) -> str:
    text = str(name or "")
    match = re.search(r"\{([^{}]+)\}", text)
    if match and match.group(1).strip():
        return match.group(1).strip()
    if _text(location):
        return str(location).strip()
    if isinstance(source_classification, dict):
        category = source_classification.get("category")
        if _text(category):
            return str(category).strip()
    return "source-described geography"


def _flow_type(value: Any) -> str:
    text = str(value or "PRODUCT_FLOW").upper()
    if "ELEMENTARY" in text:
        return "Elementary flow"
    if "WASTE" in text:
        return "Waste flow"
    return "Product flow"


def _generated_unit_entities(
    store: MemoryCanonicalStore,
) -> dict[str, tuple[CanonicalEntity, CanonicalEntity]]:
    explicit_flow_property_flows = {
        entity.internal_id
        for entity in store.iter_type("flows")
        if entity.raw.get("flowPropertyRefId")
    }
    unit_names = sorted(
        {
            str(entity.raw.get("unitName")).strip()
            for entity in store.iter_type("flows")
            if entity.internal_id not in explicit_flow_property_flows
            and _text(entity.raw.get("unitName"))
        }
    )
    result = {}
    for unit_name in unit_names:
        unit_group_id = str(
            uuid.uuid5(
                uuid.NAMESPACE_URL,
                f"{IMPORT_ID_NAMESPACE}/unitgroup/{unit_name}",
            )
        )
        flow_property_id = str(
            uuid.uuid5(
                uuid.NAMESPACE_URL,
                f"{IMPORT_ID_NAMESPACE}/flowproperty/{unit_name}",
            )
        )
        unit_group = CanonicalEntity(
            entity_type="unitgroups",
            internal_id=unit_group_id,
            name=f"Units of {unit_name}",
            raw={"units": [{"name": unit_name, "conversionFactor": 1.0}]},
        )
        flow_property = CanonicalEntity(
            entity_type="flowproperties",
            internal_id=flow_property_id,
            name=f"Amount in {unit_name}",
            raw={"unitGroupRefId": unit_group_id, "unitGroupName": unit_group.name},
        )
        result[unit_name] = (unit_group, flow_property)
    return result


def _generated_flow_property_for(
    entity: CanonicalEntity,
    generated_units: dict[str, tuple[CanonicalEntity, CanonicalEntity]] | None,
) -> CanonicalEntity | None:
    unit_name = entity.raw.get("unitName")
    if not _text(unit_name) or not generated_units:
        return None
    generated = generated_units.get(str(unit_name).strip())
    return generated[1] if generated else None


def _reference_year(value: Any) -> int | None:
    if value is None:
        return 9999
    text = str(value).strip()
    match = re.search(r"\b(\d{4})\b", text)
    if not match:
        return 9999
    return int(match.group(1))


def _optional_year(value: Any) -> int | None:
    if value is None:
        return None
    text = str(value).strip()
    match = re.search(r"\b(\d{4})\b", text)
    if not match:
        return None
    return int(match.group(1))


def _dataset_version(value: Any) -> str:
    if value is None:
        return DEFAULT_VERSION
    text = str(value).strip()
    return text or DEFAULT_VERSION


def _date_time(value: Any) -> str | None:
    if value is None:
        return None
    text = str(value).strip()
    if "T" not in text:
        return None
    if text.endswith("Z"):
        return text
    if re.search(r"[+-]\d{2}:\d{2}$", text):
        return text.replace("+00:00", "Z")
    return None


def _bool_text(value: Any) -> str:
    if isinstance(value, bool):
        return "true" if value else "false"
    text = str(value or "").strip().casefold()
    if text in {"true", "1", "yes"}:
        return "true"
    return "false"


def _uuid_text(value: Any) -> bool:
    return isinstance(value, str) and bool(
        re.fullmatch(
            r"[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}",
            value,
        )
    )


def _cas_number(value: Any) -> str | None:
    if value is None:
        return None
    text = str(value).strip()
    if not re.fullmatch(r"[0-9]{2,7}-[0-9]{2}-[0-9]", text):
        return None
    body, check_digit = text.rsplit("-", 1)
    digits = body.replace("-", "")
    checksum = sum(
        int(digit) * weight for weight, digit in enumerate(reversed(digits), start=1)
    )
    return text if checksum % 10 == int(check_digit) else None


def _location_code(value: Any) -> str:
    if value is None:
        return "GLO"
    text = str(value).strip()
    if not text:
        return "GLO"
    if text in _location_codes():
        return text
    return "GLO"


def _location_gap(value: Any) -> dict[str, Any] | None:
    """Mark a conversion gap when a declared source location is not a known
    TIDAS location code and therefore gets collapsed to GLO by _location_code.

    Returns None when the source has no location or the code is recognised
    (including aggregate regions like RoW now that they are in the enum)."""
    if value is None:
        return None
    text = str(value).strip()
    if not text or text in _location_codes():
        return None
    return _conversion_gap(
        "geography.locationOfSupply",
        "placeholder-location",
        detail=f"source location {text!r} not in TIDAS location list; defaulted to GLO",
    )


@lru_cache(maxsize=1)
def _location_codes() -> frozenset[str]:
    schema_path = (
        Path(__file__).resolve().parents[2]
        / "tidas"
        / "schemas"
        / "tidas_locations_category.json"
    )
    try:
        schema = json.loads(schema_path.read_text(encoding="utf-8"))
    except OSError:
        return frozenset({"GLO"})
    return frozenset(
        option["const"]
        for option in schema.get("oneOf", [])
        if isinstance(option, dict) and isinstance(option.get("const"), str)
    )


def _elementary_categorization(
    raw: dict[str, Any] | None,
) -> list[dict[str, str]] | None:
    """Map a FEDEFL/openLCA compartment path to the TIDAS elementary category tree.

    Returns the [L0, L1, L2] category list when the source carries a recognizable
    FEDEFL emission/resource compartment path (e.g.
    "Elementary flows/emission/air/troposphere/rural"); returns None otherwise so
    the caller keeps the legacy default. ecoSpold single-token compartments
    ("air"/"resource") and non-FEDEFL shapes have no recognizable medium and yield
    None, so this never changes the ecoSpold/BAFU output. Compartment text/catIds
    match tidas_flows_elementary_category.json.
    """
    if not raw:
        return None
    raw_path = raw.get("category")
    if not raw_path and isinstance(raw.get("sourceCategoryPath"), list):
        raw_path = "/".join(str(part) for part in raw["sourceCategoryPath"])
    segments = [segment.strip().lower() for segment in str(raw_path or "").split("/")]
    segments = [
        segment
        for segment in segments
        if segment and segment not in ("elementary flows", "non-fedefl")
    ]
    if len(segments) < 2:
        return None
    direction, medium = segments[0], segments[1]
    sub = " ".join(segments[2:])

    def path(
        level1: tuple[str, str], leaf: tuple[str, str], root: tuple[str, str]
    ) -> list[dict[str, str]]:
        return [
            {"@level": "0", "@catId": root[0], "#text": root[1]},
            {"@level": "1", "@catId": level1[0], "#text": level1[1]},
            {"@level": "2", "@catId": leaf[0], "#text": leaf[1]},
        ]

    emissions = ("1", "Emissions")
    if direction == "emission":
        if medium == "air":
            if "indoor" in sub:
                leaf = ("1.3.4", "Emissions to air, unspecified")
            elif "stratosphere" in sub:
                leaf = (
                    "1.3.3",
                    "Emissions to lower stratosphere and upper troposphere",
                )
            elif "urban" in sub:
                leaf = ("1.3.1", "Emissions to urban air close to ground")
            elif "rural" in sub or "troposphere" in sub or "high" in sub:
                leaf = ("1.3.2", "Emissions to non-urban air or from high stacks")
            else:
                leaf = ("1.3.4", "Emissions to air, unspecified")
            return path(("1.3", "Emissions to air"), leaf, emissions)
        if medium == "water":
            if "saline" in sub or "ocean" in sub or "sea" in sub:
                leaf = ("1.1.2", "Emissions to sea water")
            elif "fresh" in sub or "river" in sub or "lake" in sub:
                leaf = ("1.1.1", "Emissions to fresh water")
            else:
                leaf = ("1.1.3", "Emissions to water, unspecified")
            return path(("1.1", "Emissions to water"), leaf, emissions)
        if medium in ("ground", "soil"):
            if "agricultur" in sub:
                leaf = ("1.2.1", "Emissions to agricultural soil")
            elif "industri" in sub or "forest" in sub or "terrestrial" in sub:
                leaf = ("1.2.2", "Emissions to non-agricultural soil")
            else:
                leaf = ("1.2.3", "Emissions to soil, unspecified")
            return path(("1.2", "Emissions to soil"), leaf, emissions)
        return None
    if direction == "resource":
        resources = ("2", "Resources")
        if medium == "ground":
            return path(
                ("2.1", "Resources from ground"),
                ("2.1.8", "Non-renewable resources from ground, unspecified"),
                resources,
            )
        if medium == "water":
            return path(
                ("2.2", "Resources from water"),
                ("2.2.7", "Renewable resources from water, unspecified"),
                resources,
            )
        if medium == "air":
            return path(
                ("2.3", "Resources from air"),
                ("2.3.7", "Renewable resources from air, unspecified"),
                resources,
            )
        if medium in ("biotic", "biosphere"):
            return path(
                ("2.4", "Resources from biosphere"),
                ("2.4.5", "Renewable resources from biosphere, unspecified"),
                resources,
            )
        return None
    return None


def _flow_classification(
    flow_type: str, raw: dict[str, Any] | None = None
) -> dict[str, Any]:
    payload = _classification_payload(raw)
    if flow_type == "Elementary flow":
        categories = _elementary_categorization(raw)
        gap = None
        if categories is None:
            categories = deepcopy(PLACEHOLDER_ELEMENTARY_CATEGORY)
            gap = _conversion_gap(
                "classificationInformation.common:elementaryFlowCategorization",
                "placeholder-compartment",
                detail="source compartment not mapped; defaulted to air-unspecified",
            )
        classification = {
            "common:elementaryFlowCategorization": {"common:category": categories}
        }
        other = _common_other_trace(payload, gap)
        if other:
            classification["common:elementaryFlowCategorization"][
                "common:other"
            ] = other
        return classification
    classification = {
        "common:classification": {
            "common:class": deepcopy(PLACEHOLDER_PRODUCT_CLASSIFICATION)
        }
    }
    other = _common_other_trace(
        payload,
        _conversion_gap(
            "classificationInformation.common:classification",
            "unclassified-pending",
            detail="product flow classification not derivable from source; pending CPC assignment",
        ),
    )
    if other:
        classification["common:classification"]["common:other"] = other
    return classification


def _classification_payload(raw: dict[str, Any] | None) -> dict[str, Any] | None:
    if not raw:
        return None
    return {
        "sourceCategoryPath": raw.get("sourceCategoryPath"),
        "sourceCategory": raw.get("category"),
        "sourceFlowType": raw.get("flowType"),
        "sourceProcessType": raw.get("sourceProcessType"),
    }


def _real(value: Any) -> str:
    text = _real_text(value)
    return text if text is not None else "0"


def _optional_real(value: Any) -> str | None:
    return _real_text(value)


def _real_text(value: Any) -> str | None:
    if value is None:
        return None
    text = str(value).strip()
    return text if REAL_PATTERN.fullmatch(text) else None


def _percentage(value: Any) -> str | None:
    if value is None:
        return None
    text = str(value).strip()
    return text if PERCENT_PATTERN.fullmatch(text) else None


def _annual_supply_or_production_volume(
    value: Any,
    *,
    fallback_unit: str | None = None,
) -> dict[str, str] | None:
    text = str(value).strip() if value is not None else ""
    fallback = _unavailable_annual_supply(fallback_unit)
    if text.casefold() in {"", "<null>", "null", "na", "n/a"}:
        return fallback
    if not re.fullmatch(r"[+-]?(\d+(\.\d*)?|\.\d+)([Ee][+-]?\d+)?\s+\S.*", text):
        return fallback
    if not re.search(
        r"(?:/\s*(?:year|yr|a)\b|\bper\s+(?:year|annum)\b|/\s*年|每年|年度|年供应|年产)",
        text,
        flags=re.IGNORECASE,
    ):
        text = f"{text}/year"
    return _ml(text[:500])


def _unavailable_annual_supply(fallback_unit: str | None = None) -> dict[str, str]:
    unit = str(fallback_unit or "unit").strip() or "unit"
    return _ml(f"0 {unit}/year; {ANNUAL_VOLUME_UNAVAILABLE}")


def _reference_flow_unit(exchanges: list[dict[str, Any]]) -> str | None:
    for exchange in exchanges:
        if not exchange.get("isInput"):
            unit = exchange.get("unitName") or exchange.get("unit")
            if _text(unit):
                return str(unit).strip()
    for exchange in exchanges:
        unit = exchange.get("unitName") or exchange.get("unit")
        if _text(unit):
            return str(unit).strip()
    return None


def _data_derivation_type(value: Any) -> str | None:
    text = str(value or "").strip()
    allowed = {
        "Measured",
        "Calculated",
        "Estimated",
        "Unknown derivation",
        "Missing important",
        "Missing unimportant",
    }
    return text if text in allowed else None


def _uncertainty_distribution_type(value: Any) -> str | None:
    text = str(value or "").strip()
    allowed = {"undefined", "log-normal", "normal", "triangular", "uniform"}
    return text if text in allowed else None


def _common_other_trace(
    payload: Any, gap: dict[str, Any] | None = None
) -> dict[str, Any] | None:
    cleaned = _clean_trace_value(payload)
    parts: dict[str, Any] = {}
    if cleaned not in (None, {}, []):
        parts["tidasimport:sourceTrace"] = {
            "@marker": IMPORT_TRACE_MARKER,
            "payload": cleaned,
        }
    if gap:
        parts["tidasimport:conversionGap"] = {"@marker": IMPORT_GAP_MARKER, **gap}
    if not parts:
        return None
    return {"@xmlns:tidasimport": IMPORT_TRACE_NAMESPACE, **parts}


def _conversion_gap(
    field: str, status: str, *, detail: str | None = None
) -> dict[str, Any]:
    """Build a conversion-gap marker for a placeholder the importer emitted."""
    gap: dict[str, Any] = {"field": field, "status": status}
    if detail:
        gap["detail"] = detail
    return gap


def scan_conversion_gaps(package_dir: str | Path) -> list[dict[str, Any]]:
    """Collect importer placeholder markers from a written TIDAS package.

    Walks every dataset JSON and returns one entry per placeholder the importer
    emitted (unclassified classifications, unmapped compartments/locations,
    placeholder timestamps). This is the repair worklist, and the primitive an
    export guard can use to avoid shipping un-repaired placeholders as if they
    were real data."""
    root = Path(package_dir)
    gaps: list[dict[str, Any]] = []
    for path in sorted(root.rglob("*.json")):
        try:
            payload = json.loads(path.read_text(encoding="utf-8"))
        except (OSError, json.JSONDecodeError):
            continue
        try:
            rel = str(path.relative_to(root))
        except ValueError:
            rel = str(path)
        for marker in _iter_gap_markers(payload):
            gaps.append({"file": rel, **marker})
    return gaps


def _iter_gap_markers(node: Any) -> Iterator[dict[str, Any]]:
    if isinstance(node, dict):
        gap = node.get("tidasimport:conversionGap")
        if isinstance(gap, dict) and gap.get("@marker") == IMPORT_GAP_MARKER:
            yield {key: value for key, value in gap.items() if key != "@marker"}
        if node.get("common:timeStamp") == PLACEHOLDER_TIMESTAMP:
            yield {
                "field": "administrativeInformation.dataEntryBy.common:timeStamp",
                "status": "placeholder-timestamp",
            }
        for value in node.values():
            yield from _iter_gap_markers(value)
    elif isinstance(node, list):
        for item in node:
            yield from _iter_gap_markers(item)


def _clean_trace_value(value: Any) -> Any:
    if value is None:
        return None
    if isinstance(value, Decimal):
        return {
            "@type": "TIDAS_IMPORT_DECIMAL",
            "#text": str(value),
        }
    if isinstance(value, (str, int, float, bool)):
        return value
    if isinstance(value, list):
        if not value:
            return {"@marker": "TIDAS_IMPORT_EMPTY_ARRAY"}
        return [_clean_trace_value(item) for item in value]
    if isinstance(value, dict):
        cleaned = {}
        for key, item in value.items():
            cleaned_item = _clean_trace_value(item)
            if cleaned_item is None:
                continue
            cleaned[_safe_trace_key(str(key))] = cleaned_item
        return cleaned
    return str(value)


def _safe_trace_key(key: str) -> str:
    if key.startswith("@"):
        return key
    safe = re.sub(r"[^A-Za-z0-9_.:-]", "_", key)
    if not safe or not re.match(r"^[A-Za-z_]", safe):
        safe = f"value_{safe}"
    if safe.startswith(("common:", "xmlns:")):
        safe = f"tidasimport_{safe.replace(':', '_')}"
    return safe


def _extract_ref_id(ref: dict[str, Any]) -> str | None:
    for key in ("@id", "refId", "id"):
        value = ref.get(key)
        if value:
            return str(value)
    return None


def _extract_name(ref: dict[str, Any]) -> str | None:
    return _localized(ref.get("name") or ref.get("shortDescription"))


def _localized(value: Any) -> str | None:
    if isinstance(value, str):
        return value
    if isinstance(value, dict):
        for key in ("#text", "en", "default", "value"):
            if value.get(key):
                return str(value[key])
    if isinstance(value, list) and value:
        return _localized(value[0])
    return None


def _text(value: Any) -> bool:
    return isinstance(value, str) and bool(value.strip())


def _email(value: Any) -> bool:
    text = str(value).strip() if value is not None else ""
    return "@" in text and "." in text.rsplit("@", 1)[-1]


def _uri(value: Any) -> bool:
    text = str(value).strip() if value is not None else ""
    return "://" in text or text.startswith(("urn:", "file:"))


def _publication_type(value: Any) -> bool:
    return str(value).strip() in {
        "Undefined",
        "Article in periodical",
        "Chapter in anthology",
        "Monograph",
        "Direct measurement",
        "Oral communication",
        "Personal written communication",
        "Questionnaire",
        "Software or database",
        "Other unpublished and grey literature",
    }
