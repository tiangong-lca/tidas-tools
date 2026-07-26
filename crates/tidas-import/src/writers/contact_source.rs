use serde_json::{Value, json};

use crate::model::CanonicalEntity;

use super::common::{COMPLIANCE_SOURCE_NAME, compliance_source_id};
use super::common::{
    CONTACT_NAME, FORMAT_SOURCE_NAME, administrative, contact_id, format_source_id, localized,
};

pub fn contact() -> (String, Value) {
    let id = contact_id();
    let value = json!({
        "contactDataSet": {
            "@xmlns:common": "http://lca.jrc.it/ILCD/Common",
            "@xmlns": "http://lca.jrc.it/ILCD/Contact",
            "@xmlns:xsi": "http://www.w3.org/2001/XMLSchema-instance",
            "@version": "1.1",
            "@xsi:schemaLocation": "http://lca.jrc.it/ILCD/Contact ../../schemas/ILCD_ContactDataSet.xsd",
            "contactInformation": {
                "dataSetInformation": {
                    "common:UUID": id,
                    "common:shortName": localized(CONTACT_NAME),
                    "common:name": localized(CONTACT_NAME),
                    "classificationInformation": {
                        "common:classification": {
                            "common:class": {
                                "@level": "0",
                                "@classId": "5",
                                "#text": "Other"
                            }
                        }
                    }
                }
            },
            "administrativeInformation": administrative("contacts", &id, false)
        }
    });
    (id, value)
}

pub fn sources() -> [(String, Value); 2] {
    [
        source(
            format_source_id(),
            FORMAT_SOURCE_NAME,
            "1",
            "Data set formats",
        ),
        source(
            compliance_source_id(),
            COMPLIANCE_SOURCE_NAME,
            "3",
            "Compliance systems",
        ),
    ]
}

fn source(id: String, name: &str, class_id: &str, class_name: &str) -> (String, Value) {
    let value = json!({
        "sourceDataSet": {
            "@xmlns:common": "http://lca.jrc.it/ILCD/Common",
            "@xmlns": "http://lca.jrc.it/ILCD/Source",
            "@xmlns:xsi": "http://www.w3.org/2001/XMLSchema-instance",
            "@version": "1.1",
            "@xsi:schemaLocation": "http://lca.jrc.it/ILCD/Source ../../schemas/ILCD_SourceDataSet.xsd",
            "sourceInformation": {
                "dataSetInformation": {
                    "common:UUID": id,
                    "common:shortName": localized(name),
                    "classificationInformation": {
                        "common:classification": {
                            "common:class": {
                                "@level": "0",
                                "@classId": class_id,
                                "#text": class_name
                            }
                        }
                    }
                }
            },
            "administrativeInformation": administrative("sources", &id, false)
        }
    });
    (id, value)
}

pub fn canonical_contact(entity: &CanonicalEntity) -> Value {
    let name = entity.name.as_deref().unwrap_or("Imported contact");
    json!({
        "contactDataSet": {
            "@xmlns:common": "http://lca.jrc.it/ILCD/Common",
            "@xmlns": "http://lca.jrc.it/ILCD/Contact",
            "@xmlns:xsi": "http://www.w3.org/2001/XMLSchema-instance",
            "@version": "1.1",
            "@xsi:schemaLocation": "http://lca.jrc.it/ILCD/Contact ../../schemas/ILCD_ContactDataSet.xsd",
            "contactInformation": {
                "dataSetInformation": {
                    "common:UUID": entity.internal_id,
                    "common:shortName": localized(name),
                    "common:name": localized(name),
                    "classificationInformation": {
                        "common:classification": {
                            "common:class": {
                                "@level": "0",
                                "@classId": "5",
                                "#text": "Other"
                            }
                        }
                    }
                }
            },
            "administrativeInformation": administrative(
                "contacts",
                &entity.internal_id,
                false
            )
        }
    })
}

pub fn canonical_source(entity: &CanonicalEntity) -> Value {
    let name = entity.name.as_deref().unwrap_or("Imported source");
    let mut information = serde_json::Map::from_iter([
        (
            "common:UUID".to_owned(),
            Value::String(entity.internal_id.clone()),
        ),
        ("common:shortName".to_owned(), localized(name)),
        (
            "classificationInformation".to_owned(),
            json!({
                "common:classification": {
                    "common:class": {
                        "@level": "0",
                        "@classId": "6",
                        "#text": "Other"
                    }
                }
            }),
        ),
    ]);
    if let Some(citation) = entity.raw.get("textReference").and_then(Value::as_str) {
        information.insert(
            "sourceCitation".to_owned(),
            Value::String(citation.to_owned()),
        );
    }
    json!({
        "sourceDataSet": {
            "@xmlns:common": "http://lca.jrc.it/ILCD/Common",
            "@xmlns": "http://lca.jrc.it/ILCD/Source",
            "@xmlns:xsi": "http://www.w3.org/2001/XMLSchema-instance",
            "@version": "1.1",
            "@xsi:schemaLocation": "http://lca.jrc.it/ILCD/Source ../../schemas/ILCD_SourceDataSet.xsd",
            "sourceInformation": {"dataSetInformation": information},
            "administrativeInformation": administrative(
                "sources",
                &entity.internal_id,
                false
            )
        }
    })
}
