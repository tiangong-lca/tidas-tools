"""Write canonical entities as validation-compatible TIDAS JSON packages."""

from __future__ import annotations

from datetime import datetime, timezone
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
PLACEHOLDER_PREFIX = "TIDAS_IMPORT_PLACEHOLDER"
PLACEHOLDER_UNSPECIFIED_TEXT = f"{PLACEHOLDER_PREFIX}:UNSPECIFIED_TEXT"
PLACEHOLDER_IMPORT_ACTOR = f"{PLACEHOLDER_PREFIX}:IMPORT_ACTOR"
PLACEHOLDER_COMPLIANCE_NOT_DEFINED = f"{PLACEHOLDER_PREFIX}:COMPLIANCE_NOT_DEFINED"
PLACEHOLDER_DATA_CUTOFF = f"{PLACEHOLDER_PREFIX}:DATA_CUTOFF_NOT_AVAILABLE"
PLACEHOLDER_DATA_SOURCE = f"{PLACEHOLDER_PREFIX}:DATA_SOURCE_NOT_AVAILABLE"
PLACEHOLDER_ANNUAL_VOLUME = f"0 {PLACEHOLDER_PREFIX}:ANNUAL_VOLUME_NOT_AVAILABLE"
PLACEHOLDER_CONVERTED_APPLICATION = f"{PLACEHOLDER_PREFIX}:CONVERTED_EXTERNAL_LCA_DATA"
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


def _write_json(path: Path, payload: dict[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(payload, indent=2, ensure_ascii=False) + "\n")


def _ml(text: str, lang: str = "en") -> dict[str, str]:
    return {"@xml:lang": lang, "#text": text or PLACEHOLDER_UNSPECIFIED_TEXT}


def _now() -> str:
    return datetime.now(timezone.utc).replace(microsecond=0).isoformat()


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
        short_description=PLACEHOLDER_COMPLIANCE_NOT_DEFINED,
        category="sources",
    )


def _owner_ref() -> dict[str, Any]:
    return _ref(
        ref_type="contact data set",
        ref_id=IMPORT_CONTACT_ID,
        short_description=PLACEHOLDER_IMPORT_ACTOR,
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
) -> dict[str, Any]:
    data_entry = {
        "common:timeStamp": _now(),
        "common:referenceToDataSetFormat": _format_ref(),
    }
    if process:
        data_entry["common:referenceToPersonOrEntityEnteringTheData"] = _owner_ref()

    publication = {
        "common:dataSetVersion": version,
        "common:permanentDataSetURI": _permanent_dataset_uri(
            category, dataset_id, version
        ),
    }
    if process:
        publication.update(
            {
                "common:referenceToOwnershipOfDataSet": _owner_ref(),
                "common:copyright": "false",
                "common:licenseType": "Free of charge for all users and uses",
            }
        )
    else:
        publication["common:referenceToOwnershipOfDataSet"] = _owner_ref()

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
        name=PLACEHOLDER_IMPORT_ACTOR,
        short_name=PLACEHOLDER_IMPORT_ACTOR,
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
) -> dict[str, Any]:
    dataset_info = {
        "common:UUID": contact_id,
        "common:shortName": _ml(str(short_name)),
        "common:name": _ml(str(name)),
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
        "common:shortName": _ml(str(short_name)),
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
        PLACEHOLDER_COMPLIANCE_NOT_DEFINED,
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

    return {
        "unitGroupDataSet": {
            "@xmlns": "http://lca.jrc.it/ILCD/UnitGroup",
            "@xmlns:common": "http://lca.jrc.it/ILCD/Common",
            "@xmlns:xsi": "http://www.w3.org/2001/XMLSchema-instance",
            "@version": "1.1",
            "@xsi:schemaLocation": "http://lca.jrc.it/ILCD/UnitGroup ../../schemas/ILCD_UnitGroupDataSet.xsd",
            "unitGroupInformation": {
                "dataSetInformation": {
                    "common:UUID": entity.internal_id,
                    "common:name": _ml(entity.name or "Unit group"),
                    "classificationInformation": {
                        "common:classification": {
                            "common:class": {
                                "@level": "0",
                                "@classId": "4",
                                "#text": "Other unit groups",
                            }
                        }
                    },
                },
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
    return {
        "flowPropertyDataSet": {
            "@xmlns": "http://lca.jrc.it/ILCD/FlowProperty",
            "@xmlns:common": "http://lca.jrc.it/ILCD/Common",
            "@xmlns:xsi": "http://www.w3.org/2001/XMLSchema-instance",
            "@version": "1.1",
            "@xsi:schemaLocation": "http://lca.jrc.it/ILCD/FlowProperty ../../schemas/ILCD_FlowPropertyDataSet.xsd",
            "flowPropertiesInformation": {
                "dataSetInformation": {
                    "common:UUID": entity.internal_id,
                    "common:name": _ml(entity.name or "Flow property"),
                    "classificationInformation": {
                        "common:classification": {
                            "common:class": {
                                "@level": "0",
                                "@classId": "4",
                                "#text": "Other flow properties",
                            }
                        }
                    },
                },
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
    classification = _flow_classification(flow_type)
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
    exchange_items = [
        _exchange_item(exchange, idx) for idx, exchange in enumerate(exchanges, 1)
    ]
    reference_flow = _reference_flow_id(exchange_items)
    reference_year = _reference_year(entity.raw.get("referenceYear"))
    valid_until = _optional_year(entity.raw.get("dataSetValidUntil"))
    location = _location_code(entity.raw.get("location"))
    data_set_information = {
        "common:UUID": entity.internal_id,
        "name": _name_parts(entity.name or "Process"),
        "classificationInformation": _default_process_classification(),
        "common:generalComment": _ml(
            entity.raw.get("description") or PLACEHOLDER_CONVERTED_APPLICATION
        ),
    }
    common_other = _common_other_trace(entity.raw.get("sourceTrace"))
    if common_other:
        data_set_information["common:other"] = common_other

    time = {"common:referenceYear": reference_year}
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
        },
        "time": time,
        "geography": {
            "locationOfOperationSupplyOrProduction": location_item,
        },
    }
    technology = _technology_section(entity)
    if technology:
        process_information["technology"] = technology

    modelling_and_validation = {
        "LCIMethodAndAllocation": {"typeOfDataSet": "Unit process, single operation"}
    }
    data_sources = _data_sources_treatment_and_representativeness(entity)
    if data_sources:
        modelling_and_validation["dataSourcesTreatmentAndRepresentativeness"] = (
            data_sources
        )
    modelling_and_validation["validation"] = {"review": {"@type": "Not reviewed"}}
    modelling_and_validation["complianceDeclarations"] = _compliance_declarations(
        process=True
    )
    if not exchange_items:
        exchange_items = [
            {
                "@dataSetInternalID": "1",
                "referenceToFlowDataSet": _ref(
                    ref_type="flow data set",
                    ref_id=DEFAULT_FLOW_PROPERTY_ID,
                    short_description="Unspecified flow",
                    category="flows",
                ),
                "exchangeDirection": "Output",
                "meanAmount": "1",
                "resultingAmount": "1",
                "dataDerivationTypeStatus": "Unknown derivation",
            }
        ]
        reference_flow = "1"

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
            "administrativeInformation": {
                "common:commissionerAndGoal": {
                    "common:referenceToCommissioner": _owner_ref(),
                    "common:intendedApplications": _ml(
                        PLACEHOLDER_CONVERTED_APPLICATION
                    ),
                },
                **_admin_info(
                    category="processes",
                    dataset_id=entity.internal_id,
                    process=True,
                ),
            },
            "exchanges": {"exchange": exchange_items},
        }
    }


def _default_process_classification() -> dict[str, Any]:
    return {
        "common:classification": {
            "common:class": [
                {
                    "@level": "0",
                    "@classId": "T",
                    "#text": "Other service activities",
                },
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
        }
    }


def _technology_section(entity: CanonicalEntity) -> dict[str, Any] | None:
    description = entity.raw.get("technologyDescription")
    applicability = entity.raw.get("technologicalApplicability")
    if not _text(description) and not _text(applicability):
        return None
    section = {
        "technologyDescriptionAndIncludedProcesses": _ml(
            str(description).strip()
            if _text(description)
            else PLACEHOLDER_UNSPECIFIED_TEXT
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
    }
    if not source_refs and not any(_text(raw.get(key)) for key in relevant_keys):
        return None

    source_ref = source_refs[0] if source_refs else {}
    source_id = source_ref.get("id") or FORMAT_SOURCE_ID
    source_name = (
        source_ref.get("name") or raw.get("sourceLabel") or PLACEHOLDER_DATA_SOURCE
    )
    section: dict[str, Any] = {
        "dataCutOffAndCompletenessPrinciples": _ml(
            raw.get("dataCutOffAndCompletenessPrinciples") or PLACEHOLDER_DATA_CUTOFF
        ),
        "referenceToDataSource": _ref(
            ref_type="source data set",
            ref_id=source_id,
            short_description=str(source_name),
            category="sources",
        ),
        "annualSupplyOrProductionVolume": _annual_supply_or_production_volume(
            raw.get("productionVolume")
        ),
    }
    if _text(raw.get("samplingProcedure")):
        section["samplingProcedure"] = _ml(str(raw["samplingProcedure"]).strip())
    if _text(raw.get("dataCollectionPeriod")):
        section["dataCollectionPeriod"] = _ml(str(raw["dataCollectionPeriod"]).strip())
    if _text(raw.get("uncertaintyAdjustments")):
        section["uncertaintyAdjustments"] = _ml(
            str(raw["uncertaintyAdjustments"]).strip()
        )
    if _text(raw.get("useAdviceForDataSet")):
        section["useAdviceForDataSet"] = _ml(str(raw["useAdviceForDataSet"]).strip())

    common_other = _common_other_trace(
        {
            "format": raw.get("sourceFormat"),
            "sourceObject": raw.get("sourceLabel"),
            "sourceCategory": raw.get("sourceCategory"),
            "sourceSubCategory": raw.get("sourceSubCategory"),
            "sourceLocalCategory": raw.get("sourceLocalCategory"),
            "sourceLocalSubCategory": raw.get("sourceLocalSubCategory"),
        }
    )
    if common_other:
        section["common:other"] = common_other
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
    if _text(exchange.get("generalComment")):
        comments.append(str(exchange["generalComment"]).strip())
    if comments:
        item["generalComment"] = _ml(" ".join(comments))

    common_other = _common_other_trace(
        _exchange_trace_payload(exchange),
    )
    if common_other:
        item["common:other"] = common_other
    return item


def _reference_flow_id(exchange_items: list[dict[str, Any]]) -> str:
    for exchange in exchange_items:
        if exchange.get("exchangeDirection") == "Output":
            return str(exchange.get("@dataSetInternalID") or "1")
    if exchange_items:
        return str(exchange_items[0].get("@dataSetInternalID") or "1")
    return "1"


def _exchange_trace_payload(exchange: dict[str, Any]) -> dict[str, Any] | None:
    source_trace = exchange.get("sourceTrace")
    extra = {
        "activityLinkId": exchange.get("activityLinkId"),
        "productionVolumeAmount": exchange.get("productionVolumeAmount"),
        "isCalculatedAmount": exchange.get("isCalculatedAmount"),
    }
    if not source_trace and not any(value is not None for value in extra.values()):
        return None
    return {
        "sourceTrace": source_trace,
        "exchangeMetadata": {
            key: value for key, value in extra.items() if value is not None
        },
    }


def _name_parts(name: str) -> dict[str, Any]:
    return {
        "baseName": _ml(name),
        "treatmentStandardsRoutes": _ml(""),
        "mixAndLocationTypes": _ml(""),
    }


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


def _reference_year(value: Any) -> int:
    if value is None:
        return 2026
    text = str(value).strip()
    match = re.search(r"\b(\d{4})\b", text)
    if not match:
        return 2026
    return int(match.group(1))


def _optional_year(value: Any) -> int | None:
    if value is None:
        return None
    text = str(value).strip()
    match = re.search(r"\b(\d{4})\b", text)
    if not match:
        return None
    return int(match.group(1))


def _cas_number(value: Any) -> str | None:
    if value is None:
        return None
    text = str(value).strip()
    return text if re.fullmatch(r"[0-9]{2,7}-[0-9]{2}-[0-9]", text) else None


def _location_code(value: Any) -> str:
    if value is None:
        return "GLO"
    text = str(value).strip()
    if not text:
        return "GLO"
    if text.casefold() == "row":
        return "GLO"
    if text in _location_codes():
        return text
    return "GLO"


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


def _flow_classification(flow_type: str) -> dict[str, Any]:
    if flow_type == "Elementary flow":
        return {
            "common:elementaryFlowCategorization": {
                "common:category": [
                    {"@level": "0", "@catId": "1", "#text": "Emissions"},
                    {"@level": "1", "@catId": "1.3", "#text": "Emissions to air"},
                    {
                        "@level": "2",
                        "@catId": "1.3.4",
                        "#text": "Emissions to air, unspecified",
                    },
                ]
            }
        }
    return {
        "common:classification": {
            "common:class": [
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
        }
    }


def _real(value: Any) -> str:
    try:
        return f"{float(value):g}"
    except (TypeError, ValueError):
        return "0"


def _optional_real(value: Any) -> str | None:
    if value is None:
        return None
    try:
        return f"{float(value):g}"
    except (TypeError, ValueError):
        return None


def _percentage(value: Any) -> str | None:
    if value is None:
        return None
    try:
        numeric = float(value)
    except (TypeError, ValueError):
        return None
    if numeric < 0 or numeric > 100:
        return None
    rounded = round(numeric, 3)
    return f"{rounded:.3f}".rstrip("0").rstrip(".")


def _annual_supply_or_production_volume(value: Any) -> dict[str, str]:
    text = str(value).strip() if value is not None else ""
    if not re.fullmatch(r"[+-]?(\d+(\.\d*)?|\.\d+)([Ee][+-]?\d+)?\s+\S.*", text):
        text = PLACEHOLDER_ANNUAL_VOLUME
    return _ml(text[:500])


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


def _common_other_trace(payload: Any) -> dict[str, Any] | None:
    cleaned = _clean_trace_value(payload)
    if cleaned in (None, {}, []):
        return None
    return {
        "@xmlns:tidasimport": IMPORT_TRACE_NAMESPACE,
        "tidasimport:sourceTrace": {
            "@marker": IMPORT_TRACE_MARKER,
            "payload": cleaned,
        },
    }


def _clean_trace_value(value: Any) -> Any:
    if value is None:
        return None
    if isinstance(value, (str, int, float, bool)):
        return value
    if isinstance(value, list):
        cleaned_items = [
            cleaned
            for item in value
            if (cleaned := _clean_trace_value(item)) not in (None, {}, [])
        ]
        return cleaned_items
    if isinstance(value, dict):
        cleaned = {}
        for key, item in value.items():
            cleaned_item = _clean_trace_value(item)
            if cleaned_item in (None, {}, []):
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
