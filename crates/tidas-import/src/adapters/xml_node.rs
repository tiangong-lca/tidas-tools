use std::collections::BTreeMap;

use quick_xml::XmlVersion;
use quick_xml::events::{BytesStart, Event};
use quick_xml::reader::Reader;
use thiserror::Error;

#[derive(Clone, Debug)]
pub struct XmlNode {
    pub name: String,
    pub attributes: BTreeMap<String, String>,
    pub children: Vec<Self>,
    pub text: String,
}

impl XmlNode {
    pub fn parse(bytes: &[u8]) -> Result<Self, XmlNodeError> {
        let mut reader = Reader::from_reader(bytes);
        reader.config_mut().check_end_names = true;
        reader.config_mut().allow_unmatched_ends = false;
        let mut buffer = Vec::new();
        let mut stack = Vec::new();
        let mut root = None;
        loop {
            match reader.read_event_into(&mut buffer)? {
                Event::Start(element) => stack.push(Self::start(&reader, &element)?),
                Event::Empty(element) => {
                    let node = Self::start(&reader, &element)?;
                    append(&mut stack, &mut root, node)?;
                }
                Event::End(_) => {
                    let node = stack.pop().ok_or(XmlNodeError::UnexpectedEnd)?;
                    append(&mut stack, &mut root, node)?;
                }
                Event::Text(text) => {
                    if let Some(node) = stack.last_mut() {
                        node.text
                            .push_str(&text.xml_content(XmlVersion::Implicit1_0)?);
                    }
                }
                Event::CData(text) => {
                    if let Some(node) = stack.last_mut() {
                        node.text.push_str(&text.decode()?);
                    }
                }
                Event::Eof => break,
                _ => {}
            }
            buffer.clear();
        }
        if !stack.is_empty() {
            return Err(XmlNodeError::UnclosedElement);
        }
        root.ok_or(XmlNodeError::MissingRoot)
    }

    fn start(reader: &Reader<&[u8]>, element: &BytesStart<'_>) -> Result<Self, XmlNodeError> {
        let name = local_name(&reader.decoder().decode(element.name().as_ref())?);
        let mut attributes = BTreeMap::new();
        for attribute in element.attributes() {
            let attribute = attribute?;
            let key = local_name(&reader.decoder().decode(attribute.key.as_ref())?);
            let value = attribute
                .decoded_and_normalized_value(XmlVersion::Implicit1_0, reader.decoder())?
                .into_owned();
            attributes.insert(key, value);
        }
        Ok(Self {
            name,
            attributes,
            children: Vec::new(),
            text: String::new(),
        })
    }

    pub fn attr(&self, name: &str) -> Option<&str> {
        self.attributes.get(name).map(String::as_str)
    }

    pub fn child(&self, name: &str) -> Option<&Self> {
        self.children.iter().find(|child| child.name == name)
    }

    pub fn child_text(&self, name: &str) -> Option<&str> {
        self.child(name).and_then(Self::trimmed_text)
    }

    pub fn trimmed_text(&self) -> Option<&str> {
        let value = self.text.trim();
        (!value.is_empty()).then_some(value)
    }

    pub fn descendants_named(&self, name: &str) -> DescendantsNamed<'_> {
        DescendantsNamed {
            name: name.to_owned(),
            stack: vec![self],
        }
    }

    pub fn first_descendant(&self, name: &str) -> Option<&Self> {
        self.descendants_named(name).next()
    }
}

pub struct DescendantsNamed<'a> {
    name: String,
    stack: Vec<&'a XmlNode>,
}

impl<'a> Iterator for DescendantsNamed<'a> {
    type Item = &'a XmlNode;

    fn next(&mut self) -> Option<Self::Item> {
        while let Some(node) = self.stack.pop() {
            self.stack.extend(node.children.iter().rev());
            if node.name == self.name {
                return Some(node);
            }
        }
        None
    }
}

fn append(
    stack: &mut [XmlNode],
    root: &mut Option<XmlNode>,
    node: XmlNode,
) -> Result<(), XmlNodeError> {
    if let Some(parent) = stack.last_mut() {
        parent.children.push(node);
        return Ok(());
    }
    if root.replace(node).is_some() {
        return Err(XmlNodeError::MultipleRoots);
    }
    Ok(())
}

fn local_name(name: &str) -> String {
    name.rsplit(':').next().unwrap_or(name).to_owned()
}

#[derive(Debug, Error)]
pub enum XmlNodeError {
    #[error("XML document has no root")]
    MissingRoot,
    #[error("XML document has multiple roots")]
    MultipleRoots,
    #[error("XML document has an unexpected closing element")]
    UnexpectedEnd,
    #[error("XML document has an unclosed element")]
    UnclosedElement,
    #[error("XML parsing failed: {0}")]
    Xml(#[from] quick_xml::Error),
    #[error("XML attribute parsing failed: {0}")]
    Attribute(#[from] quick_xml::events::attributes::AttrError),
    #[error("XML decoding failed: {0}")]
    Encoding(#[from] quick_xml::encoding::EncodingError),
}
