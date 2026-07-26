use std::collections::BTreeMap;

use encoding_rs::WINDOWS_1252;
use serde_json::{Map, Value, json};
use uuid::Uuid;

use super::{AdapterContext, AdapterError, SourceAdapter};
use crate::detect::SourceFormat;
use crate::model::CanonicalEntity;
use crate::report::{ImportIssue, IssueSeverity, IssueSink};
use crate::source::{SourceReadRequest, visit_source_entries};
use crate::store::CanonicalStore;

pub struct SimaProCsvAdapter;

impl SourceAdapter for SimaProCsvAdapter {
    fn format(&self) -> SourceFormat {
        SourceFormat::SimaproCsv
    }

    fn read(
        &self,
        context: &AdapterContext<'_>,
        store: &mut CanonicalStore,
        issues: &mut dyn IssueSink,
    ) -> Result<(), AdapterError> {
        let request = SourceReadRequest {
            source: context.source,
            allowed_extensions: &["csv", "txt"],
            max_entry_bytes: context.max_entry_bytes,
            cancellation: context.cancellation,
            memory_budget: context.memory_budget,
        };
        let mut process_count = 0_u64;
        visit_source_entries(&request, |entry| {
            let _structured_reservation =
                context.reserve_structured_expansion(entry.bytes.len(), 3)?;
            let (text, _, _) = WINDOWS_1252.decode(entry.bytes);
            let separator = separator(&text);
            visit_process_blocks(text.lines(), |index, sections| {
                context
                    .cancellation
                    .check()
                    .map_err(crate::source::SourceReadError::from)?;
                let process = process_entity(&sections, &entry.stable_key, index);
                store.begin_process_exchanges(&process.internal_id)?;
                let mut exchange_count = 0_usize;
                for section in &sections {
                    for row in &section.rows {
                        let Some(mut exchange) = exchange(
                            &section.name,
                            row,
                            separator,
                            exchange_count.saturating_add(1),
                        ) else {
                            continue;
                        };
                        let flow = flow_entity(&section.name, &exchange, &entry.stable_key);
                        super::generated_units::add_for_flow(store, &flow)?;
                        store.add(&flow)?;
                        exchange.insert(
                            "flow".to_owned(),
                            json!({"@id": flow.internal_id, "name": flow.name}),
                        );
                        exchange.insert(
                            "flowRefId".to_owned(),
                            Value::String(flow.internal_id.clone()),
                        );
                        exchange.insert(
                            "flowName".to_owned(),
                            Value::String(flow.name.clone().unwrap_or_default()),
                        );
                        store.add_process_exchange(&process.internal_id, &exchange)?;
                        exchange_count = exchange_count.saturating_add(1);
                    }
                }
                store.add(&process)?;
                process_count = process_count.saturating_add(1);
                issues.push(&ImportIssue {
                    severity: IssueSeverity::Warning,
                    code: "simapro_csv_minimal_mapping".to_owned(),
                    message: "Mapped SimaPro CSV process with the current minimal adapter."
                        .to_owned(),
                    source_object: Some(entry.label.clone()),
                    context: BTreeMap::from([
                        ("process_id".to_owned(), Value::String(process.internal_id)),
                        ("exchange_count".to_owned(), json!(exchange_count)),
                    ]),
                })?;
                Ok::<(), AdapterError>(())
            })?;
            Ok::<(), AdapterError>(())
        })?;
        if process_count == 0 {
            issues.push(&ImportIssue {
                severity: IssueSeverity::Error,
                code: "no_simapro_process_blocks".to_owned(),
                message: "No SimaPro Process blocks were found.".to_owned(),
                source_object: None,
                context: BTreeMap::new(),
            })?;
        }
        Ok(())
    }
}

type Sections = Vec<Section>;

struct Section {
    name: String,
    rows: Vec<String>,
}

fn separator(text: &str) -> char {
    text.lines()
        .take(30)
        .find_map(|line| {
            line.strip_prefix("{CSV separator:")
                .and_then(|value| value.strip_suffix('}'))
                .map(str::trim)
                .and_then(|value| match value.to_ascii_lowercase().as_str() {
                    "semicolon" => Some(';'),
                    "comma" => Some(','),
                    _ => value.chars().next(),
                })
        })
        .unwrap_or(';')
}

fn visit_process_blocks<'a, E>(
    lines: impl Iterator<Item = &'a str>,
    mut visit: impl FnMut(usize, Sections) -> Result<(), E>,
) -> Result<(), E> {
    let mut lines = lines.peekable();
    let mut process_index = 0_usize;
    while let Some(line) = lines.next() {
        if line.trim() != "Process" {
            continue;
        }
        let mut sections = Sections::new();
        loop {
            let Some(line) = lines.next() else {
                process_index = process_index.saturating_add(1);
                visit(process_index, sections)?;
                return Ok(());
            };
            let line = line.trim();
            if line == "End" {
                process_index = process_index.saturating_add(1);
                visit(process_index, sections)?;
                break;
            }
            if line.is_empty() {
                continue;
            }
            let section_name = line.to_owned();
            let mut rows = Vec::new();
            let mut ended = false;
            while let Some(row) = lines.peek() {
                let row = row.trim();
                if row.is_empty() {
                    lines.next();
                    break;
                }
                if row == "End" {
                    lines.next();
                    ended = true;
                    break;
                }
                rows.push(row.to_owned());
                lines.next();
            }
            if let Some(existing) = sections
                .iter_mut()
                .find(|section| section.name == section_name)
            {
                existing.rows = rows;
            } else {
                sections.push(Section {
                    name: section_name,
                    rows,
                });
            }
            if ended {
                process_index = process_index.saturating_add(1);
                visit(process_index, sections)?;
                break;
            }
        }
    }
    Ok(())
}

fn process_entity(sections: &Sections, source: &str, index: usize) -> CanonicalEntity {
    let name = first(sections, "Process name")
        .map_or_else(|| format!("SimaPro process {index}"), str::to_owned);
    let process_id = stable_id(&format!("simapro/process/{source}/{index}/{name}"));
    CanonicalEntity {
        entity_type: "processes".to_owned(),
        internal_id: process_id,
        external_id: None,
        name: Some(name),
        category_path: Vec::new(),
        raw: Map::from_iter([(
            "description".to_owned(),
            Value::String(
                first(sections, "Comment")
                    .unwrap_or("Imported from SimaPro CSV.")
                    .to_owned(),
            ),
        )]),
    }
}

fn flow_entity(section_name: &str, exchange: &Map<String, Value>, source: &str) -> CanonicalEntity {
    let name = exchange
        .get("flowName")
        .and_then(Value::as_str)
        .unwrap_or("Unnamed flow");
    CanonicalEntity {
        entity_type: "flows".to_owned(),
        internal_id: stable_id(&format!("simapro/flow/{source}/{section_name}/{name}")),
        external_id: None,
        name: Some(name.to_owned()),
        category_path: vec![section_name.to_owned()],
        raw: Map::from_iter([
            (
                "flowType".to_owned(),
                Value::String(flow_type(section_name).to_owned()),
            ),
            (
                "unitName".to_owned(),
                exchange.get("unitName").cloned().unwrap_or(Value::Null),
            ),
        ]),
    }
}

fn exchange(
    section_name: &str,
    row: &str,
    separator: char,
    row_index: usize,
) -> Option<Map<String, Value>> {
    let indexes = exchange_indexes(section_name)?;
    let parts = row
        .split(separator)
        .map(|part| part.trim().replace('\u{7f}', "\n"))
        .collect::<Vec<_>>();
    let flow_name = parts.first()?.as_str();
    if flow_name.is_empty() {
        return None;
    }
    let amount = parts.get(indexes.amount).map_or("0", String::as_str);
    Some(Map::from_iter([
        ("internalId".to_owned(), json!(row_index)),
        ("flowName".to_owned(), Value::String(flow_name.to_owned())),
        ("isInput".to_owned(), Value::Bool(indexes.is_input)),
        ("amount".to_owned(), Value::String(numeric(amount))),
        (
            "unitName".to_owned(),
            parts
                .get(indexes.unit)
                .map_or(Value::Null, |value| Value::String(value.clone())),
        ),
    ]))
}

struct ExchangeIndexes {
    amount: usize,
    unit: usize,
    is_input: bool,
}

fn exchange_indexes(section: &str) -> Option<ExchangeIndexes> {
    let (amount, unit, is_input) = match section {
        "Products" | "Avoided products" => (2, 1, false),
        "Materials/fuels" | "Electricity/heat" => (2, 1, true),
        "Resources" | "Waste to treatment" => (3, 2, true),
        "Emissions to air"
        | "Emissions to water"
        | "Emissions to soil"
        | "Final waste flows"
        | "Non material emissions"
        | "Social issues"
        | "Economic issues" => (3, 2, false),
        _ => return None,
    };
    Some(ExchangeIndexes {
        amount,
        unit,
        is_input,
    })
}

fn flow_type(section: &str) -> &'static str {
    match section {
        "Final waste flows" | "Waste to treatment" => "WASTE_FLOW",
        "Resources"
        | "Emissions to air"
        | "Emissions to water"
        | "Emissions to soil"
        | "Non material emissions"
        | "Social issues"
        | "Economic issues" => "ELEMENTARY_FLOW",
        _ => "PRODUCT_FLOW",
    }
}

fn numeric(value: &str) -> String {
    value
        .trim()
        .replace(',', ".")
        .parse::<f64>()
        .map_or_else(|_| "0".to_owned(), |value| value.to_string())
}

fn first<'a>(sections: &'a Sections, key: &str) -> Option<&'a str> {
    sections
        .iter()
        .find(|section| section.name == key)
        .and_then(|section| section.rows.first())
        .map(String::as_str)
}

fn stable_id(seed: &str) -> String {
    Uuid::new_v5(&Uuid::NAMESPACE_URL, seed.as_bytes()).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::report::IssueSpool;
    use tempfile::tempdir;
    use tidas_runtime::{CancellationToken, MemoryBudget};

    #[test]
    fn simapro_fixture_matches_the_frozen_canonical_semantics() {
        let directory = tempdir().unwrap();
        let source = directory.path().join("source.csv");
        std::fs::write(
            &source,
            b"{SimaPro 9.5}\n{CSV separator: semicolon}\n\nProcess\n\nProcess name\nSteel production\n\nComment\nfixture\n\nProducts\nSteel;kg;1\n\nEmissions to air\nCarbon dioxide;air;kg;2.5\n\nEnd\n",
        )
        .unwrap();
        let cancellation = CancellationToken::default();
        let memory_budget = MemoryBudget::new(1024 * 1024);
        let context = AdapterContext {
            source: &source,
            cancellation: &cancellation,
            memory_budget: &memory_budget,
            max_entry_bytes: 1024 * 1024,
        };
        let mut store = CanonicalStore::create(None).unwrap();
        let mut issues = IssueSpool::new(Vec::new(), 64 * 1024);
        SimaProCsvAdapter
            .read(&context, &mut store, &mut issues)
            .unwrap();
        let (_, issue_summary) = issues.finish().unwrap();

        assert_eq!(store.counts().get("processes"), Some(&1));
        assert_eq!(store.counts().get("flows"), Some(&2));
        assert_eq!(issue_summary.warning_count, 1);
        assert_eq!(issue_summary.error_count, 0);
        let process = store
            .iter_type("processes")
            .unwrap()
            .next()
            .unwrap()
            .unwrap();
        assert_eq!(process.name.as_deref(), Some("Steel production"));
        let exchanges = store
            .iter_process_exchanges(&process.internal_id)
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(exchanges.len(), 2);
        assert_eq!(exchanges[0]["amount"], "1");
        assert_eq!(exchanges[1]["amount"], "2.5");
        assert_eq!(memory_budget.used(), 0);
    }

    #[test]
    fn missing_process_blocks_are_a_spooled_data_issue() {
        let directory = tempdir().unwrap();
        let source = directory.path().join("empty.csv");
        std::fs::write(&source, b"{SimaPro 9.5}\n").unwrap();
        let cancellation = CancellationToken::default();
        let memory_budget = MemoryBudget::new(1024);
        let context = AdapterContext {
            source: &source,
            cancellation: &cancellation,
            memory_budget: &memory_budget,
            max_entry_bytes: 1024,
        };
        let mut store = CanonicalStore::create(None).unwrap();
        let mut issues = IssueSpool::new(Vec::new(), 64 * 1024);
        SimaProCsvAdapter
            .read(&context, &mut store, &mut issues)
            .unwrap();
        let (bytes, summary) = issues.finish().unwrap();
        assert_eq!(summary.error_count, 1);
        let issue: ImportIssue =
            serde_json::from_slice(bytes.split(|byte| *byte == b'\n').next().unwrap()).unwrap();
        assert_eq!(issue.code, "no_simapro_process_blocks");
    }
}
