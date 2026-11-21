//! Generators shared by the fuzz targets: `serde_json::Value` trees drawn from a small vocabulary
//! of OpenAPI keys, `$ref`s and YAML-sensitive strings, so the fuzzer reaches the filter and the
//! YAML quoting rules instead of spending its time on random bytes.

use arbitrary::{Result, Unstructured};
use serde_json::{Map, Number, Value};

const KEYS: &[&str] = &[
    "openapi", "info", "paths", "webhooks", "components", "tags", "security", "securitySchemes",
    "schemas", "responses", "parameters", "pathItems", "requestBody", "content", "schema",
    "properties", "items", "allOf", "oneOf", "$ref", "name", "description", "x-ext", "get", "put",
    "post", "delete", "options", "head", "patch", "trace", "/a", "/a/{id}", "/b", "hook", "A",
    "B", "a.b", "a/b", "a~b", "pct name", "", " ", "-", "?", "#", "0", "200", "y", "no",
];

const STRINGS: &[&str] = &[
    "3.0.3", "3.1.0", "2.0", "a", "b", "A", "get", "GET", "x", "", " ", "  lead", "trail ", "-",
    "- x", "? x", ": x", "a: b", "a #b", "#c", "'q'", "\"dq\"", "\\", "\n", "a\nb", "a\n", "\t",
    "y", "n", "yes", "No", "on", "OFF", "true", "False", "null", "Null", "~", "0", "-0", "0.0",
    "1e3", "0x1F", "0o7", "0b1", "1_000", ".5", "+5", ".inf", "-.INF", ".nan", "NaN", "12:30",
    "2024-01-01", "@x", "`x", "%x", "!x", "&x", "*x", "|", ">", "{", "}", "[", "]", ",", "é",
    "\u{85}", "\u{2028}", "\u{feff}", "\u{1}", "\u{7f}", "😀", "a\u{0}b",
    "#/components/schemas/A", "#/components/schemas/B", "#/components/schemas/a.b",
    "#/components/schemas/a~1b", "#/components/schemas/pct%20name", "#/components/responses/A",
    "#/components/parameters/A", "#/components/pathItems/A", "#/components/securitySchemes/A",
    "#/components/schemas/A/properties/a", "#/components", "#/paths/~1a", "#/missing", "#",
];

fn string(u: &mut Unstructured) -> Result<String> {
    Ok(match u.int_in_range(0..=3)? {
        0..=2 => (*u.choose(STRINGS)?).to_string(),
        _ => {
            // Concatenate two pieces to get combinations such as "a\n#b" or "-0.5x".
            let a = *u.choose(STRINGS)?;
            let b = *u.choose(STRINGS)?;
            format!("{a}{b}")
        }
    })
}

fn key(u: &mut Unstructured) -> Result<String> {
    Ok(if u.ratio(1, 8)? { string(u)? } else { (*u.choose(KEYS)?).to_string() })
}

fn number(u: &mut Unstructured) -> Result<Number> {
    Ok(match u.int_in_range(0..=4)? {
        0 => Number::from(u.int_in_range(-3i64..=300)?),
        1 => Number::from(u.arbitrary::<i64>()?),
        2 => Number::from(u.arbitrary::<u64>()?),
        _ => {
            let f: f64 = u.arbitrary()?;
            Number::from_f64(f).unwrap_or_else(|| Number::from(0))
        }
    })
}

/// An arbitrary JSON value, at most `depth` levels deep.
pub fn value(u: &mut Unstructured, depth: u32) -> Result<Value> {
    let leaf = depth == 0 || u.ratio(1, 3)?;
    Ok(match if leaf { u.int_in_range(0..=3)? } else { u.int_in_range(0..=5)? } {
        0 => Value::Null,
        1 => Value::Bool(u.arbitrary()?),
        2 => Value::Number(number(u)?),
        3 => Value::String(string(u)?),
        4 => {
            let n = u.int_in_range(0..=4)?;
            Value::Array((0..n).map(|_| value(u, depth - 1)).collect::<Result<_>>()?)
        }
        _ => {
            let n = u.int_in_range(0..=5)?;
            let mut map = Map::new();
            for _ in 0..n {
                let k = key(u)?;
                let v = if k == "$ref" { Value::String(string(u)?) } else { value(u, depth - 1)? };
                map.insert(k, v);
            }
            Value::Object(map)
        }
    })
}

/// A list of filter patterns, or `None` (no restriction on that dimension).
pub fn patterns(u: &mut Unstructured) -> Result<Option<Vec<String>>> {
    const PATTERNS: &[&str] = &[
        "*", "/a", "/a*", "/a/*", "/a/{id}", "/b", "*{*}*", "hook", "h*", "get", "GET", "post",
        "A", "a", "B", "", "?", "[", "**",
    ];
    if u.ratio(1, 2)? {
        return Ok(None);
    }
    let n = u.int_in_range(0..=3)?;
    Ok(Some((0..n).map(|_| u.choose(PATTERNS).map(|p| (*p).to_string())).collect::<Result<_>>()?))
}
