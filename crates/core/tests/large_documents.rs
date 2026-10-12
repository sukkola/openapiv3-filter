//! Regression tests for documents whose component reference graph has a huge number of distinct
//! paths (heavily shared, cyclic components, as in real-world specs such as Stripe's).
//!
//! The reference-closure used to enumerate every simple path through the component graph, which
//! is exponential in the depth of shared references: these documents never finished. The time
//! bounds below are deliberately generous (the work takes milliseconds); they exist to catch a
//! return of super-linear behaviour, not to benchmark.

use std::time::{Duration, Instant};

use openapiv3_filter_core::filter::openapi::{FilteringParameters, OpenAPIFilter};
use serde_json::{Value, json};

const DIAMOND_DEPTH: usize = 80;
const TIME_BOUND: Duration = Duration::from_secs(10);

/// Builds a document with two operations and a chain of `DIAMOND_DEPTH` diamonds in
/// `components.schemas`: `N{i}` references `L{i}` and `R{i}`, which both reference `N{i+1}`
/// (2^DIAMOND_DEPTH distinct simple paths from `N0`). Every `N{i}` also references `N0`, making the
/// graph cyclic, and `Unrelated*` schemas are referenced by no operation.
fn shared_cyclic_document() -> Value {
    let mut schemas = serde_json::Map::new();
    for i in 0..DIAMOND_DEPTH {
        let next = format!("#/components/schemas/N{}", i + 1);
        schemas.insert(
            format!("N{i}"),
            json!({ "type": "object", "properties": {
                "left": { "$ref": format!("#/components/schemas/L{i}") },
                "right": { "$ref": format!("#/components/schemas/R{i}") },
                "root": { "$ref": "#/components/schemas/N0" },
            }}),
        );
        schemas.insert(format!("L{i}"), json!({ "allOf": [{ "$ref": next }] }));
        schemas.insert(format!("R{i}"), json!({ "oneOf": [{ "$ref": next }] }));
    }
    schemas.insert(format!("N{DIAMOND_DEPTH}"), json!({ "type": "string" }));
    for i in 0..50 {
        schemas.insert(
            format!("Unrelated{i}"),
            json!({ "type": "object", "properties": {
                "next": { "$ref": format!("#/components/schemas/Unrelated{}", (i + 1) % 50) }
            }}),
        );
    }

    json!({
        "openapi": "3.0.3",
        "info": { "title": "shared", "version": "1.0.0" },
        "paths": {
            "/shared": { "get": { "responses": { "200": { "description": "ok", "content": {
                "application/json": { "schema": { "$ref": "#/components/schemas/N0" } }
            }}}}},
            "/plain": { "get": { "responses": { "200": { "description": "ok" } } } }
        },
        "components": { "schemas": Value::Object(schemas) }
    })
}

fn schema_names(document: &Value) -> Vec<&str> {
    document
        .pointer("/components/schemas")
        .and_then(Value::as_object)
        .map(|schemas| schemas.keys().map(String::as_str).collect())
        .unwrap_or_default()
}

#[test]
fn it_keeps_the_full_closure_of_a_heavily_shared_cyclic_graph_quickly() {
    let document = shared_cyclic_document();

    let started = Instant::now();
    let filtered = document.filter_by_parameters(FilteringParameters {
        paths: Some(vec![String::from("/shared")]),
        ..Default::default()
    });

    assert!(
        started.elapsed() < TIME_BOUND,
        "took {:?}",
        started.elapsed()
    );
    let names = schema_names(&filtered);
    assert_eq!(names.len(), 3 * DIAMOND_DEPTH + 1);
    assert!(names.contains(&"N0") && names.contains(&"L79") && names.contains(&"R79"));
    assert!(names.contains(&format!("N{DIAMOND_DEPTH}").as_str()));
    assert!(names.iter().all(|name| !name.starts_with("Unrelated")));
}

#[test]
fn it_is_fast_when_the_filter_matches_nothing() {
    let document = shared_cyclic_document();

    let started = Instant::now();
    let filtered = document.filter_by_parameters(FilteringParameters {
        paths: Some(vec![String::from("/no-such-path")]),
        ..Default::default()
    });

    assert!(
        started.elapsed() < TIME_BOUND,
        "took {:?}",
        started.elapsed()
    );
    assert!(filtered.get("components").is_none());
}

#[test]
fn it_is_fast_when_no_filter_is_given() {
    let document = shared_cyclic_document();

    let started = Instant::now();
    let filtered = document.filter_by_parameters(FilteringParameters::default());

    assert!(
        started.elapsed() < TIME_BOUND,
        "took {:?}",
        started.elapsed()
    );
    // Every operation is kept, but nothing references the `Unrelated*` schemas.
    assert_eq!(schema_names(&filtered).len(), 3 * DIAMOND_DEPTH + 1);
}
