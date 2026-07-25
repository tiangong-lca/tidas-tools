//! XML portability boundary selected by issue #118.
//!
//! Streaming inspection uses pure Rust `quick-xml`. XSD 1.0 validation and
//! XSLT 1.0 transformation are isolated behind a serialized libxml2/libxslt
//! boundary so later crates do not depend on native wrapper details.

use std::path::Path;
use std::sync::{Mutex, MutexGuard};

use libxml::error::{StructuredError, XmlErrorLevel};
use libxml::parser::{Parser, ParserOptions};
use libxml::schemas::{SchemaParserContext, SchemaValidationContext};
use libxslt::parser as xslt_parser;
use quick_xml::events::Event;
use quick_xml::reader::Reader;
use serde::{Deserialize, Serialize};
use thiserror::Error;

static NATIVE_XML_ENGINE: Mutex<()> = Mutex::new(());

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct XmlInspection {
    pub root_name: String,
    pub element_count: u64,
    pub max_depth: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct XmlEngineDecisionV1 {
    pub schema_version: String,
    pub streaming_parser: String,
    pub xsd_engine: String,
    pub xslt_engine: String,
    pub concurrency: String,
    pub network_access: String,
    pub linking: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct XmlDiagnostic {
    pub message: String,
    pub level: String,
    pub filename: Option<String>,
    pub line: Option<i32>,
    pub column: Option<i32>,
    pub domain: i32,
    pub code: i32,
}

/// A reusable XSD validation context compiled from a filesystem path.
///
/// Path compilation is required for schemas that use relative imports/includes.
/// Construction and validation remain serialized behind the native engine lock.
pub struct CompiledXsd {
    validation: SchemaValidationContext,
}

impl CompiledXsd {
    pub fn from_path(path: &Path) -> Result<Self, XmlError> {
        let path = path.to_str().ok_or(XmlError::NonUtf8SchemaPath)?;
        let _guard = native_engine_guard()?;
        let mut schema_parser = SchemaParserContext::from_file(path);
        let validation = SchemaValidationContext::from_parser(&mut schema_parser)
            .map_err(|errors| XmlError::SchemaCompile(format_errors(&errors)))?;
        Ok(Self { validation })
    }

    pub fn validate(&mut self, xml: &[u8]) -> Result<Vec<XmlDiagnostic>, XmlError> {
        let _guard = native_engine_guard()?;
        let parser = Parser::default();
        let document = parser.parse_string_with_options(xml, strict_parser_options())?;
        match self.validation.validate_document(&document) {
            Ok(()) => Ok(Vec::new()),
            Err(errors) => Ok(errors.iter().map(XmlDiagnostic::from).collect()),
        }
    }
}

#[must_use]
pub fn engine_decision() -> XmlEngineDecisionV1 {
    XmlEngineDecisionV1 {
        schema_version: "tidas.xml-engine-decision.v1".to_owned(),
        streaming_parser: "quick-xml-0.41".to_owned(),
        xsd_engine: "libxml2-via-libxml-0.3".to_owned(),
        xslt_engine: "libxslt-via-libxslt-0.1".to_owned(),
        concurrency: "serialized-native-engine-boundary".to_owned(),
        network_access: "disabled-for-xml-and-xsd;-xslt-production-resolver-pending".to_owned(),
        linking: "dynamic-development;controlled-static-release".to_owned(),
    }
}

pub fn inspect_xml(bytes: &[u8]) -> Result<XmlInspection, XmlError> {
    let mut reader = Reader::from_reader(bytes);
    reader.config_mut().check_comments = true;
    reader.config_mut().check_end_names = true;
    reader.config_mut().allow_unmatched_ends = false;

    let mut buffer = Vec::new();
    let mut root_name = None;
    let mut depth = 0_u64;
    let mut max_depth = 0_u64;
    let mut element_count = 0_u64;
    loop {
        match reader.read_event_into(&mut buffer)? {
            Event::Start(element) => {
                depth += 1;
                max_depth = max_depth.max(depth);
                element_count += 1;
                if root_name.is_none() {
                    root_name = Some(String::from_utf8_lossy(element.name().as_ref()).into_owned());
                }
            }
            Event::Empty(element) => {
                element_count += 1;
                if root_name.is_none() {
                    root_name = Some(String::from_utf8_lossy(element.name().as_ref()).into_owned());
                }
                max_depth = max_depth.max(depth + 1);
            }
            Event::End(_) => depth = depth.saturating_sub(1),
            Event::Eof => break,
            _ => {}
        }
        buffer.clear();
    }

    Ok(XmlInspection {
        root_name: root_name.ok_or(XmlError::MissingRoot)?,
        element_count,
        max_depth,
    })
}

pub fn validate_xsd(xml: &[u8], xsd: &[u8]) -> Result<(), XmlError> {
    let _guard = native_engine_guard()?;
    let parser = Parser::default();
    let document = parser.parse_string_with_options(xml, strict_parser_options())?;
    let mut schema_parser = SchemaParserContext::from_buffer(xsd);
    let mut validation = SchemaValidationContext::from_parser(&mut schema_parser)
        .map_err(|errors| XmlError::SchemaCompile(format_errors(&errors)))?;
    validation
        .validate_document(&document)
        .map_err(|errors| XmlError::SchemaValidation(format_errors(&errors)))
}

pub fn transform_xslt(xml: &[u8], stylesheet: &[u8]) -> Result<Vec<u8>, XmlError> {
    let _guard = native_engine_guard()?;
    let parser = Parser::default();
    let document = parser.parse_string_with_options(xml, strict_parser_options())?;
    let mut stylesheet =
        xslt_parser::parse_bytes(stylesheet.to_vec(), "embedded-tidas-stylesheet.xsl")
            .map_err(XmlError::Stylesheet)?;
    let transformed = stylesheet
        .transform(document, Vec::new())
        .map_err(|error| XmlError::Transform(error.to_string()))?;
    Ok(transformed.to_string().into_bytes())
}

fn native_engine_guard() -> Result<MutexGuard<'static, ()>, XmlError> {
    NATIVE_XML_ENGINE
        .lock()
        .map_err(|_| XmlError::EnginePoisoned)
}

fn strict_parser_options() -> ParserOptions<'static> {
    ParserOptions {
        recover: false,
        no_net: true,
        huge: false,
        ..ParserOptions::default()
    }
}

fn format_errors(errors: &[libxml::error::StructuredError]) -> String {
    errors
        .iter()
        .map(|error| {
            error
                .message
                .as_deref()
                .unwrap_or("unknown libxml2 error")
                .trim()
                .to_owned()
        })
        .collect::<Vec<_>>()
        .join("; ")
}

impl From<&StructuredError> for XmlDiagnostic {
    fn from(error: &StructuredError) -> Self {
        let level = match error.level {
            XmlErrorLevel::None => "none",
            XmlErrorLevel::Warning => "warning",
            XmlErrorLevel::Error => "error",
            XmlErrorLevel::Fatal => "fatal",
        };
        Self {
            message: error
                .message
                .as_deref()
                .unwrap_or("unknown libxml2 error")
                .trim()
                .to_owned(),
            level: level.to_owned(),
            filename: error.filename.clone(),
            line: error.line,
            column: error.col,
            domain: error.domain,
            code: error.code,
        }
    }
}

#[derive(Debug, Error)]
pub enum XmlError {
    #[error("XML input has no root element")]
    MissingRoot,
    #[error("streaming XML parse failed: {0}")]
    Streaming(#[from] quick_xml::Error),
    #[error("native XML parse failed: {0}")]
    NativeParse(#[from] libxml::parser::XmlParseError),
    #[error("XSD compilation failed: {0}")]
    SchemaCompile(String),
    #[error("XSD schema path is not valid UTF-8")]
    NonUtf8SchemaPath,
    #[error("XSD validation failed: {0}")]
    SchemaValidation(String),
    #[error("XSLT stylesheet compilation failed: {0}")]
    Stylesheet(String),
    #[error("XSLT transformation failed: {0}")]
    Transform(String),
    #[error("native XML engine lock is poisoned")]
    EnginePoisoned,
}

#[cfg(test)]
mod tests {
    use super::*;

    const XML: &[u8] = include_bytes!("../tests/fixtures/simple.xml");
    const INVALID_XML: &[u8] = include_bytes!("../tests/fixtures/simple-invalid.xml");
    const XSD: &[u8] = include_bytes!("../tests/fixtures/simple.xsd");
    const XSLT: &[u8] = include_bytes!("../tests/fixtures/simple.xsl");

    #[test]
    fn pure_rust_streaming_parser_is_strict_and_bounded() {
        assert_eq!(
            inspect_xml(XML).unwrap(),
            XmlInspection {
                root_name: "package".to_owned(),
                element_count: 3,
                max_depth: 2,
            }
        );
        assert!(inspect_xml(b"<package><item></package>").is_err());
    }

    #[test]
    fn native_xsd_validation_accepts_and_rejects_expected_documents() {
        validate_xsd(XML, XSD).unwrap();
        assert!(matches!(
            validate_xsd(INVALID_XML, XSD),
            Err(XmlError::SchemaValidation(_))
        ));
    }

    #[test]
    fn native_xslt_transform_is_repeatable() {
        let first = transform_xslt(XML, XSLT).unwrap();
        let second = transform_xslt(XML, XSLT).unwrap();
        assert_eq!(first, second);
        let output = String::from_utf8(first).unwrap();
        assert!(output.contains("<count>2</count>"), "{output}");
    }

    #[test]
    fn decision_contract_records_all_three_engine_boundaries() {
        let decision = engine_decision();
        assert!(decision.streaming_parser.starts_with("quick-xml"));
        assert!(decision.xsd_engine.contains("libxml"));
        assert!(decision.xslt_engine.contains("libxslt"));
        assert!(decision.concurrency.contains("serialized"));
    }
}
