//! Property tests for the filtering contract, on generated OpenAPI-shaped documents.
//!
//! Documents are drawn from a fixed universe of component names, so every `$ref` in the input
//! resolves (cycles between schemas included). For any such document and any parameters:
//!
//! * reference closure: every `$ref` in the output resolves inside the output, and every security
//!   scheme a kept security requirement names is still declared;
//! * selection: the kept paths/webhooks and their operations are exactly the ones the parameters
//!   select, in input order;
//! * idempotence: filtering the output again with the same parameters changes nothing;
//! * `into_filtered` and `filter_by_parameters` agree, key order included.
//!
//! The YAML round trip goes through the public `yaml::parse_value` / `yaml::value_to_string`, so it
//! covers whichever implementation the build selects.
//!
//! `PROPTEST_CASES` raises the number of cases (default 256) for a longer local run.

use openapiv3_filter_core::filter::openapi::{FilteringParameters, OpenAPIFilter};
use openapiv3_filter_core::{parser, yaml};
use proptest::prelude::*;
use proptest::sample::subsequence;
use serde_json::{Map, Number, Value, json};

const SCHEMAS: &[&str] = &["A", "B", "C", "a/b"];
const RESPONSES: &[&str] = &["Ok", "NotFound"];
const PARAMETERS: &[&str] = &["Id", "Limit"];
const SCHEMES: &[&str] = &["key", "oauth", "basic"];
const TAGS: &[&str] = &["pets", "store", "admin"];
const PATHS: &[&str] = &["/a", "/a/{id}", "/b", "/c/d", "/e"];
const WEBHOOKS: &[&str] = &["hook", "other"];
const METHODS: &[&str] = &["get", "put", "post", "delete", "patch"];

/// `#/components/<category>/<name>`, with `name` escaped as a JSON Pointer token.
fn component_ref(category: &str, name: &str) -> Value {
    let name = name.replace('~', "~0").replace('/', "~1");
    json!({ "$ref": format!("#/components/{category}/{name}") })
}

fn object(entries: impl IntoIterator<Item = (String, Value)>) -> Value {
    Value::Object(entries.into_iter().collect())
}

fn schema() -> impl Strategy<Value = Value> {
    let leaf = prop_oneof![
        Just(json!({ "type": "string" })),
        Just(json!({ "type": "integer", "format": "int64" })),
        prop::sample::select(SCHEMAS).prop_map(|name| component_ref("schemas", name)),
    ];
    leaf.prop_recursive(3, 12, 3, |inner| {
        prop_oneof![
            prop::collection::vec(inner.clone(), 1..3).prop_map(|props| {
                let props = props
                    .into_iter()
                    .enumerate()
                    .map(|(i, s)| (format!("p{i}"), s));
                json!({ "type": "object", "properties": object(props) })
            }),
            inner
                .clone()
                .prop_map(|items| json!({ "type": "array", "items": items })),
            prop::collection::vec(inner, 1..3).prop_map(|all| json!({ "allOf": all })),
        ]
    })
}

/// A security requirement array: each requirement names a subset of the schemes (an empty object
/// is the "anonymous" alternative).
fn security() -> impl Strategy<Value = Value> {
    prop::collection::vec(subsequence(SCHEMES, 0..=2), 1..3).prop_map(|requirements| {
        Value::Array(
            requirements
                .into_iter()
                .map(|names| object(names.into_iter().map(|n| (n.to_string(), json!([])))))
                .collect(),
        )
    })
}

fn operation() -> impl Strategy<Value = Value> {
    let tag = prop_oneof![4 => prop::sample::select(TAGS), 1 => Just("ghost")];
    let parameter = prop_oneof![
        prop::sample::select(PARAMETERS).prop_map(|name| component_ref("parameters", name)),
        schema().prop_map(|s| json!({ "name": "q", "in": "query", "schema": s })),
    ];
    let response = prop_oneof![
        prop::sample::select(RESPONSES).prop_map(|name| component_ref("responses", name)),
        schema().prop_map(|s| {
            json!({ "description": "inline", "content": { "application/json": { "schema": s } } })
        }),
    ];
    (
        prop::option::of(prop::collection::vec(tag, 0..3)),
        prop::option::of(security()),
        prop::option::of(prop::collection::vec(parameter, 1..3)),
        response,
        prop::option::of(schema()),
    )
        .prop_map(|(tags, security, parameters, response, body)| {
            let mut op = Map::new();
            if let Some(tags) = tags {
                op.insert("tags".into(), json!(tags));
            }
            if let Some(security) = security {
                op.insert("security".into(), security);
            }
            if let Some(parameters) = parameters {
                op.insert("parameters".into(), Value::Array(parameters));
            }
            if let Some(body) = body {
                op.insert(
                    "requestBody".into(),
                    json!({ "content": { "application/json": { "schema": body } } }),
                );
            }
            op.insert("responses".into(), json!({ "200": response }));
            Value::Object(op)
        })
}

/// A Path Item without a `$ref`: some operations, and possibly path-level parameters.
fn inline_path_item() -> impl Strategy<Value = Value> {
    (
        subsequence(METHODS, 0..=3),
        prop::collection::vec(operation(), 3),
        any::<bool>(),
    )
        .prop_map(|(methods, operations, shared_parameter)| {
            let mut item = Map::new();
            item.insert("summary".into(), json!("item"));
            if shared_parameter {
                item.insert(
                    "parameters".into(),
                    json!([component_ref("parameters", "Id")]),
                );
            }
            for (method, operation) in methods.into_iter().zip(operations) {
                item.insert(method.to_string(), operation);
            }
            Value::Object(item)
        })
}

fn path_item() -> impl Strategy<Value = Value> {
    prop_oneof![
        5 => inline_path_item(),
        1 => Just(component_ref("pathItems", "Shared")),
    ]
}

/// A `paths` or `webhooks` container over `names`, possibly with a vendor extension in between.
fn container(names: &'static [&'static str]) -> impl Strategy<Value = Value> {
    (
        subsequence(names, 0..=names.len()),
        prop::collection::vec(path_item(), names.len()),
        prop::option::of(0..=names.len()),
    )
        .prop_map(|(names, items, extension_at)| {
            let mut entries: Vec<(String, Value)> =
                names.into_iter().map(String::from).zip(items).collect();
            if let Some(at) = extension_at {
                let at = at.min(entries.len());
                entries.insert(at, ("x-extension".into(), json!({ "kept": true })));
            }
            object(entries)
        })
}

fn components() -> impl Strategy<Value = Value> {
    (
        prop::collection::vec(schema(), SCHEMAS.len()),
        prop::collection::vec(schema(), RESPONSES.len()),
        prop::collection::vec(schema(), PARAMETERS.len()),
        inline_path_item(),
    )
        .prop_map(|(schemas, responses, parameters, shared)| {
            let named = |names: &[&str], values: Vec<Value>, wrap: &dyn Fn(Value) -> Value| {
                object(
                    names
                        .iter()
                        .map(|n| n.to_string())
                        .zip(values.into_iter().map(wrap)),
                )
            };
            json!({
                "schemas": named(SCHEMAS, schemas, &|s| s),
                "responses": named(RESPONSES, responses, &|s| json!({
                    "description": "r",
                    "content": { "application/json": { "schema": s } },
                })),
                "parameters": named(PARAMETERS, parameters, &|s| json!({
                    "name": "p", "in": "query", "schema": s,
                })),
                "securitySchemes": object(SCHEMES.iter().map(|n| {
                    (n.to_string(), json!({ "type": "apiKey", "name": n, "in": "header" }))
                })),
                "pathItems": { "Shared": shared },
            })
        })
}

fn document() -> impl Strategy<Value = Value> {
    (
        container(PATHS),
        prop::option::of(container(WEBHOOKS)),
        components(),
        any::<bool>(),
        prop::option::of(security()),
    )
        .prop_map(|(paths, webhooks, components, with_tags, root_security)| {
            let mut doc = Map::new();
            doc.insert("openapi".into(), json!("3.1.0"));
            doc.insert(
                "info".into(),
                json!({ "title": "generated", "version": "1" }),
            );
            if with_tags {
                doc.insert(
                    "tags".into(),
                    Value::Array(TAGS.iter().map(|t| json!({ "name": t })).collect()),
                );
            }
            doc.insert("paths".into(), paths);
            if let Some(webhooks) = webhooks {
                doc.insert("webhooks".into(), webhooks);
            }
            doc.insert("components".into(), components);
            if let Some(security) = root_security {
                doc.insert("security".into(), security);
            }
            doc.insert("x-tail".into(), json!(1));
            Value::Object(doc)
        })
}

/// Raw parameter lists, kept so they can be turned into [`FilteringParameters`] (which is not
/// `Clone`) as often as needed and checked against independently.
#[derive(Debug, Clone)]
struct Params {
    paths: Option<Vec<String>>,
    webhooks: Option<Vec<String>>,
    tags: Option<Vec<String>>,
    methods: Option<Vec<String>>,
    security: Option<Vec<String>>,
}

impl Params {
    fn build(&self) -> FilteringParameters {
        FilteringParameters {
            paths: self.paths.clone(),
            webhooks: self.webhooks.clone(),
            tags: self.tags.clone(),
            methods: self.methods.clone(),
            security: self.security.clone(),
        }
    }
}

fn pattern_list(pool: &'static [&'static str]) -> impl Strategy<Value = Option<Vec<String>>> {
    prop::option::weighted(
        0.5,
        subsequence(pool, 1..=2).prop_map(|items| items.into_iter().map(String::from).collect()),
    )
}

fn params() -> impl Strategy<Value = Params> {
    (
        pattern_list(&["/a", "/a*", "/a/*", "*", "/b", "/c/*", "/zzz"]),
        pattern_list(&["hook", "h*", "*", "zzz"]),
        pattern_list(&["pets", "store", "admin", "ghost", "nope"]),
        pattern_list(&["get", "POST", "delete", "put", "trace"]),
        pattern_list(&["key", "oauth", "basic", "nope"]),
    )
        .prop_map(|(paths, webhooks, tags, methods, security)| Params {
            paths,
            webhooks,
            tags,
            methods,
            security,
        })
}

/// The pattern pools above only use a trailing `*`, so matching is equality or a prefix test.
fn pattern_matches(pattern: &str, name: &str) -> bool {
    match pattern.strip_suffix('*') {
        Some(prefix) => name.starts_with(prefix),
        None => pattern == name,
    }
}

/// Resolves a local `$ref` inside `doc` (the generator only escapes `/` as `~1`).
fn resolve<'a>(doc: &'a Value, reference: &str) -> Option<&'a Value> {
    let pointer = reference.strip_prefix('#')?;
    doc.pointer(pointer)
}

fn walk<'a>(value: &'a Value, visit: &mut impl FnMut(&'a str, &'a Value)) {
    match value {
        Value::Object(map) => {
            for (key, child) in map {
                visit(key, child);
                walk(child, visit);
            }
        }
        Value::Array(items) => items.iter().for_each(|child| walk(child, visit)),
        _ => {}
    }
}

/// Every security requirement array in `doc`: the root one and every operation's.
fn security_requirements(doc: &Value) -> Vec<&Value> {
    let mut found: Vec<&Value> = doc.get("security").into_iter().collect();
    for container in ["paths", "webhooks"] {
        for item in doc
            .get(container)
            .and_then(Value::as_object)
            .into_iter()
            .flatten()
            .map(|e| e.1)
        {
            for method in METHODS {
                found.extend(item.get(*method).and_then(|op| op.get("security")));
            }
        }
    }
    found.extend(
        doc.pointer("/components/pathItems/Shared")
            .into_iter()
            .flat_map(|item| METHODS.iter().filter_map(|m| item.get(*m)?.get("security"))),
    );
    found
}

/// The operations of a (possibly `$ref`) path item that `params` selects.
fn selected_methods(doc: &Value, item: &Value, params: &Params) -> Vec<&'static str> {
    let item = match item.get("$ref").and_then(Value::as_str) {
        Some(reference) => resolve(doc, reference).unwrap_or(&Value::Null),
        None => item,
    };
    let methods: Vec<String> = params
        .methods
        .iter()
        .flatten()
        .map(|m| m.to_lowercase())
        .collect();
    let names = |op: &Value, key: &str| -> Vec<String> {
        match (key, op.get(key)) {
            ("tags", Some(Value::Array(tags))) => tags
                .iter()
                .filter_map(Value::as_str)
                .map(String::from)
                .collect(),
            ("security", Some(Value::Array(reqs))) => reqs
                .iter()
                .filter_map(Value::as_object)
                .flat_map(|r| r.keys().cloned())
                .collect(),
            _ => Vec::new(),
        }
    };
    let allows = |filter: &Option<Vec<String>>, found: Vec<String>| match filter {
        Some(allowed) if !allowed.is_empty() => found.iter().any(|f| allowed.contains(f)),
        _ => true,
    };
    METHODS
        .iter()
        .copied()
        .filter(|method| {
            let Some(op) = item.get(*method) else {
                return false;
            };
            (methods.is_empty() || methods.iter().any(|m| m == method))
                && allows(&params.tags, names(op, "tags"))
                && allows(&params.security, names(op, "security"))
        })
        .collect()
}

fn check_selection(
    input: &Value,
    output: &Value,
    container: &str,
    patterns: &Option<Vec<String>>,
    params: &Params,
) -> Result<(), TestCaseError> {
    let Some(entries) = input.get(container).and_then(Value::as_object) else {
        prop_assert!(output.get(container).is_none());
        return Ok(());
    };
    let mut expected: Vec<(&str, Option<Vec<&str>>)> = Vec::new();
    for (name, item) in entries {
        if name.starts_with("x-") {
            expected.push((name, None));
            continue;
        }
        if let Some(patterns) = patterns
            && !patterns.iter().any(|p| pattern_matches(p, name))
        {
            continue;
        }
        let methods = selected_methods(input, item, params);
        if methods.is_empty() {
            continue;
        }
        let is_ref = item.get("$ref").is_some();
        expected.push((name, (!is_ref).then_some(methods)));
    }
    let kept = output[container]
        .as_object()
        .expect("container stays an object");
    let kept_names: Vec<&str> = kept.keys().map(String::as_str).collect();
    let expected_names: Vec<&str> = expected.iter().map(|(name, _)| *name).collect();
    prop_assert_eq!(&kept_names, &expected_names, "kept {} entries", container);
    for (name, methods) in expected {
        let Some(methods) = methods else {
            prop_assert_eq!(
                &kept[name],
                &entries[name],
                "{} {} is kept whole",
                container,
                name
            );
            continue;
        };
        let kept_methods: Vec<&str> = METHODS
            .iter()
            .copied()
            .filter(|m| kept[name].get(*m).is_some())
            .collect();
        prop_assert_eq!(
            kept_methods,
            methods,
            "methods kept under {} {}",
            container,
            name
        );
    }
    Ok(())
}

fn check_closure(output: &Value) -> Result<(), TestCaseError> {
    let mut dangling = Vec::new();
    walk(output, &mut |key, value| {
        if key == "$ref"
            && let Some(reference) = value.as_str()
            && resolve(output, reference).is_none()
        {
            dangling.push(reference.to_string());
        }
    });
    prop_assert!(
        dangling.is_empty(),
        "dangling $refs {:?} in {}",
        dangling,
        output
    );

    let declared = output
        .pointer("/components/securitySchemes")
        .and_then(Value::as_object);
    for requirements in security_requirements(output) {
        for requirement in requirements.as_array().into_iter().flatten() {
            for scheme in requirement.as_object().into_iter().flatten().map(|e| e.0) {
                prop_assert!(
                    declared.is_some_and(|d| d.contains_key(scheme)),
                    "security scheme {:?} is required but not declared in {}",
                    scheme,
                    output
                );
            }
        }
    }
    Ok(())
}

proptest! {
    #[test]
    fn filtered_output_is_reference_closed(doc in document(), params in params()) {
        check_closure(&doc.filter_by_parameters(params.build()))?;
    }

    #[test]
    fn filter_keeps_exactly_the_selected_operations(doc in document(), params in params()) {
        let output = doc.filter_by_parameters(params.build());
        check_selection(&doc, &output, "paths", &params.paths, &params)?;
        check_selection(&doc, &output, "webhooks", &params.webhooks, &params)?;
        prop_assert_eq!(output.get("x-tail"), Some(&json!(1)));
    }

    #[test]
    fn filtering_is_idempotent(doc in document(), params in params()) {
        let once = doc.filter_by_parameters(params.build());
        let twice = once.filter_by_parameters(params.build());
        prop_assert_eq!(
            serde_json::to_string(&twice).unwrap(),
            serde_json::to_string(&once).unwrap()
        );
    }

    #[test]
    fn owned_and_borrowed_filtering_agree(doc in document(), params in params()) {
        let borrowed = doc.filter_by_parameters(params.build());
        let owned = doc.into_filtered(params.build());
        prop_assert_eq!(
            serde_json::to_string(&owned).unwrap(),
            serde_json::to_string(&borrowed).unwrap()
        );
    }

    #[test]
    fn yaml_round_trips(value in yaml_value()) {
        let text = yaml::value_to_string(&value).expect("any value tree can be emitted");
        let back = yaml::parse_value(&text)
            .unwrap_or_else(|e| panic!("emitted YAML does not parse: {e}\n{text}"));
        prop_assert_eq!(
            serde_json::to_string(&back).unwrap(),
            serde_json::to_string(&value).unwrap(),
            "round trip through\n{}", text
        );
    }
}

/// Strings that YAML treats specially when unquoted, plus arbitrary text.
fn yaml_string() -> impl Strategy<Value = String> {
    prop_oneof![
        prop::sample::select(
            &[
                "",
                " ",
                "y",
                "No",
                "on",
                "null",
                "~",
                "true",
                "-0",
                "0x1F",
                "0o7",
                "1e3",
                ".inf",
                "-.NaN",
                "- x",
                "a: b",
                "a #b",
                "#x",
                "&a",
                "*a",
                "!t",
                "%x",
                "@x",
                "`x",
                "|",
                ">",
                "'",
                "\"",
                "{}",
                "[]",
                "?",
                ": ",
                "---",
                "...",
                "\t",
                "a\nb",
                "a\n",
                "\u{85}",
                "\u{2028}",
                "\u{feff}x",
                "\u{7f}",
                "\u{1}",
                "é",
                "😀",
                "3.1.0",
                "2024-01-01",
            ][..]
        )
        .prop_map(String::from),
        any::<String>(),
    ]
}

fn yaml_value() -> impl Strategy<Value = Value> {
    let leaf = prop_oneof![
        Just(Value::Null),
        any::<bool>().prop_map(Value::Bool),
        any::<i64>().prop_map(Value::from),
        any::<u64>().prop_map(Value::from),
        any::<f64>().prop_map(|f| Number::from_f64(f).map_or(Value::Null, Value::Number)),
        yaml_string().prop_map(Value::String),
    ];
    leaf.prop_recursive(4, 48, 5, |inner| {
        prop_oneof![
            prop::collection::vec(inner.clone(), 0..5).prop_map(Value::Array),
            prop::collection::vec((yaml_string(), inner), 0..5).prop_map(object),
        ]
    })
}

proptest! {
    /// Numbers in a JSON document come out of parsing and filtering exactly as written (as far as
    /// an `f64` can hold them): reading them must be correctly rounded, not merely close.
    #[test]
    fn json_numbers_survive_parsing_and_filtering(numbers in prop::collection::vec(any::<f64>(), 1..8)) {
        let numbers: Vec<Value> = numbers
            .into_iter()
            .filter_map(Number::from_f64)
            .map(Value::Number)
            .collect();
        let doc = json!({
            "openapi": "3.1.0",
            "info": { "title": "numbers", "version": "1" },
            "x-numbers": numbers,
        });
        let text = serde_json::to_string(&doc).unwrap();
        let parsed = parser::parse(&text).expect("generated document is valid");
        prop_assert_eq!(&parsed, &doc);
        let filtered = parsed.into_filtered(FilteringParameters::default());
        prop_assert_eq!(serde_json::to_string(&filtered).unwrap(), text);
    }
}
