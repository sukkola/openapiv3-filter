//! The single place where the YAML implementation is configured.
//!
//! Both the parser in this crate and the CLI go through these two functions, so the choice of YAML
//! crate (and the options that give OpenAPI documents the intended meaning) live in one spot.
//!
//! OpenAPI documents are YAML 1.2 / JSON-compatible, so the options below are chosen to read them
//! the way a YAML 1.2 core-schema parser does:
//!
//! * `strict_booleans`: only the literals `true` and `false` are booleans. Without it the YAML 1.1
//!   forms `y`, `n`, `yes`, `no`, `on`, `off` become booleans, which would silently turn a schema
//!   such as `required: [x, y]` into `["x", true]`.
//! * `merge_keys = AsOrdinary`: `<<` is an ordinary key, not an instruction to merge mappings.
//! * the parser budget is raised well above the defaults (which are tuned for small, untrusted
//!   configuration files and reject real-world documents of a few MB such as Stripe's), while the
//!   alias-bomb protections stay at their defaults.

use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_saphyr::{Budget, MergeKeyPolicy, Options};

/// Upper bound on the nesting depth of a document; matches the recursion limit `serde_json` applies
/// to JSON documents.
const MAX_DEPTH: usize = 128;
/// Generous caps sized for multi-megabyte API descriptions.
const MAX_EVENTS: usize = 200_000_000;
const MAX_NODES: usize = 50_000_000;

fn options() -> Options {
    // `Options` and `Budget` are `#[non_exhaustive]`: start from the defaults and assign fields.
    let mut budget = Budget::default();
    budget.max_depth = MAX_DEPTH;
    budget.max_events = MAX_EVENTS;
    budget.max_nodes = MAX_NODES;

    let mut options = Options::default();
    options.strict_booleans = true;
    options.merge_keys = MergeKeyPolicy::AsOrdinary;
    options.budget = Some(budget);
    options
}

/// Deserializes a YAML (or JSON, which is a subset of YAML) document from a string.
pub fn from_str<T: DeserializeOwned>(contents: &str) -> Result<T, serde_saphyr::Error> {
    serde_saphyr::from_str_with_options(contents, options())
}

/// Serializes a value as a YAML document.
pub fn to_string<T: Serialize>(value: &T) -> Result<String, serde_saphyr::SerializeError> {
    serde_saphyr::to_string(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};

    #[test]
    fn it_keeps_yaml_1_1_boolean_words_as_strings() {
        let value: Value = from_str("required: [x, y, n, yes, no, on, off]").unwrap();

        assert_eq!(
            value,
            json!({ "required": ["x", "y", "n", "yes", "no", "on", "off"] })
        );
    }

    #[test]
    fn it_still_reads_true_and_false_as_booleans() {
        let value: Value = from_str("a: true\nb: false").unwrap();

        assert_eq!(value, json!({ "a": true, "b": false }));
    }

    #[test]
    fn it_treats_merge_keys_as_ordinary_keys() {
        let value: Value = from_str("base: &b {x: 1}\nderived:\n  <<: *b\n  y: 2").unwrap();

        assert_eq!(value["derived"], json!({ "<<": { "x": 1 }, "y": 2 }));
    }

    #[test]
    fn it_preserves_mapping_order() {
        let value: Value = from_str("z: 1\na: 2\nm: 3").unwrap();

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
        let value: Value = from_str(r#"{"openapi": "3.0.3", "paths": {}}"#).unwrap();

        assert_eq!(value, json!({ "openapi": "3.0.3", "paths": {} }));
    }

    #[test]
    fn it_rejects_duplicate_keys() {
        assert!(from_str::<Value>("a: 1\na: 2").is_err());
    }

    #[test]
    fn it_quotes_scalars_that_would_otherwise_change_type_when_serialized() {
        let yaml = to_string(&json!({ "version": "1.0.0", "code": "200", "flag": "y" })).unwrap();

        let back: Value = from_str(&yaml).unwrap();
        assert_eq!(
            back,
            json!({ "version": "1.0.0", "code": "200", "flag": "y" })
        );
    }
}
