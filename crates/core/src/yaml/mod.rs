//! YAML reading and writing.
//!
//! Two interchangeable implementations sit behind Cargo features:
//!
//! | feature                | implementation                                             | notes |
//! |------------------------|------------------------------------------------------------|-------|
//! | `yaml-saphyr` (default) | [`serde-saphyr`](https://crates.io/crates/serde-saphyr)   | full serde integration, rendered syntax-error snippets |
//! | `yaml-lean`            | [`lean`] (in this crate, on top of `granit-parser`)         | `Value` only, roughly half the wasm size |
//!
//! At least one of the two must be enabled. Cargo features are additive, so when both are enabled
//! (the default features plus `yaml-lean`) the lean implementation is the one used by
//! [`parse_value`] and [`value_to_string`]; the generic [`from_str`] / [`to_string`] only exist
//! with `yaml-saphyr`. To get a build that does not link `serde-saphyr` at all, depend on this
//! crate with `default-features = false, features = ["yaml-lean"]`.
//!
//! The lean implementation reads and writes the same documents as the default one; its
//! deliberate differences are listed in `src/yaml/lean/NOTICE.md` and pinned by tests:
//!
//! * syntax errors carry the parser's own message and position ([`Error::MalformedYaml`]
//!   instead of a JSON error),
//! * `!!binary` scalars are rejected,
//! * a scalar document followed by something that is not a document is a syntax error,
//! * alias limits are approximated rather than counted event by event.
//!
//! [`Error::MalformedYaml`]: crate::Error::MalformedYaml

use std::fmt;

use serde_json::Value;

#[cfg(not(any(feature = "yaml-saphyr", feature = "yaml-lean")))]
compile_error!(
    "openapiv3-filter-core needs a YAML implementation: enable `yaml-saphyr` or `yaml-lean`"
);

#[cfg(feature = "yaml-lean")]
pub mod lean;
#[cfg(feature = "yaml-saphyr")]
mod saphyr;

#[cfg(feature = "yaml-saphyr")]
pub use saphyr::{from_str, to_string};

/// A YAML syntax or resolution error, carrying the underlying implementation's message.
#[derive(Debug)]
pub struct YamlError(String);

impl YamlError {
    pub(crate) fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

impl fmt::Display for YamlError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for YamlError {}

/// Parses YAML (or JSON) text into a value tree, preserving mapping key order, with whichever
/// implementation is selected (see the module documentation).
pub fn parse_value(text: &str) -> Result<Value, YamlError> {
    #[cfg(feature = "yaml-lean")]
    {
        lean::parse(text)
    }
    #[cfg(all(feature = "yaml-saphyr", not(feature = "yaml-lean")))]
    {
        saphyr::from_str::<Value>(text).map_err(|e| YamlError::new(e.to_string()))
    }
}

/// Serializes a value tree as a YAML document, with whichever implementation is selected.
pub fn value_to_string(value: &Value) -> Result<String, YamlError> {
    #[cfg(feature = "yaml-lean")]
    {
        lean::to_string(value)
    }
    #[cfg(all(feature = "yaml-saphyr", not(feature = "yaml-lean")))]
    {
        saphyr::to_string(value).map_err(|e| YamlError::new(e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    //! Behaviour every implementation must share; runs under whichever feature is selected.
    use super::*;
    use serde_json::json;

    #[test]
    fn it_keeps_yaml_1_1_boolean_words_as_strings() {
        let value = parse_value("required: [x, y, n, yes, no, on, off]").unwrap();

        assert_eq!(
            value,
            json!({ "required": ["x", "y", "n", "yes", "no", "on", "off"] })
        );
    }

    #[test]
    fn it_reads_true_and_false_as_booleans() {
        assert_eq!(
            parse_value("a: true\nb: false").unwrap(),
            json!({ "a": true, "b": false })
        );
    }

    #[test]
    fn it_treats_merge_keys_as_ordinary_keys() {
        let value = parse_value("base: &b {x: 1}\nderived:\n  <<: *b\n  y: 2").unwrap();

        assert_eq!(value["derived"], json!({ "<<": { "x": 1 }, "y": 2 }));
    }

    #[test]
    fn it_preserves_mapping_order() {
        let value = parse_value("z: 1\na: 2\nm: 3").unwrap();

        let keys: Vec<&str> = value
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(keys, ["z", "a", "m"]);
    }

    #[test]
    fn it_reads_json_documents() {
        assert_eq!(
            parse_value(r#"{"openapi": "3.0.3", "paths": {}}"#).unwrap(),
            json!({ "openapi": "3.0.3", "paths": {} })
        );
    }

    #[test]
    fn it_rejects_duplicate_keys() {
        assert!(parse_value("a: 1\na: 2").is_err());
    }

    #[test]
    fn it_rejects_syntax_errors() {
        assert!(parse_value("a: [1, 2").is_err());
        assert!(parse_value("a: b: c").is_err());
    }

    #[test]
    fn it_resolves_an_anchored_mapping_key_to_its_type_when_aliased_as_a_value() {
        let value = parse_value("openapi: 3.0.3\n&code 200: ok\nexample: *code\n").unwrap();

        assert_eq!(value["example"], json!(200));
    }

    #[test]
    fn it_rejects_a_mapping_key_tagged_as_a_non_string_type() {
        // A mapping key always ends up as a JSON object's string key; both implementations
        // reject a tag that asserts some other type, regardless of whether the tag and text
        // would even be compatible on their own.
        assert!(parse_value("!!int notanumber: v").is_err());
        assert!(parse_value("!!int 1: v").is_err());
        assert!(parse_value("!!bool notabool: v").is_err());
        assert!(parse_value("!!float .inf: v").is_err());
    }

    #[test]
    fn it_rejects_an_alias_to_a_non_string_tagged_scalar_used_as_a_mapping_key() {
        // The same rule as above, but reached through an alias to an anchor defined in a value
        // position rather than a tagged scalar written directly as the key.
        assert!(parse_value("a: &n !!int 42\nb:\n  ? *n\n  : v\n").is_err());
    }

    #[test]
    fn it_accepts_a_mapping_key_whose_tag_is_compatible_with_a_string() {
        // Unlike an int/bool/float/null tag, these are all fine as a plain key: no tag, the
        // non-specific `!`/`!!`, an explicit `!!str`, and an untagged non-finite-looking text
        // (which, lacking a tag, is never even asked to resolve to a number).
        assert_eq!(parse_value("a: v").unwrap()["a"], json!("v"));
        assert_eq!(parse_value("! a: v").unwrap()["a"], json!("v"));
        assert_eq!(parse_value("!!str a: v").unwrap()["a"], json!("v"));
        assert_eq!(parse_value(".inf: v").unwrap()[".inf"], json!("v"));
    }

    #[test]
    fn it_round_trips_scalars_that_would_otherwise_change_type() {
        let value = json!({ "version": "1.0.0", "code": "200", "flag": "y", "n": 1.5, "z": null });

        let yaml = value_to_string(&value).unwrap();

        assert_eq!(parse_value(&yaml).unwrap(), value);
    }
}
