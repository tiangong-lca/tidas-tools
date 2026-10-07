//! Exact evidence from an explicit package/manifest boundary; never a filesystem resolver.
use crate::{ExactFlowEvidence, ValidationError, ValidationRequest};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fmt::Write,
    fs,
    io::Read,
    path::{Path, PathBuf},
};
use tidas_runtime::MemoryReservation;

#[derive(Default)]
pub(crate) struct FlowEvidenceIndex {
    entries: BTreeMap<String, Vec<ExactFlowEvidence>>,
    sources: BTreeMap<PathBuf, String>,
    reservations: Vec<MemoryReservation>,
}
impl FlowEvidenceIndex {
    pub(crate) fn add(
        &mut self,
        path: &Path,
        request: &ValidationRequest,
    ) -> Result<(), ValidationError> {
        request.cancellation.check()?;
        let size = fs::metadata(path)?.len();
        let estimate = size
            .checked_mul(8)
            .and_then(|v| v.checked_add(4096))
            .ok_or(ValidationError::SizeOverflow)?;
        let _document = request.memory_budget.reserve(estimate)?;
        let bytes = fs::read(path)?;
        let hash = digest_hex(&Sha256::digest(&bytes));
        let path_cost =
            u64::try_from(path.as_os_str().len()).map_err(|_| ValidationError::SizeOverflow)?;
        self.reservations
            .push(request.memory_budget.reserve(path_cost + 256)?);
        self.sources.insert(path.to_owned(), hash.clone());
        let Ok(json) = serde_json::from_slice::<Value>(&bytes) else {
            return Ok(());
        };
        let Some(uuid) = json
            .pointer("/flowDataSet/flowInformation/dataSetInformation/common:UUID")
            .and_then(Value::as_str)
        else {
            return Ok(());
        };
        let Some(version) = json.pointer("/flowDataSet/administrativeInformation/publicationAndOwnership/common:dataSetVersion").and_then(Value::as_str) else { return Ok(()); };
        let Some(kind) = json
            .pointer("/flowDataSet/modellingAndValidation/LCIMethod/typeOfDataSet")
            .and_then(Value::as_str)
        else {
            return Ok(());
        };
        let evidence = ExactFlowEvidence {
            uuid: uuid.to_owned(),
            version: version.to_owned(),
            r#type: kind.to_owned(),
            content_hash: Some(hash),
        };
        let cost = u64::try_from(512 + 2 * (uuid.len() + version.len()) + kind.len())
            .map_err(|_| ValidationError::SizeOverflow)?;
        self.reservations.push(request.memory_budget.reserve(cost)?);
        self.entries
            .entry(uuid.to_owned())
            .or_default()
            .push(evidence);
        Ok(())
    }
    pub(crate) fn verify(&self, request: &ValidationRequest) -> Result<(), ValidationError> {
        let _buffer_memory = request.memory_budget.reserve(8192)?;
        let mut buffer = [0_u8; 8192];
        for (path, expected) in &self.sources {
            request.cancellation.check()?;
            let mut file = fs::File::open(path)?;
            let mut hash = Sha256::new();
            loop {
                request.cancellation.check()?;
                let count = file.read(&mut buffer)?;
                if count == 0 {
                    break;
                }
                hash.update(&buffer[..count]);
            }
            if digest_hex(&hash.finalize()) != *expected {
                return Err(ValidationError::FlowEvidenceDrift(path.clone()));
            }
        }
        Ok(())
    }
    pub(crate) fn for_process(
        &self,
        process: &Value,
        request: &ValidationRequest,
    ) -> Result<(Vec<ExactFlowEvidence>, MemoryReservation), ValidationError> {
        let exchanges = &process["processDataSet"]["exchanges"]["exchange"];
        let rows: Vec<&Value> = match exchanges {
            Value::Array(rows) => rows.iter().collect(),
            Value::Object(_) => vec![exchanges],
            _ => vec![],
        };
        let mut identities = std::collections::BTreeSet::new();
        for row in rows {
            request.cancellation.check()?;
            if let Some(uuid) = row["referenceToFlowDataSet"]["@refObjectId"].as_str() {
                identities.insert(uuid);
            }
        }
        let mut cost = 0_u64;
        for evidence in identities
            .iter()
            .filter_map(|uuid| self.entries.get(*uuid))
            .flatten()
        {
            cost = cost
                .checked_add(
                    u64::try_from(
                        256 + evidence.uuid.len() + evidence.version.len() + evidence.r#type.len(),
                    )
                    .map_err(|_| ValidationError::SizeOverflow)?,
                )
                .ok_or(ValidationError::SizeOverflow)?;
        }
        let reservation = request.memory_budget.reserve(cost)?;
        let flows = identities
            .into_iter()
            .filter_map(|uuid| self.entries.get(uuid))
            .flatten()
            .cloned()
            .collect();
        Ok((flows, reservation))
    }
}

fn digest_hex(digest: &[u8]) -> String {
    let mut output = String::with_capacity(digest.len() * 2);
    for byte in digest {
        write!(&mut output, "{byte:02x}").expect("writing to a String cannot fail");
    }
    output
}
