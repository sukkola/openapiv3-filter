//! Regression tests for component names that are not plain identifiers: names containing `.`
//! (Stripe's `issuing.authorization`), `/` and `~` (JSON Pointer escapes `~1`/`~0`), and names
//! referenced through percent-encoded `$ref`s.
//!
//! Component paths used to be joined with `.` and split again when selecting what to keep, so a
//! dotted name was treated as nested keys and silently dropped from `components`, leaving the
//! retained `$ref`s dangling.

use openapiv3_filter_core::filter::openapi::{FilteringParameters, OpenAPIFilter};
use openapiv3_filter_core::parser;
use serde_json::Value;

const DOCUMENT: &str = include_str!("resources/special-component-names.json");

fn filter_by_path(path: &str) -> Value {
    let document = parser::parse(DOCUMENT).expect("fixture parses");
    document.filter_by_parameters(FilteringParameters {
        paths: Some(vec![path.to_string()]),
        ..Default::default()
    })
}

fn names(document: &Value, category: &str) -> Vec<String> {
    let mut names: Vec<String> = document
        .pointer(&format!("/components/{category}"))
        .and_then(Value::as_object)
        .map(|map| map.keys().cloned().collect())
        .unwrap_or_default();
    names.sort();
    names
}

/// Resolves a local `$ref` the way RFC 6901 describes: percent-decode, split, unescape.
fn resolves(document: &Value, reference: &str) -> bool {
    let Some(pointer) = reference.strip_prefix("#/") else {
        return true;
    };
    let decoded = pointer
        .replace("%2E", ".")
        .replace("%20", " ")
        .replace("%25", "%");
    let mut current = document;
    for segment in decoded.split('/') {
        let key = segment.replace("~1", "/").replace("~0", "~");
        match current.get(&key) {
            Some(next) => current = next,
            None => return false,
        }
    }
    true
}

fn dangling_refs(document: &Value) -> Vec<String> {
    fn walk(value: &Value, root: &Value, out: &mut Vec<String>) {
        match value {
            Value::Object(map) => {
                if let Some(reference) = map.get("$ref").and_then(Value::as_str)
                    && !resolves(root, reference)
                {
                    out.push(reference.to_string());
                }
                map.values().for_each(|child| walk(child, root, out));
            }
            Value::Array(items) => items.iter().for_each(|child| walk(child, root, out)),
            _ => {}
        }
    }
    let mut out = Vec::new();
    walk(document, document, &mut out);
    out
}

#[test]
fn it_keeps_dotted_slash_and_tilde_component_names_with_no_dangling_refs() {
    let filtered = filter_by_path("/auth");

    assert_eq!(
        names(&filtered, "schemas"),
        vec![
            "Foo Bar",
            "a/b",
            "billing.feedback_option",
            "issuing.authorization",
            "til~de",
        ]
    );
    assert_eq!(names(&filtered, "responses"), vec!["not.found"]);
    assert_eq!(dangling_refs(&filtered), Vec::<String>::new());
}

#[test]
fn it_drops_special_named_components_that_nothing_references() {
    let filtered = filter_by_path("/auth");

    let schemas = names(&filtered, "schemas");
    assert!(!schemas.contains(&"Unused.dotted".to_string()));
    assert!(!schemas.contains(&"x/unused".to_string()));
    assert!(!schemas.contains(&"Plain".to_string()));
    assert!(!names(&filtered, "responses").contains(&"unused.response".to_string()));
}

#[test]
fn it_resolves_percent_encoded_refs_to_the_component_they_name() {
    let filtered = filter_by_path("/encoded");

    // The `$ref` text is preserved verbatim; the component it names (and the closure behind it)
    // is kept.
    assert_eq!(
        filtered
            .pointer("/paths/~1encoded/get/responses/200/content/application~1json/schema/$ref"),
        Some(&Value::String(
            "#/components/schemas/issuing%2Eauthorization".to_string()
        ))
    );
    assert!(names(&filtered, "schemas").contains(&"issuing.authorization".to_string()));
    assert!(names(&filtered, "schemas").contains(&"til~de".to_string()));
    assert_eq!(dangling_refs(&filtered), Vec::<String>::new());
}

#[test]
fn it_does_not_pull_in_special_named_components_for_unrelated_paths() {
    let filtered = filter_by_path("/other");

    assert_eq!(names(&filtered, "schemas"), vec!["Plain"]);
    assert!(filtered.pointer("/components/responses").is_none());
}
