use std::fmt::Write as _;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

#[cfg(test)]
use clap::CommandFactory;
use clap::error::ErrorKind;
use clap::{Parser, Subcommand, ValueEnum};
use tidas_assets::{asset_fingerprint, bundled_assets};
use tidas_contracts::{ExitClass, OperationReportV1};
use tidas_xml::engine_decision;

#[derive(Debug, Parser)]
#[command(
    name = "tidas",
    version,
    about = "Cross-platform TIDAS conversion, import, export, validation, and release tooling",
    long_about = "The unified TIDAS executable. Domain behavior lives in reusable Rust crates; this binary only parses inputs and renders stable human or JSON results.",
    after_help = "JSON stdout is a stable contract and never includes logs. Use --output for durable reports. During migration, commands whose Rust slice is not complete fail with the unavailable exit class instead of invoking Python."
)]
struct Cli {
    /// Reading-oriented human output or stable machine-readable JSON.
    #[arg(long, value_enum, default_value_t = OutputFormat::Human, global = true)]
    format: OutputFormat,

    /// Write the complete result to a file instead of stdout.
    #[arg(long, value_name = "PATH", global = true)]
    output: Option<PathBuf>,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
enum OutputFormat {
    Human,
    Json,
}

#[derive(Debug, Subcommand)]
enum Commands {
    /// Convert between TIDAS JSON and eILCD XML.
    Convert,
    /// Import supported external LCA formats into TIDAS.
    Import,
    /// Export database records and external documents as a package.
    Export,
    /// Validate TIDAS JSON or eILCD/ILCD XML.
    Validate,
    /// Build and verify deterministic release packages.
    Release,
    /// Inspect and validate packaged methodology rulesets.
    Ruleset,
    /// Print binary, contract, asset, and XML engine fingerprints.
    Version,
}

impl Commands {
    const fn name(&self) -> &'static str {
        match self {
            Self::Convert => "convert",
            Self::Import => "import",
            Self::Export => "export",
            Self::Validate => "validate",
            Self::Release => "release",
            Self::Ruleset => "ruleset",
            Self::Version => "version",
        }
    }
}

fn main() -> ExitCode {
    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(error) => {
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
            return ExitCode::from(code.code());
        }
    };

    let report = match run(&cli) {
        Ok(report) => report,
        Err(error) => {
            eprintln!("tidas failed before producing a result: {error}");
            return ExitCode::from(ExitClass::Internal.code());
        }
    };
    let exit_class = report.exit_class;
    match render(&cli, &report) {
        Ok(()) => ExitCode::from(exit_class.code()),
        Err(error) => {
            eprintln!("tidas failed to render its result: {error}");
            ExitCode::from(ExitClass::Io.code())
        }
    }
}

fn run(cli: &Cli) -> Result<OperationReportV1, Box<dyn std::error::Error>> {
    if matches!(cli.command, Commands::Version) {
        let mut report = OperationReportV1::succeeded("version");
        report.summary.insert(
            "binary_version".to_owned(),
            serde_json::json!(env!("CARGO_PKG_VERSION")),
        );
        report.summary.insert(
            "asset_fingerprint".to_owned(),
            serde_json::json!(asset_fingerprint()?),
        );
        report.summary.insert(
            "asset_count".to_owned(),
            serde_json::json!(bundled_assets().len()),
        );
        report.summary.insert(
            "xml_engine".to_owned(),
            serde_json::to_value(engine_decision())?,
        );
        report.next_actions.push(
            "Run `tidas --help` to inspect the final command tree; functional slices are tracked under tidas-tools#117."
                .to_owned(),
        );
        return Ok(report);
    }

    Ok(OperationReportV1::unavailable(
        cli.command.name(),
        format!(
            "Follow the `{}` Rust migration child Issue linked from tidas-tools#117.",
            cli.command.name()
        ),
    ))
}

fn render(cli: &Cli, report: &OperationReportV1) -> io::Result<()> {
    let bytes = match cli.format {
        OutputFormat::Json => report.to_canonical_json_line().map_err(io::Error::other)?,
        OutputFormat::Human => human_report(report).into_bytes(),
    };
    if let Some(path) = &cli.output {
        write_atomic(path, &bytes)?;
        let mut stderr = io::stderr().lock();
        writeln!(
            stderr,
            "wrote {} result to {}",
            report.command,
            path.display()
        )?;
    } else {
        io::stdout().lock().write_all(&bytes)?;
    }
    Ok(())
}

fn human_report(report: &OperationReportV1) -> String {
    let mut output = format!("tidas {}\n\nSummary:\n", report.command);
    writeln!(&mut output, "- status: {:?}", report.status)
        .expect("writing to a String cannot fail");
    writeln!(&mut output, "- completeness: {:?}", report.completeness)
        .expect("writing to a String cannot fail");
    for (key, value) in &report.summary {
        writeln!(&mut output, "- {key}: {value}").expect("writing to a String cannot fail");
    }
    for diagnostic in &report.diagnostics {
        writeln!(&mut output, "- {}: {}", diagnostic.code, diagnostic.message)
            .expect("writing to a String cannot fail");
    }
    if !report.next_actions.is_empty() {
        output.push_str("\nNext:\n");
        for action in &report.next_actions {
            writeln!(&mut output, "- {action}").expect("writing to a String cannot fail");
        }
    }
    output
}

fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "output path needs a file name")
        })?;
    let temporary = path.with_file_name(format!(".{file_name}.tmp"));
    fs::write(&temporary, bytes)?;
    fs::rename(temporary, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clap_contract_is_internally_consistent() {
        Cli::command().debug_assert();
    }

    #[test]
    fn only_the_final_command_names_are_registered() {
        let command = Cli::command();
        let names: Vec<_> = command
            .get_subcommands()
            .map(clap::Command::get_name)
            .collect();
        assert_eq!(
            names,
            [
                "convert", "import", "export", "validate", "release", "ruleset", "version"
            ]
        );
        for legacy in [
            "tidas-convert",
            "tidas-import",
            "tidas-export",
            "tidas-validate",
            "tidas-release-tool",
        ] {
            assert!(!names.contains(&legacy));
        }
    }

    #[test]
    fn human_output_is_result_first_and_next_action_last() {
        let report =
            OperationReportV1::unavailable("validate", "Run the tracked validation slice.");
        let output = human_report(&report);
        assert!(output.starts_with("tidas validate\n\nSummary:"));
        assert!(output.ends_with("- Run the tracked validation slice.\n"));
    }
}
