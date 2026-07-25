mod args;
mod context;
mod output;

use std::process::ExitCode;

use args::{Cli, Commands, ValidateArgs, ValidationInputFormat};
use clap::error::ErrorKind;
use context::ExecutionContext;
use tidas_assets::{asset_fingerprint, bundled_assets};
use tidas_contracts::{
    ArtifactRefV1, CommandNameV1, DiagnosticV1, ExitClass, INVOCATION_CONTEXT_SCHEMA_V1,
    OPERATION_REPORT_SCHEMA_V1, OperationReportV1,
};
use tidas_runtime::RuntimeError;
use tidas_validation::{
    ValidationError, ValidationRequest, ValidationSummaryV1, validate_tidas_package,
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
        Commands::Validate(arguments) => validation_report(&arguments, execution),
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

fn validation_report(arguments: &ValidateArgs, execution: &ExecutionContext) -> OperationReportV1 {
    if arguments.input_format == ValidationInputFormat::IlcdXml {
        return OperationReportV1::unavailable(
            CommandNameV1::Validate,
            "ILCD XML validation is the next active path in tidas-tools#120; use --input-format tidas-json for the native path available now.",
        );
    }
    let request = ValidationRequest {
        input_dir: arguments.input.clone(),
        issue_spool: arguments.issues.clone(),
        cancellation: execution.cancellation.clone(),
        memory_budget: execution.memory_budget.clone(),
        queue_capacity: execution.invocation.queue_capacity,
    };
    match validate_tidas_package(&request) {
        Ok(output) => completed_validation_report(output.summary, output.issue_spool_path),
        Err(ValidationError::Runtime(RuntimeError::Cancelled)) => {
            OperationReportV1::cancelled(CommandNameV1::Validate)
        }
        Err(error) => failed_validation_report(&error),
    }
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
        ValidationError::Runtime(RuntimeError::BudgetExceeded { .. }) => {
            (ExitClass::Internal, "memory_budget_exceeded")
        }
        ValidationError::Runtime(_) => (ExitClass::Internal, "validation_runtime_failed"),
        ValidationError::ZeroQueueCapacity => (ExitClass::Usage, "invalid_queue_capacity"),
        ValidationError::PathOutsideInput(_)
        | ValidationError::SizeOverflow
        | ValidationError::Asset(_)
        | ValidationError::Schema(_) => (ExitClass::Internal, "validation_setup_failed"),
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
    report.next_actions.push(
        "Run `tidas --help` to inspect the stable command tree; functional slices are tracked under tidas-tools#117."
            .to_owned(),
    );
    report
}
