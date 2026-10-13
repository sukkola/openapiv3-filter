//! The lean YAML layer (`yaml-lean` feature).
//!
//! The default `yaml-saphyr` implementation reads and writes YAML through `serde-saphyr`, whose
//! serde machinery (a string parser *and* a reader parser, snippet-rendering diagnostics, a full
//! serializer) is more than half of a wasm build of this crate. The filter only ever needs "YAML
//! text -> `serde_json::Value`" and "`serde_json::Value` -> YAML text", so this module provides
//! exactly that directly on top of `granit-parser` (the event parser `serde-saphyr` itself uses),
//! with scalar resolution and output styling adapted from `serde-saphyr` so results agree with
//! the default implementation. `tests/yaml_differential.rs` checks that agreement.
//!
//! Known, deliberate differences from the default implementation are listed in `NOTICE.md`.

mod emit;
mod scalar;

use std::collections::HashMap;

use granit_parser::{Event, Options, Parser};

use serde_json::{Map, Value};

pub use super::YamlError;
pub use emit::to_string;

fn err<T>(message: impl Into<String>) -> Result<T, YamlError> {
    Err(YamlError::new(message))
}

/// Nesting depth cap, matching the recursion limit `serde_json` applies to JSON documents.
const MAX_DEPTH: usize = 128;
/// Alias-bomb protection, at `serde-saphyr`'s defaults.
const MAX_ALIASES: usize = 50_000;
const MAX_ANCHORS: usize = 50_000;
const MAX_REPLAYED_NODES: usize = 1_000_000;
/// Once there are this many aliases, they may not outnumber anchors by more than the multiplier.
const ALIAS_RATIO_MIN_ALIASES: usize = 100;
const ALIAS_RATIO_MULTIPLIER: usize = 10;

enum Frame {
    Seq {
        anchor: usize,
        items: Vec<Value>,
        nodes: usize,
    },
    Map {
        anchor: usize,
        entries: Map<String, Value>,
        key: Option<String>,
        nodes: usize,
    },
}

struct Builder {
    stack: Vec<Frame>,
    /// Finished anchored nodes, the number of nodes each contains, and for scalars the text a
    /// mapping key spelled with an alias would take.
    anchors: HashMap<usize, (Value, usize, Option<String>)>,
    root: Option<Value>,
    documents: usize,
    aliases: usize,
    replayed: usize,
}

impl Builder {
    /// Hands a finished node (`nodes` nodes in total) to its parent.
    fn put(&mut self, value: Value, nodes: usize) -> Result<(), YamlError> {
        match self.stack.last_mut() {
            None => self.root = Some(value),
            Some(Frame::Seq {
                items, nodes: n, ..
            }) => {
                items.push(value);
                *n += nodes;
            }
            Some(Frame::Map {
                entries,
                key,
                nodes: n,
                ..
            }) => {
                let Some(key) = key.take() else {
                    return err("mapping value without a key");
                };
                if entries.insert(key.clone(), value).is_some() {
                    return err(format!("duplicate mapping key `{key}`"));
                }
                *n += nodes;
            }
        }
        Ok(())
    }

    fn anchor(
        &mut self,
        id: usize,
        value: &Value,
        nodes: usize,
        key_text: Option<&str>,
    ) -> Result<(), YamlError> {
        if id != 0 {
            if self.anchors.len() >= MAX_ANCHORS {
                return err("too many YAML anchors");
            }
            self.anchors
                .insert(id, (value.clone(), nodes, key_text.map(str::to_owned)));
        }
        Ok(())
    }

    fn push(&mut self, frame: Frame) -> Result<(), YamlError> {
        if self.stack.len() >= MAX_DEPTH {
            return err("YAML nesting is too deep");
        }
        self.stack.push(frame);
        Ok(())
    }
}

fn check_tag(tag: Option<&granit_parser::Tag>, node: scalar::Node) -> Result<(), YamlError> {
    match tag.and_then(scalar::required_node) {
        Some(required) if required != node => err("unsupported tag on this kind of node"),
        _ => Ok(()),
    }
}

/// Parses YAML (or JSON) text into a value tree, preserving mapping key order.
pub fn parse(text: &str) -> Result<Value, YamlError> {
    let mut options = Options::default();
    options.emit_comments = false;
    let mut parser = Parser::new_from_str_with_options(text, options);
    let mut b = Builder {
        stack: Vec::new(),
        anchors: HashMap::new(),
        root: None,
        documents: 0,
        aliases: 0,
        replayed: 0,
    };

    while let Some(event) = parser.next_event() {
        let (event, span) = event.map_err(|e| YamlError::new(e.to_string()))?;
        match event {
            Event::DocumentStart(..) => {
                b.documents += 1;
                if b.documents > 1 {
                    return err("multiple YAML documents are not supported");
                }
            }
            Event::Scalar(text, style, anchor, tag) => {
                // A scalar where a mapping key is expected is the key; its text is the key.
                let at = |m: String| {
                    YamlError::new(format!(
                        "{m} at line {} column {}",
                        span.start.line(),
                        span.start.col() + 1
                    ))
                };
                if matches!(b.stack.last(), Some(Frame::Map { key: None, .. })) {
                    if scalar::is_nullish(&text, style, tag.as_deref()) {
                        return Err(at(
                            "a null mapping key cannot be used as a string key".into()
                        ));
                    }
                    if let Some(Frame::Map { key, .. }) = b.stack.last_mut() {
                        *key = Some(text.to_string());
                    }
                    b.anchor(anchor, &Value::String(text.to_string()), 1, Some(&text))?;
                    continue;
                }
                let value = scalar::resolve(&text, style, tag.as_deref()).map_err(at)?;
                b.anchor(anchor, &value, 1, Some(&text))?;
                b.put(value, 1)?;
            }
            Event::SequenceStart(_, anchor, tag) => {
                check_tag(tag.as_deref(), scalar::Node::Sequence)?;
                b.push(Frame::Seq {
                    anchor,
                    items: Vec::new(),
                    nodes: 1,
                })?;
            }
            Event::MappingStart(_, anchor, tag) => {
                check_tag(tag.as_deref(), scalar::Node::Mapping)?;
                b.push(Frame::Map {
                    anchor,
                    entries: Map::new(),
                    key: None,
                    nodes: 1,
                })?;
            }
            Event::SequenceEnd | Event::MappingEnd => {
                let Some(frame) = b.stack.pop() else {
                    return err("unbalanced YAML collection");
                };
                let (anchor, value, nodes) = match frame {
                    Frame::Seq {
                        anchor,
                        items,
                        nodes,
                    } => (anchor, Value::Array(items), nodes),
                    Frame::Map {
                        anchor,
                        entries,
                        key: None,
                        nodes,
                    } => (anchor, Value::Object(entries), nodes),
                    Frame::Map { .. } => return err("mapping key without a value"),
                };
                b.anchor(anchor, &value, nodes, None)?;
                b.put(value, nodes)?;
            }
            Event::Alias(id) => {
                b.aliases += 1;
                if b.aliases > MAX_ALIASES
                    || (b.aliases >= ALIAS_RATIO_MIN_ALIASES
                        && b.aliases > ALIAS_RATIO_MULTIPLIER * b.anchors.len().max(1))
                {
                    return err("too many YAML aliases");
                }
                let Some((value, nodes, key_text)) = b.anchors.get(&id) else {
                    return err("alias refers to an unknown or still open anchor");
                };
                if matches!(b.stack.last(), Some(Frame::Map { key: None, .. })) {
                    let Some(text) = key_text.clone() else {
                        return err("an alias to a collection cannot be used as a mapping key");
                    };
                    if value.is_null() {
                        return err("a null mapping key cannot be used as a string key");
                    }
                    if let Some(Frame::Map { key, .. }) = b.stack.last_mut() {
                        *key = Some(text);
                    }
                    continue;
                }
                let (value, nodes) = (value.clone(), *nodes);
                b.replayed += nodes;
                if b.replayed > MAX_REPLAYED_NODES {
                    return err("YAML alias expansion is too large");
                }
                b.put(value, nodes)?;
            }
            _ => {}
        }
    }

    Ok(b.root.unwrap_or(Value::Null))
}
