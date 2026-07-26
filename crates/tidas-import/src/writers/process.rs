use serde_json::{Map, Value, json};
use thiserror::Error;

use crate::model::CanonicalEntity;

use super::common::{
    CONTACT_NAME, administrative, compliance_declarations, contact_id, dataset_ref,
    format_source_id, localized, name_parts,
};
use super::unit_flow::exchange_metadata;

pub fn process_base(
    entity: &CanonicalEntity,
    reference: &str,
    functional_unit: &str,
) -> Result<Value, ProcessWriteError> {
    let name = entity.name.as_deref().unwrap_or("Process");
    let description = entity
        .raw
        .get("description")
        .and_then(Value::as_str)
        .unwrap_or("Converted from external LCA package");
    let format_id = format_source_id();
    let contact_id = contact_id();
    let location = entity
        .raw
        .get("location")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .unwrap_or("GLO");
    let reference_year = entity
        .raw
        .get("referenceYear")
        .and_then(Value::as_u64)
        .unwrap_or(9999);
    json_process(
        entity,
        &ProcessParts {
            name,
            description,
            reference,
            functional_unit,
            format_id: &format_id,
            contact_id: &contact_id,
            location,
            reference_year,
        },
    )
}

struct ProcessParts<'a> {
    name: &'a str,
    description: &'a str,
    reference: &'a str,
    functional_unit: &'a str,
    format_id: &'a str,
    contact_id: &'a str,
    location: &'a str,
    reference_year: u64,
}

fn json_process(
    entity: &CanonicalEntity,
    parts: &ProcessParts<'_>,
) -> Result<Value, ProcessWriteError> {
    Ok(json!({
        "processDataSet": {
            "@xmlns": "http://lca.jrc.it/ILCD/Process",
            "@xmlns:common": "http://lca.jrc.it/ILCD/Common",
            "@xmlns:xsi": "http://www.w3.org/2001/XMLSchema-instance",
            "@version": "1.1",
            "@locations": "../ILCDLocations.xml",
            "@xsi:schemaLocation": "http://lca.jrc.it/ILCD/Process ../../schemas/ILCD_ProcessDataSet.xsd",
            "processInformation": {
                "dataSetInformation": {
                    "common:UUID": entity.internal_id,
                    "name": name_parts(parts.name, parts.location),
                    "classificationInformation": {
                        "common:classification": {
                            "@name": "ISIC rev.4",
                            "common:class": [
                                {"@level": "0", "@classId": "T", "#text": "Other service activities"},
                                {"@level": "1", "@classId": "94", "#text": "Activities of membership organizations"},
                                {"@level": "2", "@classId": "949", "#text": "Activities of other membership organizations"},
                                {"@level": "3", "@classId": "9499", "#text": "Activities of other membership organizations n.e.c."}
                            ]
                        }
                    },
                    "common:generalComment": localized(parts.description)
                },
                "quantitativeReference": {
                    "@type": "Reference flow(s)",
                    "referenceToReferenceFlow": parts.reference,
                    "functionalUnitOrOther": localized(parts.functional_unit)
                },
                "time": {"common:referenceYear": parts.reference_year},
                "geography": {
                    "locationOfOperationSupplyOrProduction": {
                        "@location": parts.location
                    }
                }
            },
            "modellingAndValidation": {
                "LCIMethodAndAllocation": {
                    "typeOfDataSet": "Unit process, single operation"
                },
                "dataSourcesTreatmentAndRepresentativeness": {
                    "dataCutOffAndCompletenessPrinciples": localized(
                        "Converted from external LCA package"
                    ),
                    "referenceToDataSource": dataset_ref(
                        "source data set",
                        parts.format_id,
                        "External LCA source metadata",
                        "sources"
                    ),
                    "annualSupplyOrProductionVolume": localized(
                        "0 kg/year; source production volume unavailable"
                    )
                },
                "validation": {"review": {"@type": "Not reviewed"}},
                "complianceDeclarations": compliance_declarations(true)
            },
            "administrativeInformation": {
                "common:commissionerAndGoal": {
                    "common:referenceToCommissioner": dataset_ref(
                        "contact data set",
                        parts.contact_id,
                        CONTACT_NAME,
                        "contacts"
                    ),
                    "common:intendedApplications": localized(
                        "Converted from external LCA package"
                    )
                },
                "dataEntryBy": administrative("processes", &entity.internal_id, true)
                    .get("dataEntryBy")
                    .cloned()
                    .ok_or(ProcessWriteError::AdministrativeShape)?,
                "publicationAndOwnership": administrative(
                    "processes",
                    &entity.internal_id,
                    true
                )
                .get("publicationAndOwnership")
                .cloned()
                .ok_or(ProcessWriteError::AdministrativeShape)?
            }
        }
    }))
}

pub fn exchange_item(
    exchange: &Map<String, Value>,
    internal_id: &str,
) -> Result<Value, ProcessWriteError> {
    let flow_id = exchange
        .get("flowRefId")
        .and_then(Value::as_str)
        .ok_or_else(|| ProcessWriteError::MissingFlowReference(internal_id.to_owned()))?;
    let flow_name = exchange
        .get("flowName")
        .and_then(Value::as_str)
        .unwrap_or("Flow");
    let amount = exchange
        .get("amount")
        .and_then(Value::as_str)
        .unwrap_or("0");
    let direction = if exchange
        .get("isInput")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        "Input"
    } else {
        "Output"
    };
    Ok(json!({
        "@dataSetInternalID": internal_id,
        "referenceToFlowDataSet": dataset_ref(
            "flow data set",
            flow_id,
            flow_name,
            "flows"
        ),
        "exchangeDirection": direction,
        "meanAmount": amount,
        "resultingAmount": amount,
        "dataDerivationTypeStatus": "Unknown derivation",
        "generalComment": localized(format!(
            "Source exchange internal id: {internal_id}."
        )),
        "common:other": exchange_metadata(exchange)
    }))
}

pub fn functional_unit(exchange: &Map<String, Value>) -> String {
    let amount = exchange
        .get("amount")
        .and_then(Value::as_str)
        .unwrap_or("0");
    let unit = exchange
        .get("unitName")
        .and_then(Value::as_str)
        .unwrap_or("unit");
    let flow = exchange
        .get("flowName")
        .and_then(Value::as_str)
        .unwrap_or("flow");
    format!("{amount} {unit} {flow}")
}

#[derive(Debug, Error)]
pub enum ProcessWriteError {
    #[error("exchange {0} has no flow reference")]
    MissingFlowReference(String),
    #[error("internal administrative dataset shape is invalid")]
    AdministrativeShape,
}
