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
        /// Greatest depth (see [`Builder::put`]) of any child put into this frame so far.
        depth: usize,
    },
    Map {
        anchor: usize,
        entries: Map<String, Value>,
        key: Option<String>,
        nodes: usize,
        /// Greatest depth (see [`Builder::put`]) of any child put into this frame so far.
        depth: usize,
    },
}

struct Builder {
    stack: Vec<Frame>,
    /// Finished anchored nodes: the value, the number of nodes it contains, its depth (see
    /// [`Builder::put`]), and for scalars the text a mapping key spelled with an alias would take.
    anchors: HashMap<usize, (Value, usize, usize, Option<String>)>,
    root: Option<Value>,
    documents: usize,
    aliases: usize,
    replayed: usize,
}

impl Builder {
    /// Hands a finished node to its parent: `nodes` nodes in total, nested `depth` containers deep
    /// at most (0 for a scalar; a container is one more than its deepest child, 0 if it has none).
    /// This is the same notion of depth [`Builder::push`] enforces structurally; it is tracked
    /// separately here because a replayed alias can introduce nesting no `push` call sees (the
    /// node was already built once, when its anchor was first defined).
    fn put(&mut self, value: Value, nodes: usize, depth: usize) -> Result<(), YamlError> {
        match self.stack.last_mut() {
            None => self.root = Some(value),
            Some(Frame::Seq {
                items,
                nodes: n,
                depth: d,
                ..
            }) => {
                items.push(value);
                *n += nodes;
                *d = (*d).max(depth);
            }
            Some(Frame::Map {
                entries,
                key,
                nodes: n,
                depth: d,
                ..
            }) => {
                let Some(key) = key.take() else {
                    return err("mapping value without a key");
                };
                if entries.insert(key.clone(), value).is_some() {
                    return err(format!("duplicate mapping key `{key}`"));
                }
                *n += nodes;
                *d = (*d).max(depth);
            }
        }
        Ok(())
    }

    fn anchor(
        &mut self,
        id: usize,
        value: &Value,
        nodes: usize,
        depth: usize,
        key_text: Option<&str>,
    ) -> Result<(), YamlError> {
        if id != 0 {
            if self.anchors.len() >= MAX_ANCHORS {
                return err("too many YAML anchors");
            }
            self.anchors.insert(
                id,
                (value.clone(), nodes, depth, key_text.map(str::to_owned)),
            );
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
                    // A mapping key always ends up as a JSON object's string key. The engine
                    // accepts a tag compatible with that — no tag, the non-specific `!`/`!!`,
                    // `!!str`, or any tag it does not itself recognise, all of which `classify`
                    // maps to `Str` or `Implicit` — but rejects one that asserts a genuinely
                    // different type, regardless of whether the tag and text would even be
                    // compatible on their own (so `!!int notanumber: v` is rejected the same way
                    // `!!int 1: v` is, even though only one of them is an "invalid" integer).
                    if matches!(
                        scalar::classify(tag.as_deref()),
                        scalar::Kind::Null
                            | scalar::Kind::Bool
                            | scalar::Kind::Int
                            | scalar::Kind::Float
                            | scalar::Kind::Unsupported
                    ) {
                        return Err(at("a mapping key cannot have this tag".into()));
                    }
                    if scalar::is_nullish(&text, style, tag.as_deref()) {
                        return Err(at(
                            "a null mapping key cannot be used as a string key".into()
                        ));
                    }
                    if let Some(Frame::Map { key, .. }) = b.stack.last_mut() {
                        *key = Some(text.to_string());
                    }
                    // The anchor's value is the key's resolved scalar (e.g. the number `200` for
                    // `&code 200:`), not its string spelling, so an alias to this anchor used in
                    // a *value* position comes back as that scalar. The only way `resolve` can
                    // still fail here, given the tag was just accepted above, is an untagged,
                    // numeral-shaped but non-finite text like `.inf`: the engine accepts that as
                    // a plain key without ever attempting to resolve it to a number, so this
                    // falls back to the key's own text the same way.
                    let resolved = scalar::resolve(&text, style, tag.as_deref())
                        .unwrap_or_else(|_| Value::String(text.to_string()));
                    b.anchor(anchor, &resolved, 1, 0, Some(&text))?;
                    continue;
                }
                let value = scalar::resolve(&text, style, tag.as_deref()).map_err(at)?;
                b.anchor(anchor, &value, 1, 0, Some(&text))?;
                b.put(value, 1, 0)?;
            }
            Event::SequenceStart(_, anchor, tag) => {
                check_tag(tag.as_deref(), scalar::Node::Sequence)?;
                b.push(Frame::Seq {
                    anchor,
                    items: Vec::new(),
                    nodes: 1,
                    depth: 0,
                })?;
            }
            Event::MappingStart(_, anchor, tag) => {
                check_tag(tag.as_deref(), scalar::Node::Mapping)?;
                b.push(Frame::Map {
                    anchor,
                    entries: Map::new(),
                    key: None,
                    nodes: 1,
                    depth: 0,
                })?;
            }
            Event::SequenceEnd | Event::MappingEnd => {
                let Some(frame) = b.stack.pop() else {
                    return err("unbalanced YAML collection");
                };
                let (anchor, value, nodes, depth) = match frame {
                    Frame::Seq {
                        anchor,
                        items,
                        nodes,
                        depth,
                    } => (anchor, Value::Array(items), nodes, depth + 1),
                    Frame::Map {
                        anchor,
                        entries,
                        key: None,
                        nodes,
                        depth,
                    } => (anchor, Value::Object(entries), nodes, depth + 1),
                    Frame::Map { .. } => return err("mapping key without a value"),
                };
                b.anchor(anchor, &value, nodes, depth, None)?;
                b.put(value, nodes, depth)?;
            }
            Event::Alias(id) => {
                b.aliases += 1;
                if b.aliases > MAX_ALIASES
                    || (b.aliases >= ALIAS_RATIO_MIN_ALIASES
                        && b.aliases > ALIAS_RATIO_MULTIPLIER * b.anchors.len().max(1))
                {
                    return err("too many YAML aliases");
                }
                let Some((value, nodes, depth, key_text)) = b.anchors.get(&id) else {
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
                // The anchor's own structural depth was already bounded by `push` when it was
                // first built; what `push` cannot see is this replay adding it on top of the
                // nesting already open here, which is exactly how a chain of anchors (each
                // wrapping the previous alias) can build a tree far deeper than any single
                // `push` call witnessed.
                if b.stack.len() + depth > MAX_DEPTH {
                    return err("YAML nesting is too deep");
                }
                let (value, nodes, depth) = (value.clone(), *nodes, *depth);
                b.replayed += nodes;
                if b.replayed > MAX_REPLAYED_NODES {
                    return err("YAML alias expansion is too large");
                }
                b.put(value, nodes, depth)?;
            }
            _ => {}
        }
    }

    Ok(b.root.unwrap_or(Value::Null))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `a0: &a0 [0]`, `a1: &a1 [*a0]`, ... each one element deeper than the last via an alias, not
    /// direct nesting: `push` never sees more than two open frames (the root mapping and the
    /// current list) however long the chain, so only the depth check on the alias itself can
    /// catch a chain long enough to build a tree past `MAX_DEPTH`.
    fn alias_chain(n: usize) -> String {
        let mut s = String::from("a0: &a0 [0]\n");
        for i in 1..n {
            s.push_str(&format!("a{i}: &a{i} [*a{}]\n", i - 1));
        }
        s
    }

    #[test]
    fn it_accepts_an_alias_chain_within_the_depth_limit() {
        assert!(parse(&alias_chain(50)).is_ok());
    }

    #[test]
    fn it_rejects_an_alias_chain_that_would_build_a_tree_past_the_depth_limit() {
        let err = parse(&alias_chain(200)).unwrap_err();
        assert!(err.to_string().contains("too deep"), "{err}");
    }
}
