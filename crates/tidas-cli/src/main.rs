mod args;
mod context;
mod output;

use std::process::ExitCode;

use args::{
    Cli, Commands, ConversionTarget, ConvertArgs, RulesetArgs, ValidateArgs, ValidationInputFormat,
    ValidationProtocol,
};
use clap::error::ErrorKind;
use context::ExecutionContext;
use tidas_assets::{asset_fingerprint, bundled_assets};
use tidas_contracts::{
    ArtifactRefV1, CommandNameV1, DiagnosticV1, ExitClass, INVOCATION_CONTEXT_SCHEMA_V1,
    OPERATION_REPORT_SCHEMA_V1, OperationReportV1,
};
use tidas_conversion::{
    ConversionDirection, ConversionError, ConversionProgressReporter, ConversionRequest,
    convert_directory,
};
use tidas_rulesets::{RulesetCatalog, RulesetError};
use tidas_runtime::RuntimeError;
use tidas_validation::{
    BatchValidationOutput, BatchValidationRequest, DOCUMENT_VALIDATION_PROFILE, ValidationError,
    ValidationProgressReporter, ValidationRequest, ValidationSummaryV1,
    describe_document_validation, run_document_validation_batch, validate_ilcd_package,
    validate_tidas_package,
};
use tidas_xml::engine_decision;

fn main() -> ExitCode {
    let cli = match Cli::try_parse_checked() {
        Ok(cli) => cli,
        Err(error) => return print_clap_error(&error),
    };

    if let Some(shell) = cli.completion {
        return match args::write_completion(shell, &mut std::io::stdout().lock()) {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("failed to write {shell} completion script: {error}");
                ExitCode::from(ExitClass::Io.code())
            }
        };
    }

    let execution = ExecutionContext::from_cli(&cli);
    let command = cli
        .command
        .clone()
        .expect("checked CLI always has a command unless completion was requested");
    let command_name = command.name();
    let report = match execution.install_cancellation_handler() {
        Ok(()) => dispatch(command, &execution),
        Err(error) => OperationReportV1::failed(
            command_name,
            ExitClass::Internal,
            "cancellation_handler_unavailable",
            format!("Failed to install the process cancellation handler: {error}"),
        ),
    }
    .with_invocation(execution.invocation.clone());

    let exit_class = report.exit_class;
    match output::render(&cli, &report) {
        Ok(()) => ExitCode::from(exit_class.code()),
        Err(error) => {
            eprintln!("tidas failed to render its report: {error}");
            ExitCode::from(ExitClass::Io.code())
        }
    }
}

fn print_clap_error(error: &clap::Error) -> ExitCode {
    let kind = error.kind();
    let code = if matches!(kind, ErrorKind::DisplayHelp | ErrorKind::DisplayVersion) {
        ExitClass::Success
    } else {
        ExitClass::Usage
    };
    if let Err(print_error) = error.print() {
        eprintln!("failed to print CLI guidance: {print_error}");
        return ExitCode::from(ExitClass::Io.code());
    }
    ExitCode::from(code.code())
}

fn dispatch(command: Commands, execution: &ExecutionContext) -> OperationReportV1 {
    if execution.cancellation.is_cancelled() {
        return OperationReportV1::cancelled(command.name());
    }

    match command {
        Commands::Convert(arguments) => conversion_report(&arguments, execution),
        Commands::Validate(arguments) => validation_report(&arguments, execution),
        Commands::Ruleset(arguments) => ruleset_report(&arguments),
        Commands::Version => version_report(execution),
        other => OperationReportV1::unavailable(
            other.name(),
            format!(
                "Follow the `{}` Rust migration child Issue linked from tidas-tools#117.",
                other.name().as_str()
            ),
        ),
    }
}

fn conversion_report(arguments: &ConvertArgs, execution: &ExecutionContext) -> OperationReportV1 {
    let direction = match arguments.to {
        ConversionTarget::Ilcd => ConversionDirection::TidasToIlcd,
        ConversionTarget::Tidas => ConversionDirection::IlcdToTidas,
    };
    let request = ConversionRequest {
        input_dir: arguments.input.clone(),
        output_dir: arguments.output.clone(),
        direction,
        cancellation: execution.cancellation.clone(),
        memory_budget: execution.memory_budget.clone(),
        queue_capacity: execution.invocation.queue_capacity,
        progress: execution
            .invocation
            .progress_enabled
            .then(conversion_progress_reporter),
    };
    match convert_directory(&request) {
        Ok(summary) => {
            let mut report = OperationReportV1::succeeded(CommandNameV1::Convert);
            report.summary.insert(
                "conversion".to_owned(),
                serde_json::to_value(&summary).expect("conversion report contract is serializable"),
            );
            report.artifacts.push(ArtifactRefV1 {
                path: arguments.output.to_string_lossy().into_owned(),
                media_type: "application/vnd.tidas.package-directory".to_owned(),
                sha256: Some(summary.output_tree_sha256),
                bytes: Some(summary.output_bytes),
            });
            let input_format = match arguments.to {
                ConversionTarget::Ilcd => "ilcd-xml",
                ConversionTarget::Tidas => "tidas-json",
            };
            report.next_actions.push(format!(
                "tidas validate {} --input-format {input_format}",
                arguments.output.join("data").display()
            ));
            report
        }
        Err(ConversionError::Runtime(RuntimeError::Cancelled)) => {
            OperationReportV1::cancelled(CommandNameV1::Convert)
        }
        Err(error) => failed_conversion_report(&error),
    }
}

fn conversion_progress_reporter() -> ConversionProgressReporter {
    ConversionProgressReporter::new(|progress| {
        eprintln!(
            "tidas progress: convert phase={} direction={} files={} converted={} copied={} assets={}",
            progress.phase,
            progress.direction.as_str(),
            progress.files_processed,
            progress.converted_file_count,
            progress.copied_file_count,
            progress.asset_file_count,
        );
    })
}

fn failed_conversion_report(error: &ConversionError) -> OperationReportV1 {
    let (exit_class, code) = match error {
        ConversionError::InputNotDirectory(_)
        | ConversionError::OutputNotDirectory(_)
        | ConversionError::InvalidOutput(_)
        | ConversionError::SourceChanged(_)
        | ConversionError::CommitRollback { .. }
        | ConversionError::Io(_)
        | ConversionError::Walk(_) => (ExitClass::Io, "conversion_io_failed"),
        ConversionError::OutputInsideInput(_) | ConversionError::ZeroQueueCapacity => {
            (ExitClass::Usage, "invalid_conversion_request")
        }
        ConversionError::JsonRootNotObject
        | ConversionError::JsonRootCount(_)
        | ConversionError::MissingDatasetRoot { .. }
        | ConversionError::InvalidEnvelope(_)
        | ConversionError::OrphanEnvelopeSidecar(_)
        | ConversionError::EnvelopeKeyCollision { .. }
        | ConversionError::InvalidXmlName(_)
        | ConversionError::InvalidXmlCharacter(_)
        | ConversionError::NonScalarText
        | ConversionError::TextOutsideRoot
        | ConversionError::MultipleRoots
        | ConversionError::MissingRoot
        | ConversionError::UnmatchedEnd
        | ConversionError::UnclosedElements
        | ConversionError::DoctypeForbidden
        | ConversionError::Symlink(_)
        | ConversionError::Json(_)
        | ConversionError::Xml(_)
        | ConversionError::Attribute(_)
        | ConversionError::Encoding(_)
        | ConversionError::Escape(_) => (ExitClass::DataIssues, "conversion_input_invalid"),
        ConversionError::Runtime(RuntimeError::BudgetExceeded { .. }) => {
            (ExitClass::Internal, "memory_budget_exceeded")
        }
        ConversionError::Runtime(_) => (ExitClass::Internal, "conversion_runtime_failed"),
        ConversionError::PathOutsideInput(_)
        | ConversionError::NonPortablePath(_)
        | ConversionError::SizeOverflow
        | ConversionError::Asset(_) => (ExitClass::Internal, "conversion_setup_failed"),
    };
    OperationReportV1::failed(CommandNameV1::Convert, exit_class, code, error.to_string())
}

fn ruleset_report(arguments: &RulesetArgs) -> OperationReportV1 {
    let catalog = match RulesetCatalog::load() {
        Ok(catalog) => catalog,
        Err(error) => {
            return OperationReportV1::failed(
                CommandNameV1::Ruleset,
                ExitClass::Internal,
                "ruleset_catalog_invalid",
                error.to_string(),
            );
        }
    };
    let mut report = OperationReportV1::succeeded(CommandNameV1::Ruleset);
    report.summary.insert(
        "ruleset_description".to_owned(),
        serde_json::to_value(catalog.description())
            .expect("ruleset description contract is serializable"),
    );
    report.summary.insert(
        "methodology_validation".to_owned(),
        serde_json::to_value(catalog.methodology_report())
            .expect("methodology validation contract is serializable"),
    );
    if let Some(id) = &arguments.id {
        match catalog.rules_for(id) {
            Ok(rules) => {
                report
                    .summary
                    .insert("ruleset_id".to_owned(), serde_json::json!(id));
                report.summary.insert(
                    "rules".to_owned(),
                    serde_json::to_value(rules).expect("packaged rules are serializable"),
                );
            }
            Err(RulesetError::UnknownRuleset(_)) => {
                return OperationReportV1::failed(
                    CommandNameV1::Ruleset,
                    ExitClass::Usage,
                    "unknown_ruleset",
                    error_message_for_unknown_ruleset(
                        id,
                        catalog.description().ruleset_ids.as_slice(),
                    ),
                );
            }
            Err(error) => {
                return OperationReportV1::failed(
                    CommandNameV1::Ruleset,
                    ExitClass::Internal,
                    "ruleset_resolution_failed",
                    error.to_string(),
                );
            }
        }
    } else {
        report
            .summary
            .insert("catalog".to_owned(), catalog.metadata().clone());
    }
    report
}

fn error_message_for_unknown_ruleset(id: &str, available: &[String]) -> String {
    format!(
        "Unknown ruleset id '{id}'. Available ids: {}",
        available.join(", ")
    )
}

fn validation_report(arguments: &ValidateArgs, execution: &ExecutionContext) -> OperationReportV1 {
    if arguments.describe {
        return validation_describe_report();
    }
    let input = arguments
        .input
        .clone()
        .expect("checked validate arguments require an input");
    let request = ValidationRequest {
        input_dir: input,
        issue_spool: arguments.issues.clone(),
        cancellation: execution.cancellation.clone(),
        memory_budget: execution.memory_budget.clone(),
        queue_capacity: execution.invocation.queue_capacity,
        progress: execution
            .invocation
            .progress_enabled
            .then(validation_progress_reporter),
    };
    if arguments.protocol == ValidationProtocol::DocumentValidationBatchV1 {
        return batch_validation_report(arguments, request);
    }
    let result = match arguments.input_format {
        ValidationInputFormat::TidasJson => validate_tidas_package(&request),
        ValidationInputFormat::IlcdXml => validate_ilcd_package(&request),
    };
    match result {
        Ok(output) => completed_validation_report(output.summary, output.issue_spool_path),
        Err(ValidationError::Runtime(RuntimeError::Cancelled)) => {
            OperationReportV1::cancelled(CommandNameV1::Validate)
        }
        Err(error) => failed_validation_report(&error),
    }
}

fn validation_progress_reporter() -> ValidationProgressReporter {
    ValidationProgressReporter::new(|progress| {
        let total = progress
            .documents_total
            .map_or_else(|| "?".to_owned(), |value| value.to_string());
        let category = progress.category.as_deref().unwrap_or("-");
        eprintln!(
            "tidas progress: validate phase={} format={} category={} documents={}/{} issues={}",
            progress.phase,
            progress.input_format,
            category,
            progress.documents_processed,
            total,
            progress.issues_found,
        );
    })
}

fn validation_describe_report() -> OperationReportV1 {
    match asset_fingerprint() {
        Ok(fingerprint) => {
            let mut report = OperationReportV1::succeeded(CommandNameV1::Validate);
            report.summary.insert(
                "validation_describe".to_owned(),
                serde_json::to_value(match describe_document_validation(fingerprint) {
                    Ok(description) => description,
                    Err(error) => {
                        return OperationReportV1::failed(
                            CommandNameV1::Validate,
                            ExitClass::Internal,
                            "validation_describe_failed",
                            error.to_string(),
                        );
                    }
                })
                .expect("validation describe contract is serializable"),
            );
            report
        }
        Err(error) => OperationReportV1::failed(
            CommandNameV1::Validate,
            ExitClass::Internal,
            "validation_describe_failed",
            error.to_string(),
        ),
    }
}

fn batch_validation_report(
    arguments: &ValidateArgs,
    validation: ValidationRequest,
) -> OperationReportV1 {
    let request = BatchValidationRequest {
        validation,
        input_manifest: arguments
            .input_manifest
            .clone()
            .expect("checked batch arguments require a manifest"),
        event_spool: arguments.events.clone(),
        profile: DOCUMENT_VALIDATION_PROFILE.to_owned(),
    };
    match run_document_validation_batch(&request) {
        Ok(output) => completed_batch_validation_report(output),
        Err(ValidationError::Runtime(RuntimeError::Cancelled)) => {
            OperationReportV1::cancelled(CommandNameV1::Validate)
        }
        Err(error) => failed_validation_report(&error),
    }
}

fn completed_batch_validation_report(output: BatchValidationOutput) -> OperationReportV1 {
    let mut report = OperationReportV1::succeeded(CommandNameV1::Validate);
    report.summary.insert(
        "validation_batch_final".to_owned(),
        serde_json::to_value(output.final_event)
            .expect("validation final event contract is serializable"),
    );
    if let (Some(path), Some(spool)) = (output.event_spool_path, output.event_spool) {
        report.artifacts.push(ArtifactRefV1 {
            path: path.to_string_lossy().into_owned(),
            media_type: "application/x-ndjson".to_owned(),
            sha256: Some(spool.sha256),
            bytes: Some(spool.bytes),
        });
    }
    report
}

fn completed_validation_report(
    summary: ValidationSummaryV1,
    issue_spool_path: Option<std::path::PathBuf>,
) -> OperationReportV1 {
    let has_issues = !summary.ok;
    let issue_spool = summary.issue_spool.clone();
    let summary_value = match serde_json::to_value(summary) {
        Ok(value) => value,
        Err(error) => {
            return OperationReportV1::failed(
                CommandNameV1::Validate,
                ExitClass::Internal,
                "validation_summary_serialization_failed",
                error.to_string(),
            );
        }
    };
    let mut report = if has_issues {
        OperationReportV1::completed_with_issues(
            CommandNameV1::Validate,
            DiagnosticV1::new(
                "validation_issues",
                "Validation completed and found data issues.",
            ),
        )
    } else {
        OperationReportV1::succeeded(CommandNameV1::Validate)
    };
    report
        .summary
        .insert("validation".to_owned(), summary_value);
    if let (Some(path), Some(spool)) = (issue_spool_path, issue_spool) {
        report.artifacts.push(ArtifactRefV1 {
            path: path.to_string_lossy().into_owned(),
            media_type: "application/x-ndjson".to_owned(),
            sha256: Some(spool.sha256),
            bytes: Some(spool.bytes),
        });
    }
    report
}

fn failed_validation_report(error: &ValidationError) -> OperationReportV1 {
    let (exit_class, code) = match &error {
        ValidationError::InputNotDirectory(_)
        | ValidationError::SpoolParentMissing(_)
        | ValidationError::PersistSpool { .. }
        | ValidationError::Io(_) => (ExitClass::Io, "validation_io_failed"),
        ValidationError::BatchProtocol(_) => (ExitClass::DataIssues, "batch_protocol_failed"),
        ValidationError::Runtime(RuntimeError::BudgetExceeded { .. }) => {
            (ExitClass::Internal, "memory_budget_exceeded")
        }
        ValidationError::Runtime(_) => (ExitClass::Internal, "validation_runtime_failed"),
        ValidationError::ZeroQueueCapacity => (ExitClass::Usage, "invalid_queue_capacity"),
        ValidationError::PathOutsideInput(_)
        | ValidationError::SizeOverflow
        | ValidationError::UnexpectedXsdAsset(_)
        | ValidationError::Walk(_)
        | ValidationError::InvalidXml(_)
        | ValidationError::Json(_)
        | ValidationError::Asset(_)
        | ValidationError::Schema(_)
        | ValidationError::Semantic(_)
        | ValidationError::Ruleset(_)
        | ValidationError::Xml(_) => (ExitClass::Internal, "validation_setup_failed"),
    };
    OperationReportV1::failed(CommandNameV1::Validate, exit_class, code, error.to_string())
}

fn version_report(execution: &ExecutionContext) -> OperationReportV1 {
    let mut report = OperationReportV1::succeeded(CommandNameV1::Version);
    report.summary.insert(
        "binary_version".to_owned(),
        serde_json::json!(env!("CARGO_PKG_VERSION")),
    );
    report.summary.insert(
        "operation_report_schema".to_owned(),
        serde_json::json!(OPERATION_REPORT_SCHEMA_V1),
    );
    report.summary.insert(
        "invocation_context_schema".to_owned(),
        serde_json::json!(INVOCATION_CONTEXT_SCHEMA_V1),
    );
    report
        .summary
        .insert("command_count".to_owned(), serde_json::json!(7));
    report.summary.insert(
        "memory_budget_bytes".to_owned(),
        serde_json::json!(execution.memory_budget.limit()),
    );
    report.summary.insert(
        "asset_count".to_owned(),
        serde_json::json!(bundled_assets().len()),
    );
    match asset_fingerprint() {
        Ok(fingerprint) => {
            report.summary.insert(
                "asset_fingerprint".to_owned(),
                serde_json::json!(fingerprint),
            );
        }
        Err(error) => {
            return OperationReportV1::failed(
                CommandNameV1::Version,
                ExitClass::Internal,
                "asset_fingerprint_failed",
                error.to_string(),
            );
        }
    }
    match serde_json::to_value(engine_decision()) {
        Ok(decision) => {
            report.summary.insert("xml_engine".to_owned(), decision);
        }
        Err(error) => {
            return OperationReportV1::failed(
                CommandNameV1::Version,
                ExitClass::Internal,
                "xml_engine_contract_failed",
                error.to_string(),
            );
        }
    }
    match RulesetCatalog::load() {
        Ok(catalog) => {
            report.summary.insert(
                "ruleset_catalog".to_owned(),
                serde_json::to_value(catalog.description())
                    .expect("ruleset description contract is serializable"),
            );
        }
        Err(error) => {
            return OperationReportV1::failed(
                CommandNameV1::Version,
                ExitClass::Internal,
                "ruleset_fingerprint_failed",
                error.to_string(),
            );
        }
    }
    report.next_actions.push(
        "Run `tidas --help` to inspect the stable command tree; functional slices are tracked under tidas-tools#117."
            .to_owned(),
    );
    report
}
