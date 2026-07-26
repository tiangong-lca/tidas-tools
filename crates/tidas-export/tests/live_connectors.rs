use std::env;
use std::fs;

use tidas_export::{ExportFormat, ExportRequest, S3Config, SecretString, run_export};
use tidas_runtime::{CancellationToken, MemoryBudget};

#[test]
fn disposable_postgres_and_s3_fixture_is_deterministic_when_configured() {
    let Some(database_url) = env::var("TIDAS_EXPORT_TEST_DATABASE_URL")
        .ok()
        .filter(|value| !value.is_empty())
    else {
        return;
    };
    let temporary = tempfile::tempdir().unwrap();
    let make_request = |name: &str| {
        let mut request = ExportRequest::new(
            SecretString::new(database_url.clone()),
            temporary.path().join(name),
            ExportFormat::Tidas,
            CancellationToken::default(),
            MemoryBudget::new(32 * 1024 * 1024),
            2,
        );
        request.external_documents = env::var("TIDAS_EXPORT_TEST_S3_BUCKET")
            .ok()
            .filter(|value| !value.is_empty())
            .map(|bucket| S3Config {
                bucket,
                region: env::var("TIDAS_EXPORT_TEST_S3_REGION")
                    .unwrap_or_else(|_| "us-east-1".to_owned()),
                endpoint: env::var("TIDAS_EXPORT_TEST_S3_ENDPOINT").ok(),
                prefix: None,
                access_key_id: SecretString::new(
                    env::var("TIDAS_EXPORT_TEST_S3_ACCESS_KEY_ID").unwrap(),
                ),
                secret_access_key: SecretString::new(
                    env::var("TIDAS_EXPORT_TEST_S3_SECRET_ACCESS_KEY").unwrap(),
                ),
                session_token: None,
            });
        request.skip_external_documents = request.external_documents.is_none();
        request
    };

    let first_request = make_request("first.zip");
    let first = run_export(&first_request).unwrap();
    let second_request = make_request("second.zip");
    let second = run_export(&second_request).unwrap();
    assert_eq!(first.database_record_count, second.database_record_count);
    assert!(first.database_record_count > 0);
    assert_eq!(
        first
            .version_normalization
            .as_ref()
            .expect("TIDAS exports normalize versions")
            .removed_records,
        1
    );
    assert_eq!(first.archive_sha256, second.archive_sha256);
    assert_eq!(
        fs::read(&first_request.output_zip).unwrap(),
        fs::read(&second_request.output_zip).unwrap()
    );
    if first_request.external_documents.is_some() {
        assert!(first.external_document_count > 0);
        assert!(!first.external_documents_skipped);
    }

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        let (client, connection) = tokio_postgres::connect(&database_url, tokio_postgres::NoTls)
            .await
            .unwrap();
        let connection_task = tokio::spawn(connection);
        client
            .batch_execute("DROP TABLE lciamethods")
            .await
            .unwrap();
        drop(client);
        connection_task.await.unwrap().unwrap();
    });
    let warning_request = make_request("warning.zip");
    let warning = run_export(&warning_request).unwrap();
    assert!(
        warning
            .warnings
            .iter()
            .any(|message| message.contains("'lciamethods'"))
    );
    assert!(warning_request.output_zip.is_file());
}
