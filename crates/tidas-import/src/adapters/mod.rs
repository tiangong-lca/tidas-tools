mod ecospold;
mod generated_units;
mod ilcd;
mod openlca_jsonld;
mod openlca_xlsx;
mod simapro_csv;
mod xml_node;

use std::path::Path;

use thiserror::Error;
use tidas_runtime::{CancellationToken, MemoryBudget};

use crate::detect::SourceFormat;
use crate::report::{IssueSink, IssueSinkError};
use crate::source::SourceReadError;
use crate::store::{CanonicalStore, StoreError};
use tidas_runtime::MemoryReservation;

pub use ilcd::IlcdAdapter;
pub use openlca_jsonld::OpenLcaJsonLdAdapter;
pub use openlca_xlsx::OpenLcaProcessXlsxAdapter;
pub use simapro_csv::SimaProCsvAdapter;

pub struct AdapterContext<'a> {
    pub source: &'a Path,
    pub cancellation: &'a CancellationToken,
    pub memory_budget: &'a MemoryBudget,
    pub max_entry_bytes: u64,
}

impl AdapterContext<'_> {
    pub(crate) fn reserve_structured_expansion(
        &self,
        input_bytes: usize,
        expansion_factor: u64,
    ) -> Result<MemoryReservation, AdapterError> {
        let input_bytes =
            u64::try_from(input_bytes).map_err(|_| tidas_runtime::RuntimeError::SizeOverflow)?;
        let expanded = input_bytes
            .checked_mul(expansion_factor)
            .ok_or(tidas_runtime::RuntimeError::SizeOverflow)?;
        Ok(self.memory_budget.reserve(expanded)?)
    }
}

pub trait SourceAdapter: Sync {
    fn format(&self) -> SourceFormat;

    fn read(
        &self,
        context: &AdapterContext<'_>,
        store: &mut CanonicalStore,
        issues: &mut dyn IssueSink,
    ) -> Result<(), AdapterError>;
}

static OPENLCA_JSONLD_ADAPTER: OpenLcaJsonLdAdapter = OpenLcaJsonLdAdapter;
static SIMAPRO_CSV_ADAPTER: SimaProCsvAdapter = SimaProCsvAdapter;
static ILCD_ADAPTER: IlcdAdapter = IlcdAdapter;
static OPENLCA_XLSX_ADAPTER: OpenLcaProcessXlsxAdapter = OpenLcaProcessXlsxAdapter;

#[must_use]
pub fn adapter_for(format: SourceFormat) -> Option<&'static dyn SourceAdapter> {
    match format {
        SourceFormat::Ecospold1 => Some(&ECOSPOLD1_ADAPTER),
        SourceFormat::Ecospold2 => Some(&ECOSPOLD2_ADAPTER),
        SourceFormat::Ilcd => Some(&ILCD_ADAPTER),
        SourceFormat::OpenlcaJsonld => Some(&OPENLCA_JSONLD_ADAPTER),
        SourceFormat::OpenlcaProcessXlsx => Some(&OPENLCA_XLSX_ADAPTER),
        SourceFormat::SimaproCsv => Some(&SIMAPRO_CSV_ADAPTER),
        _ => None,
    }
}

#[derive(Debug, Error)]
pub enum AdapterError {
    #[error("source adapter input failed: {0}")]
    Source(#[from] SourceReadError),
    #[error("source adapter canonical store failed: {0}")]
    Store(#[from] StoreError),
    #[error("source adapter issue stream failed: {0}")]
    Issue(#[from] IssueSinkError),
    #[error("source adapter JSON failed: {0}")]
    Json(#[from] serde_json::Error),
    #[error("source adapter ZIP failed: {0}")]
    Zip(#[from] zip::result::ZipError),
    #[error("source adapter XML failed: {0}")]
    Xml(#[from] quick_xml::Error),
    #[error("source adapter XML attribute failed: {0}")]
    XmlAttribute(#[from] quick_xml::events::attributes::AttrError),
    #[error("source adapter XML decoding failed: {0}")]
    XmlEncoding(#[from] quick_xml::encoding::EncodingError),
    #[error("source adapter I/O failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("source adapter runtime failed: {0}")]
    Runtime(#[from] tidas_runtime::RuntimeError),
}
pub use ecospold::{EcoSpold1Adapter, EcoSpold2Adapter};
static ECOSPOLD1_ADAPTER: EcoSpold1Adapter = EcoSpold1Adapter;
static ECOSPOLD2_ADAPTER: EcoSpold2Adapter = EcoSpold2Adapter;
