use std::collections::BTreeMap;

use quick_xml::XmlVersion;
use quick_xml::escape::{EscapeError, resolve_predefined_entity};
use quick_xml::events::{BytesDecl, BytesEnd, BytesStart, BytesText, Event};
use quick_xml::reader::Reader;
use quick_xml::writer::Writer;
use serde_json::{Map, Value};
use tidas_runtime::CancellationToken;

use crate::ConversionError;

pub fn json_to_xml(
    bytes: &[u8],
    cancellation: &CancellationToken,
) -> Result<Vec<u8>, ConversionError> {
    cancellation.check()?;
    let document: Value = serde_json::from_slice(bytes)?;
    json_value_to_xml(&document, cancellation)
}

pub(crate) fn json_value_to_xml(
    document: &Value,
    cancellation: &CancellationToken,
) -> Result<Vec<u8>, ConversionError> {
    let object = document
        .as_object()
        .ok_or(ConversionError::JsonRootNotObject)?;
    if object.len() != 1 {
        return Err(ConversionError::JsonRootCount(object.len()));
    }
    let (root, value) = object
        .iter()
        .next()
        .expect("a one-member object has a root element");
    let mut writer = Writer::new_with_indent(Vec::new(), b'\t', 1);
    writer.write_event(Event::Decl(BytesDecl::new("1.0", Some("utf-8"), None)))?;
    write_element(&mut writer, root, value, cancellation)?;
    Ok(writer.into_inner())
}

fn write_element(
    writer: &mut Writer<Vec<u8>>,
    name: &str,
    value: &Value,
    cancellation: &CancellationToken,
) -> Result<(), ConversionError> {
    cancellation.check()?;
    validate_xml_name(name)?;
    if let Value::Array(items) = value {
        for item in items {
            write_element(writer, name, item, cancellation)?;
        }
        return Ok(());
    }

    let mut start = BytesStart::new(name);
    if let Value::Object(object) = value {
        for (key, attribute_value) in object {
            if let Some(attribute_name) = key.strip_prefix('@') {
                validate_xml_name(attribute_name)?;
                let text = scalar_text(attribute_value)?;
                validate_xml_text(&text)?;
                start.push_attribute((attribute_name, text.as_str()));
            }
        }
    }
    writer.write_event(Event::Start(start))?;
    match value {
        Value::Object(object) => {
            for (key, child) in object {
                if key.starts_with('@') || key == "#text" {
                    continue;
                }
                write_element(writer, key, child, cancellation)?;
            }
            if let Some(text) = object.get("#text") {
                let text = scalar_text(text)?;
                validate_xml_text(&text)?;
                writer.write_event(Event::Text(BytesText::new(&text)))?;
            }
        }
        Value::Null => {}
        scalar => {
            let text = scalar_text(scalar)?;
            validate_xml_text(&text)?;
            writer.write_event(Event::Text(BytesText::new(&text)))?;
        }
    }
    writer.write_event(Event::End(BytesEnd::new(name)))?;
    Ok(())
}

pub fn xml_to_json(
    bytes: &[u8],
    cancellation: &CancellationToken,
) -> Result<Vec<u8>, ConversionError> {
    cancellation.check()?;
    let mut reader = Reader::from_reader(bytes);
    reader.config_mut().check_comments = true;
    reader.config_mut().check_end_names = true;
    reader.config_mut().allow_unmatched_ends = false;
    let mut buffer = Vec::new();
    let mut stack = Vec::<Frame>::new();
    let mut root: Option<(String, Value)> = None;
    loop {
        cancellation.check()?;
        let event = reader.read_event_into(&mut buffer)?;
        match event {
            Event::Start(element) => {
                stack.push(Frame::new(&reader, &element)?);
            }
            Event::Empty(element) => {
                let frame = Frame::new(&reader, &element)?;
                let (name, value) = frame.finish();
                append_value(&mut stack, &mut root, name, value)?;
            }
            Event::Text(text) => {
                let decoded = text.xml_content(XmlVersion::Implicit1_0)?;
                validate_xml_text(&decoded)?;
                if let Some(frame) = stack.last_mut() {
                    frame.push_text(&decoded);
                } else if !decoded.trim().is_empty() {
                    return Err(ConversionError::TextOutsideRoot);
                }
            }
            Event::CData(text) => {
                if let Some(frame) = stack.last_mut() {
                    let decoded = text.decode()?;
                    validate_xml_text(&decoded)?;
                    frame.push_text(&decoded);
                }
            }
            Event::GeneralRef(reference) => {
                let resolved = if let Some(character) = reference.resolve_char_ref()? {
                    character.to_string()
                } else {
                    let name = reference.decode()?;
                    resolve_predefined_entity(&name)
                        .ok_or_else(|| {
                            EscapeError::UnrecognizedEntity(0..reference.len(), name.into_owned())
                        })?
                        .to_owned()
                };
                validate_xml_text(&resolved)?;
                if let Some(frame) = stack.last_mut() {
                    frame.push_text(&resolved);
                } else if !resolved.trim().is_empty() {
                    return Err(ConversionError::TextOutsideRoot);
                }
            }
            Event::End(_) => {
                let frame = stack.pop().ok_or(ConversionError::UnmatchedEnd)?;
                let (name, value) = frame.finish();
                append_value(&mut stack, &mut root, name, value)?;
            }
            Event::DocType(_) => return Err(ConversionError::DoctypeForbidden),
            Event::Eof => break,
            _ => {}
        }
        buffer.clear();
    }
    if !stack.is_empty() {
        return Err(ConversionError::UnclosedElements);
    }
    let (name, value) = root.ok_or(ConversionError::MissingRoot)?;
    let mut output = Map::new();
    output.insert(name, value);
    let mut bytes = serde_json::to_vec_pretty(&Value::Object(output))?;
    bytes.push(b'\n');
    Ok(bytes)
}

struct Frame {
    name: String,
    attributes: BTreeMap<String, Value>,
    children: Vec<(String, Value)>,
    text: String,
}

impl Frame {
    fn new(reader: &Reader<&[u8]>, element: &BytesStart<'_>) -> Result<Self, ConversionError> {
        let name = reader
            .decoder()
            .decode(element.name().as_ref())?
            .into_owned();
        validate_xml_name(&name)?;
        let mut attributes = BTreeMap::new();
        for attribute in element.attributes() {
            let attribute = attribute?;
            let key = reader
                .decoder()
                .decode(attribute.key.as_ref())?
                .into_owned();
            validate_xml_name(&key)?;
            let value = attribute
                .decoded_and_normalized_value(XmlVersion::Implicit1_0, reader.decoder())?
                .into_owned();
            validate_xml_text(&value)?;
            attributes.insert(format!("@{key}"), Value::String(value));
        }
        Ok(Self {
            name,
            attributes,
            children: Vec::new(),
            text: String::new(),
        })
    }

    fn push_text(&mut self, text: &str) {
        if !text.is_empty() {
            self.text.push_str(text);
        }
    }

    fn finish(self) -> (String, Value) {
        let Self {
            name,
            attributes,
            children,
            text,
        } = self;
        let text = text.trim().to_owned();
        if attributes.is_empty() && children.is_empty() {
            let value = if text.is_empty() {
                Value::Null
            } else {
                Value::String(text)
            };
            return (name, value);
        }
        let mut object: Map<String, Value> = attributes.into_iter().collect();
        for (child_name, value) in children {
            match object.remove(&child_name) {
                None => {
                    object.insert(child_name, value);
                }
                Some(Value::Array(mut values)) => {
                    values.push(value);
                    object.insert(child_name, Value::Array(values));
                }
                Some(previous) => {
                    object.insert(child_name, Value::Array(vec![previous, value]));
                }
            }
        }
        if !text.is_empty() {
            object.insert("#text".to_owned(), Value::String(text));
        }
        (name, Value::Object(object))
    }
}

fn append_value(
    stack: &mut [Frame],
    root: &mut Option<(String, Value)>,
    name: String,
    value: Value,
) -> Result<(), ConversionError> {
    if let Some(parent) = stack.last_mut() {
        parent.children.push((name, value));
    } else if root.replace((name, value)).is_some() {
        return Err(ConversionError::MultipleRoots);
    }
    Ok(())
}

fn scalar_text(value: &Value) -> Result<String, ConversionError> {
    match value {
        Value::Null => Ok(String::new()),
        Value::Bool(value) => Ok(value.to_string()),
        Value::Number(value) => Ok(value.to_string()),
        Value::String(value) => Ok(value.clone()),
        Value::Array(_) | Value::Object(_) => Err(ConversionError::NonScalarText),
    }
}

fn validate_xml_name(name: &str) -> Result<(), ConversionError> {
    let mut parts = name.split(':');
    let first = parts.next().unwrap_or_default();
    let second = parts.next();
    if parts.next().is_some()
        || !valid_name_part(first)
        || second.is_some_and(|part| !valid_name_part(part))
    {
        return Err(ConversionError::InvalidXmlName(name.to_owned()));
    }
    Ok(())
}

fn valid_name_part(part: &str) -> bool {
    let mut chars = part.chars();
    matches!(chars.next(), Some(first) if first == '_' || first.is_alphabetic())
        && chars.all(|character| {
            character == '_' || character == '-' || character == '.' || character.is_alphanumeric()
        })
}

fn validate_xml_text(text: &str) -> Result<(), ConversionError> {
    if let Some(character) = text.chars().find(|character| {
        !matches!(
            u32::from(*character),
            0x9 | 0xA | 0xD | 0x20..=0xD7FF | 0xE000..=0xFFFD | 0x1_0000..=0x0010_FFFF
        )
    }) {
        return Err(ConversionError::InvalidXmlCharacter(u32::from(character)));
    }
    Ok(())
}
