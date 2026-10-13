//! Implicit scalar resolution for plain YAML scalars.
//!
//! Adapted from `serde-saphyr` 1.3.0 (`src/parse_scalars.rs`, `src/de/tags.rs` and the scalar rules of
//! `deserialize_any` in `src/de/deserializer.rs`) by the serde-saphyr authors
//! (<https://github.com/bourumir-wyngs/serde-saphyr>), MIT OR Apache-2.0; see `NOTICE.md`. The
//! rules are the ones `yaml/saphyr.rs` configures (`strict_booleans`, no legacy octals,
//! non-finite floats rejected) so parsing agrees with the default implementation value for value.

use granit_parser::{ScalarStyle, Tag};
use serde_json::{Number, Value};

/// What an explicit YAML tag asks of a scalar.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Kind {
    /// No tag, or a tag that does not influence scalar resolution.
    Implicit,
    Null,
    Bool,
    Int,
    Float,
    /// `!!str` and the non-specific tag `!`: keep the text.
    Str,
    /// Tags whose scalars cannot become a JSON value (`!!binary`, `!!timestamp`, `!!map`, ...).
    Unsupported,
}

const NS: &str = "tag:yaml.org,2002:";

pub(super) fn classify(tag: Option<&Tag>) -> Kind {
    let Some(tag) = tag else {
        return Kind::Implicit;
    };
    let by_name = |name: &str| match name {
        "null" => Some(Kind::Null),
        "bool" => Some(Kind::Bool),
        "int" => Some(Kind::Int),
        "float" => Some(Kind::Float),
        "str" => Some(Kind::Str),
        "map" | "seq" | "timestamp" | "time" | "binary" | "include" | "degrees" | "radians" => {
            Some(Kind::Unsupported)
        }
        _ => None,
    };
    if let Some(kind) = tag.core_suffix().and_then(by_name) {
        return kind;
    }
    if let Some(suffix) = tag.suffix_in_namespace(NS)
        && (suffix == "merge" || suffix == "value")
    {
        return Kind::Implicit;
    }
    let spelled = tag.to_string();
    if spelled == "!" || spelled == "!!" {
        return Kind::Str;
    }
    let name = spelled
        .strip_prefix(NS)
        .or_else(|| spelled.strip_prefix("!!"))
        .or_else(|| spelled.strip_prefix('!'));
    let name = name.map(|n| n.strip_prefix('!').unwrap_or(n));
    match name.and_then(by_name) {
        Some(kind) => kind,
        None if spelled.starts_with("!include") || spelled.starts_with(&format!("{NS}include")) => {
            Kind::Unsupported
        }
        None => Kind::Implicit,
    }
}

/// The node kind a tag is only valid on.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Node {
    Scalar,
    Sequence,
    Mapping,
}

/// Tags that name a YAML type can only decorate that kind of node (`!!str [1]` is an error);
/// local and unknown tags are accepted anywhere and ignored.
pub(super) fn required_node(tag: &Tag) -> Option<Node> {
    let spelled = tag.to_string();
    let name = tag
        .core_suffix()
        .or_else(|| spelled.strip_prefix(NS))
        .or_else(|| spelled.strip_prefix("!!"))
        .or_else(|| spelled.strip_prefix('!'));
    let name = name.map(|n| n.strip_prefix('!').unwrap_or(n))?;
    match name {
        "seq" => Some(Node::Sequence),
        "map" => Some(Node::Mapping),
        "null" | "bool" | "int" | "float" | "str" | "timestamp" | "time" | "binary" | "merge"
        | "value" | "degrees" | "radians" => Some(Node::Scalar),
        _ => None,
    }
}

/// Whether a plain, untagged scalar of this text is YAML null (`~`, `null`, empty).
pub(super) fn is_nullish(text: &str, style: ScalarStyle, tag: Option<&Tag>) -> bool {
    match classify(tag) {
        Kind::Null => true,
        Kind::Implicit => {
            style == ScalarStyle::Plain
                && (text.is_empty() || text == "~" || text.eq_ignore_ascii_case("null"))
        }
        _ => false,
    }
}

pub(super) fn resolve(text: &str, style: ScalarStyle, tag: Option<&Tag>) -> Result<Value, String> {
    let kind = classify(tag);
    match kind {
        Kind::Null => return Ok(Value::Null),
        Kind::Bool => {
            return parse_bool(text)
                .map(Value::Bool)
                .ok_or_else(|| format!("invalid boolean `{text}`"));
        }
        Kind::Int => {
            return if text.trim().starts_with('-') {
                parse_int_signed(text).map(Value::from)
            } else {
                parse_int_unsigned(text).map(Value::from)
            }
            .ok_or_else(|| format!("invalid integer `{text}`"));
        }
        Kind::Float => {
            return match float(text) {
                Float::Finite(f) => Ok(finite(f)),
                Float::NonFinite => Err(non_finite(text)),
                Float::No => Err(format!("invalid float `{text}`")),
            };
        }
        Kind::Str => return Ok(Value::String(text.to_owned())),
        Kind::Unsupported => return Err("unsupported YAML tag on a scalar".to_owned()),
        Kind::Implicit => {}
    }
    if style != ScalarStyle::Plain {
        return Ok(Value::String(text.to_owned()));
    }
    if text.is_empty() || text == "~" || text.eq_ignore_ascii_case("null") {
        return Ok(Value::Null);
    }
    if let Some(b) = parse_bool(text) {
        return Ok(Value::Bool(b));
    }
    let t = text.trim();
    if t.starts_with('-') {
        if !leading_zero_decimal(t)
            && let Some(v) = parse_int_signed(t)
        {
            return Ok(Value::from(v));
        }
    } else if let Some(v) = parse_int_unsigned(t) {
        return Ok(Value::from(v));
    }
    match float(text) {
        Float::Finite(f) => Ok(finite(f)),
        Float::NonFinite => Err(non_finite(text)),
        Float::No => Ok(Value::String(text.to_owned())),
    }
}

fn finite(f: f64) -> Value {
    Value::Number(Number::from_f64(f).expect("finite"))
}

fn non_finite(text: &str) -> String {
    format!("non-finite float `{text}` cannot be represented in a JSON document")
}

/// YAML 1.2 core booleans only: `y`, `yes`, `on`, ... are plain strings.
fn parse_bool(s: &str) -> Option<bool> {
    let t = s.trim();
    if t.eq_ignore_ascii_case("true") {
        Some(true)
    } else if t.eq_ignore_ascii_case("false") {
        Some(false)
    } else {
        None
    }
}

fn digits(digits: &str, radix: u32) -> Option<u128> {
    let bytes = digits.as_bytes();
    let mut val: u128 = 0;
    let mut saw = false;
    for (i, &b) in bytes.iter().enumerate() {
        if b == b'_' {
            let prev_ok = i > 0 && bytes[i - 1] != b'_';
            let next_ok = i + 1 < bytes.len() && bytes[i + 1] != b'_';
            if !prev_ok || !next_ok {
                return None;
            }
            continue;
        }
        let d = (b as char).to_digit(radix)?;
        val = val
            .checked_mul(u128::from(radix))?
            .checked_add(u128::from(d))?;
        saw = true;
    }
    saw.then_some(val)
}

fn radix_and_digits(rest: &str) -> (u32, &str) {
    if let Some(r) = rest.strip_prefix("0x").or_else(|| rest.strip_prefix("0X")) {
        (16, r)
    } else if let Some(r) = rest.strip_prefix("0o").or_else(|| rest.strip_prefix("0O")) {
        (8, r)
    } else if let Some(r) = rest.strip_prefix("0b").or_else(|| rest.strip_prefix("0B")) {
        (2, r)
    } else {
        (10, rest)
    }
}

/// Magnitude of an integer literal; decimal literals with a redundant leading zero are rejected
/// (YAML 1.2).
fn magnitude(rest: &str) -> Option<u128> {
    let (radix, d) = radix_and_digits(rest);
    if radix == 10 && d.starts_with('0') && d != "0" {
        return None;
    }
    digits(d, radix)
}

fn parse_int_signed(s: &str) -> Option<i64> {
    let t = s.trim();
    let (neg, rest) = match t.strip_prefix('+') {
        Some(r) => (false, r),
        None => match t.strip_prefix('-') {
            Some(r) => (true, r),
            None => (false, t),
        },
    };
    let mag = magnitude(rest)?;
    if neg {
        if mag == (i64::MAX as u128) + 1 {
            Some(i64::MIN)
        } else {
            i64::try_from(mag).ok().map(|v| -v)
        }
    } else {
        i64::try_from(mag).ok()
    }
}

fn parse_int_unsigned(s: &str) -> Option<u64> {
    let t = s.trim();
    if t.starts_with('-') {
        return None;
    }
    u64::try_from(magnitude(t.strip_prefix('+').unwrap_or(t))?).ok()
}

/// A decimal with a redundant leading zero such as `0127`, `+0127`, `-0127`; explicit radices
/// (`0x`, `0o`, `0b`) are excluded.
fn leading_zero_decimal(t: &str) -> bool {
    let s = t.trim();
    let digits = s.strip_prefix(['+', '-']).unwrap_or(s);
    match digits.strip_prefix('0') {
        Some(rest) => match rest.chars().next() {
            Some(next) => !matches!(next, 'x' | 'X' | 'o' | 'O' | 'b' | 'B'),
            None => false,
        },
        None => false,
    }
}

enum Float {
    Finite(f64),
    /// `.inf`, `.nan` or a literal that overflows `f64`.
    NonFinite,
    No,
}

fn float(s: &str) -> Float {
    let t = s.trim();
    match t.to_ascii_lowercase().as_str() {
        ".nan" | "+.nan" | "-.nan" | ".inf" | "+.inf" | "-.inf" => return Float::NonFinite,
        _ => {}
    }
    if let Ok(v) = t.parse::<f64>()
        && v.is_finite()
    {
        return Float::Finite(v);
    }
    // A numeral-shaped literal that overflows (`1e999`) is non-finite too; `inf`/`nan`
    // spellings stay plain strings.
    let unsigned = t.strip_prefix(['+', '-']).unwrap_or(t);
    if !unsigned.as_bytes().first().is_some_and(u8::is_ascii_digit) {
        return Float::No;
    }
    match t.parse::<f64>() {
        Ok(v) if v.is_infinite() => Float::NonFinite,
        _ => Float::No,
    }
}
