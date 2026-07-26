use std::path::{Path, PathBuf};

use futures_util::{StreamExt, pin_mut};
use serde_json::Value;
use tidas_runtime::MemoryReservation;
use tokio::sync::mpsc;
use tokio_postgres::types::ToSql;
use tokio_postgres::{Client, Row};
use tokio_postgres_rustls::MakeRustlsConnect;

use crate::{ExportError, ExportRequest, write_record};

const CATEGORIES: &[&str] = &[
    "contacts",
    "flows",
    "flowproperties",
    "processes",
    "sources",
    "unitgroups",
    "lciamethods",
    "lifecyclemodels",
];
const RECORD_MEMORY_OVERHEAD: u64 = 1024;

#[derive(Clone, Debug, Default)]
pub struct DatabaseCounts {
    pub common: u64,
    pub category: u64,
    pub warnings: Vec<String>,
}

struct DatabaseRecord {
    relative_stem: PathBuf,
    json: Vec<u8>,
    common: bool,
    _reservation: MemoryReservation,
}

pub async fn export_records(
    request: &ExportRequest,
    package_dir: &Path,
) -> Result<DatabaseCounts, ExportError> {
    let (tls, _certificate_warnings) =
        MakeRustlsConnect::with_native_certs().map_err(|_| ExportError::DatabaseTlsRoots)?;
    let (client, connection) = tokio_postgres::connect(request.database_url.expose(), tls)
        .await
        .map_err(ExportError::DatabaseConnect)?;
    let connection_task = tokio::spawn(connection);
    let (sender, mut receiver) = mpsc::channel(request.queue_capacity);
    let producer_request = request.clone();
    let producer =
        tokio::spawn(async move { produce_snapshot(client, sender, &producer_request).await });

    let mut counts = DatabaseCounts::default();
    while let Some(record) = receiver.recv().await {
        request.cancellation.check()?;
        write_record(
            package_dir,
            &record.relative_stem,
            &record.json,
            request.format,
            &request.cancellation,
        )?;
        if record.common {
            counts.common += 1;
        } else {
            counts.category += 1;
        }
    }
    counts.warnings = producer.await??;
    connection_task.await?.map_err(ExportError::Database)?;
    Ok(counts)
}

async fn produce_snapshot(
    client: Client,
    sender: mpsc::Sender<DatabaseRecord>,
    request: &ExportRequest,
) -> Result<Vec<String>, ExportError> {
    let mut warnings = Vec::new();
    client
        .batch_execute("BEGIN TRANSACTION ISOLATION LEVEL REPEATABLE READ READ ONLY")
        .await
        .map_err(ExportError::Database)?;
    stream_query(
        &client,
        "SELECT file_name::text, json_ordered FROM ilcd ORDER BY file_name::text",
        true,
        None,
        &sender,
        request,
    )
    .await?;
    for category in CATEGORIES {
        request.cancellation.check()?;
        client
            .batch_execute("SAVEPOINT tidas_export_category")
            .await
            .map_err(ExportError::Database)?;
        let query = format!(
            "SELECT id::text, json_ordered, version::text FROM {category} \
             WHERE state_code = 100 ORDER BY id::text, version::text"
        );
        match stream_query(&client, &query, false, Some(category), &sender, request).await {
            Ok(()) => {
                client
                    .batch_execute("RELEASE SAVEPOINT tidas_export_category")
                    .await
                    .map_err(ExportError::Database)?;
            }
            Err(ExportError::Database(_)) => {
                client
                    .batch_execute(
                        "ROLLBACK TO SAVEPOINT tidas_export_category; \
                         RELEASE SAVEPOINT tidas_export_category",
                    )
                    .await
                    .map_err(ExportError::Database)?;
                warnings.push(format!(
                    "Database category '{category}' was skipped because its query failed."
                ));
            }
            Err(error) => return Err(error),
        }
    }
    client
        .batch_execute("COMMIT")
        .await
        .map_err(ExportError::Database)?;
    Ok(warnings)
}

async fn stream_query(
    client: &Client,
    query: &str,
    common: bool,
    category: Option<&str>,
    sender: &mpsc::Sender<DatabaseRecord>,
    request: &ExportRequest,
) -> Result<(), ExportError> {
    let parameters = std::iter::empty::<&(dyn ToSql + Sync)>();
    let rows = client
        .query_raw(query, parameters)
        .await
        .map_err(ExportError::Database)?;
    pin_mut!(rows);
    while let Some(row) = rows.next().await {
        request.cancellation.check()?;
        let row = row.map_err(ExportError::Database)?;
        let record = decode_row(&row, common, category, request)?;
        sender
            .send(record)
            .await
            .map_err(|_| ExportError::Runtime(tidas_runtime::RuntimeError::QueueDisconnected))?;
    }
    Ok(())
}

fn decode_row(
    row: &Row,
    common: bool,
    category: Option<&str>,
    request: &ExportRequest,
) -> Result<DatabaseRecord, ExportError> {
    let id: String = row.try_get(0).map_err(ExportError::Database)?;
    let payload: Value = row.try_get(1).map_err(ExportError::Database)?;
    let relative_stem = if common {
        PathBuf::from(id)
    } else {
        let version: String = row.try_get(2).map_err(ExportError::Database)?;
        PathBuf::from(category.expect("category records always have a category"))
            .join(format!("{id}_{version}"))
    };
    let mut json = serde_json::to_vec_pretty(&payload)?;
    json.push(b'\n');
    let estimated_bytes = u64::try_from(json.len())
        .map_err(|_| tidas_runtime::RuntimeError::SizeOverflow)?
        .saturating_add(
            u64::try_from(relative_stem.as_os_str().len())
                .map_err(|_| tidas_runtime::RuntimeError::SizeOverflow)?,
        )
        .saturating_add(RECORD_MEMORY_OVERHEAD);
    let reservation = request.memory_budget.reserve(estimated_bytes)?;
    Ok(DatabaseRecord {
        relative_stem,
        json,
        common,
        _reservation: reservation,
    })
}
