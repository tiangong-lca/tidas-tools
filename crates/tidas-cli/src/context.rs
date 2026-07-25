use std::env;
use std::io::{self, IsTerminal};

use tidas_contracts::{
    ConfigSourceV1, DiagnosticDestinationV1, INVOCATION_CONTEXT_SCHEMA_V1, InputPolicyV1,
    InvocationContextV1, ReportDestinationV1,
};
use tidas_runtime::{CancellationToken, MemoryBudget};

use crate::args::{Cli, CliProgressMode, OutputFormat};

#[derive(Clone, Debug)]
pub struct ExecutionContext {
    pub invocation: InvocationContextV1,
    pub cancellation: CancellationToken,
    pub memory_budget: MemoryBudget,
}

impl ExecutionContext {
    #[must_use]
    pub fn from_cli(cli: &Cli) -> Self {
        let (config_source, config_path) = resolve_config(cli);
        let progress_enabled = match cli.progress {
            CliProgressMode::Always => true,
            CliProgressMode::Never => false,
            CliProgressMode::Auto => {
                io::stderr().is_terminal() && matches!(cli.format, OutputFormat::Human)
            }
        };
        let memory_budget = MemoryBudget::new(cli.memory_budget_bytes());
        Self {
            invocation: InvocationContextV1 {
                schema_version: INVOCATION_CONTEXT_SCHEMA_V1.to_owned(),
                config_source,
                config_path,
                log_level: cli.log_level.into(),
                progress_mode: cli.progress.into(),
                progress_enabled,
                memory_budget_bytes: memory_budget.limit(),
                queue_capacity: cli.queue_capacity,
                input_policy: InputPolicyV1::ExplicitPathOrDash,
                report_destination: if cli.report.is_some() {
                    ReportDestinationV1::File
                } else {
                    ReportDestinationV1::Stdout
                },
                diagnostic_destination: DiagnosticDestinationV1::Stderr,
            },
            cancellation: CancellationToken::default(),
            memory_budget,
        }
    }

    pub fn install_cancellation_handler(&self) -> Result<(), ctrlc::Error> {
        let cancellation = self.cancellation.clone();
        ctrlc::set_handler(move || cancellation.cancel())
    }
}

fn resolve_config(cli: &Cli) -> (ConfigSourceV1, Option<String>) {
    if let Some(path) = &cli.config {
        return (
            ConfigSourceV1::Cli,
            Some(path.to_string_lossy().into_owned()),
        );
    }
    match env::var_os("TIDAS_CONFIG").filter(|value| !value.is_empty()) {
        Some(path) => (
            ConfigSourceV1::Environment,
            Some(path.to_string_lossy().into_owned()),
        ),
        None => (ConfigSourceV1::None, None),
    }
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use super::*;
    use crate::args::DEFAULT_MEMORY_BUDGET_MIB;

    #[test]
    fn defaults_are_bounded_and_do_not_search_the_current_directory() {
        let cli = Cli::try_parse_from(["tidas", "version"]).unwrap();
        let context = ExecutionContext::from_cli(&cli);
        assert_eq!(context.invocation.config_source, ConfigSourceV1::None);
        assert_eq!(context.invocation.config_path, None);
        assert_eq!(
            context.invocation.memory_budget_bytes,
            DEFAULT_MEMORY_BUDGET_MIB * 1024 * 1024
        );
        assert_eq!(context.memory_budget.limit(), 512 * 1024 * 1024);
    }
}
