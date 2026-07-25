use std::fmt::Write as _;
use std::fs;
use std::io::{self, Write};
use std::path::Path;

use tidas_contracts::OperationReportV1;

use crate::args::{Cli, OutputFormat};

pub fn render(cli: &Cli, report: &OperationReportV1) -> io::Result<()> {
    let bytes = match cli.format {
        OutputFormat::Json => report.to_canonical_json_line().map_err(io::Error::other)?,
        OutputFormat::Human => human_report(report).into_bytes(),
    };
    if let Some(path) = &cli.report {
        write_atomic(path, &bytes)?;
        writeln!(
            io::stderr().lock(),
            "wrote {} report to {}",
            report.command.as_str(),
            path.display()
        )?;
    } else {
        io::stdout().lock().write_all(&bytes)?;
    }
    Ok(())
}

fn human_report(report: &OperationReportV1) -> String {
    let mut output = format!(
        "tidas {}\n\nSummary:\n- status: {}\n- exit class: {}\n- completeness: {}\n",
        report.command.as_str(),
        enum_name(report.status),
        enum_name(report.exit_class),
        enum_name(report.completeness),
    );
    for (key, value) in &report.summary {
        writeln!(&mut output, "- {key}: {}", human_value(value))
            .expect("writing to a String cannot fail");
    }
    for diagnostic in &report.diagnostics {
        writeln!(&mut output, "- {}: {}", diagnostic.code, diagnostic.message)
            .expect("writing to a String cannot fail");
    }
    if let Some(invocation) = &report.invocation {
        output.push_str("\nRuntime:\n");
        writeln!(
            &mut output,
            "- memory budget: {} bytes",
            invocation.memory_budget_bytes
        )
        .expect("writing to a String cannot fail");
        writeln!(
            &mut output,
            "- queue capacity: {}",
            invocation.queue_capacity
        )
        .expect("writing to a String cannot fail");
        writeln!(
            &mut output,
            "- config source: {}",
            enum_name(invocation.config_source)
        )
        .expect("writing to a String cannot fail");
    }
    if !report.artifacts.is_empty() {
        output.push_str("\nArtifacts:\n");
        for artifact in &report.artifacts {
            writeln!(&mut output, "- {} ({})", artifact.path, artifact.media_type)
                .expect("writing to a String cannot fail");
        }
    }
    if !report.next_actions.is_empty() {
        output.push_str("\nNext:\n");
        for action in &report.next_actions {
            writeln!(&mut output, "- {action}").expect("writing to a String cannot fail");
        }
    }
    output
}

fn enum_name<T: serde::Serialize>(value: T) -> String {
    serde_json::to_value(value)
        .ok()
        .and_then(|encoded| encoded.as_str().map(str::to_owned))
        .unwrap_or_else(|| "unknown".to_owned())
}

fn human_value(value: &serde_json::Value) -> String {
    value
        .as_str()
        .map_or_else(|| value.to_string(), str::to_owned)
}

fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "report path needs a file name")
        })?;
    let temporary = path.with_file_name(format!(".{file_name}.tmp"));
    fs::write(&temporary, bytes)?;
    fs::rename(temporary, path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tidas_contracts::{CommandNameV1, OperationReportV1};

    #[test]
    fn human_output_is_result_first_and_next_action_last() {
        let report = OperationReportV1::unavailable(
            CommandNameV1::Validate,
            "Run the tracked validation slice.",
        );
        let output = human_report(&report);
        assert!(output.starts_with("tidas validate\n\nSummary:"));
        assert!(output.ends_with("- Run the tracked validation slice.\n"));
    }
}
