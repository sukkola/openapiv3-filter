use serde_json::{Map, Value};
use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use wildmatch::WildMatch;

use crate::filter::content::json_path_filter;
use crate::filter::content::pointer;
use crate::filter::content::reference_collector;
use crate::filter::content::reference_collector::{
    collect_operation_securities, collect_operation_tags, collect_path_refs,
};
use crate::filter::content::reference_processor;

/// HTTP method keys recognized under a Path Item / Webhook Item object. Identical across
/// OpenAPI 3.0 and 3.1.
const HTTP_METHODS: [&str; 8] = [
    "get", "put", "post", "delete", "options", "head", "patch", "trace",
];

///Filtering parameters for the filtering trait
#[derive(Debug, Default)]
pub struct FilteringParameters {
    ///when provided only outputs paths that match the parameters
    pub paths: Option<Vec<String>>,
    ///when provided only outputs webhooks whose name matches the parameters (OpenAPI 3.1)
    pub webhooks: Option<Vec<String>>,
    ///when provided only outputs tags that match the parameters
    pub tags: Option<Vec<String>>,
    ///when provided only outputs http methods that match the parameters
    pub methods: Option<Vec<String>>,
    ///when provided only outputs endpoints that use provided security parameters
    pub security: Option<Vec<String>>,
}

///Adds filtering capability to OpenAPI documents
pub trait OpenAPIFilter {
    /// Filters an OpenAPI document based on provided criteria
    ///
    /// This trait provides a method to filter and extract portions of an OpenAPI document according to specified parameters.
    /// The filtering can be done by paths, webhooks, tags, HTTP methods, security schemes, and other criteria while maintaining
    /// referential integrity for used components and definitions.
    ///
    /// Filtering a structurally valid, already-parsed document cannot fail, so the filtered value
    /// is returned directly rather than wrapped in an `Option`.
    fn filter_by_parameters(&self, filters: FilteringParameters) -> Self
    where
        Self: Sized;
}

/// Filtering implementation for OpenAPI documents represented as a generic, order-preserving
/// `serde_json::Value` tree.
///
/// The document is never deserialized into a version-specific typed struct: structural elements
/// (components of any category, operation-level `callbacks`/`links`, `webhooks`, reusable
/// `pathItems`, ...) are addressed generically by key/path, so the engine is not limited to a
/// fixed list of fields a crate author happened to model. Only operation selection (does this
/// operation match `--path`/`--webhook`/`--method`/`--tag`/`--security`) is filter-specific logic;
/// everything downstream of "this operation is kept" is a single generic reference-closure walk
/// run uniformly over `paths`, `webhooks`, and every `components.*` category.
impl OpenAPIFilter for Value {
    fn filter_by_parameters(&self, filters: FilteringParameters) -> Self {
        let allowed_tags: HashSet<String> = filters.tags.into_iter().flatten().collect();
        let allowed_methods: HashSet<String> = filters
            .methods
            .into_iter()
            .flatten()
            .map(|m| m.to_lowercase())
            .collect();
        let allowed_securities: HashSet<String> = filters.security.into_iter().flatten().collect();

        let mut kept_tags: HashSet<String> = HashSet::new();
        let mut kept_securities: HashSet<String> = HashSet::new();

        let empty_map = Map::new();
        let paths_container = self
            .get("paths")
            .and_then(Value::as_object)
            .unwrap_or(&empty_map);
        let webhooks_container = self
            .get("webhooks")
            .and_then(Value::as_object)
            .unwrap_or(&empty_map);

        let filtered_paths = filter_operations_container(
            paths_container,
            &filters.paths,
            &allowed_methods,
            &allowed_tags,
            &allowed_securities,
            self,
            &mut kept_tags,
            &mut kept_securities,
        );
        let filtered_webhooks = filter_operations_container(
            webhooks_container,
            &filters.webhooks,
            &allowed_methods,
            &allowed_tags,
            &allowed_securities,
            self,
            &mut kept_tags,
            &mut kept_securities,
        );

        let mut referenced: HashSet<String> = HashSet::new();
        collect_path_refs(
            &Value::Object(filtered_paths.clone()),
            &mut referenced,
            None,
        );
        collect_path_refs(
            &Value::Object(filtered_webhooks.clone()),
            &mut referenced,
            None,
        );

        let components = build_components(self, &mut referenced, &kept_securities);
        let tags = found_refs_to_tags(self, &kept_tags);

        let mut result = self.clone();
        if let Some(obj) = result.as_object_mut() {
            if self.get("paths").is_some() {
                obj.insert("paths".to_string(), Value::Object(filtered_paths));
            }
            if self.get("webhooks").is_some() {
                obj.insert("webhooks".to_string(), Value::Object(filtered_webhooks));
            }
            match components {
                Some(components) => {
                    obj.insert("components".to_string(), components);
                }
                None => {
                    obj.remove("components");
                }
            }
            match tags {
                Some(tags) => {
                    obj.insert("tags".to_string(), tags);
                }
                None => {
                    obj.remove("tags");
                }
            }
        }
        result
    }
}

/// Resolves a Path Item / Webhook Item `$ref` (`components.pathItems.*`) to the value whose
/// operations should be evaluated against the active filters. Reusable path items are treated
/// like any other component category: the original `$ref` is kept verbatim in the output (not
/// inlined) and the referenced component is pulled in wholesale by the generic reference-closure
/// walk once reachable.
fn resolve_operations_source<'a>(item: &'a Value, doc: &'a Value) -> Cow<'a, Value> {
    match item.get("$ref").and_then(Value::as_str) {
        Some(reference) => resolve_json_pointer(doc, reference)
            .map(Cow::Borrowed)
            .unwrap_or(Cow::Owned(Value::Null)),
        None => Cow::Borrowed(item),
    }
}

/// Resolves a `#/a/b/c`-style JSON reference string into the referenced value in `doc`.
fn resolve_json_pointer<'a>(doc: &'a Value, reference: &str) -> Option<&'a Value> {
    match pointer::ref_segments(reference) {
        Some(segments) => {
            let mut current = doc;
            for segment in &segments {
                current = match current {
                    Value::Object(map) => map.get(segment)?,
                    Value::Array(items) => items.get(segment.parse::<usize>().ok()?)?,
                    _ => return None,
                };
            }
            Some(current)
        }
        // `#` alone is the document root; other shapes (external refs) are not resolvable here.
        None => (reference == "#").then_some(doc),
    }
}

/// Returns true if an operation value matches the active method/tag/security filters. An empty
/// allowed-set for a given concern means "no restriction" (matches the existing "no filter = keep
/// all" semantics already used by `--path`, extended symmetrically to `--webhook`).
fn operation_matches(
    method: &str,
    operation: &Value,
    allowed_methods: &HashSet<String>,
    allowed_tags: &HashSet<String>,
    allowed_securities: &HashSet<String>,
) -> bool {
    if !allowed_methods.is_empty() && !allowed_methods.contains(method) {
        return false;
    }
    if !allowed_tags.is_empty() {
        let tags = reference_collector::operation_tags(operation);
        if !tags.iter().any(|tag| allowed_tags.contains(tag)) {
            return false;
        }
    }
    if !allowed_securities.is_empty() {
        let securities = reference_collector::operation_security_names(operation);
        if !securities
            .iter()
            .any(|security| allowed_securities.contains(security))
        {
            return false;
        }
    }
    true
}

/// Filters the entries of a `paths` or `webhooks` container (identical shape: a map from name to
/// a Path/Webhook Item object, each potentially a `$ref`, each potentially holding HTTP-method
/// operation entries).
///
/// Vendor extension siblings (`x-*` keys) at the container level are preserved verbatim, matching
/// the fact that the OpenAPI Paths/Webhooks Object permits extensions alongside entries.
#[allow(clippy::too_many_arguments)]
fn filter_operations_container(
    container: &Map<String, Value>,
    name_patterns: &Option<Vec<String>>,
    allowed_methods: &HashSet<String>,
    allowed_tags: &HashSet<String>,
    allowed_securities: &HashSet<String>,
    doc: &Value,
    kept_tags: &mut HashSet<String>,
    kept_securities: &mut HashSet<String>,
) -> Map<String, Value> {
    let name_matchers: Option<Vec<WildMatch>> = name_patterns
        .as_ref()
        .map(|patterns| patterns.iter().map(|p| WildMatch::new(p)).collect());

    let mut result = Map::new();

    for (name, item) in container {
        if name.starts_with("x-") {
            result.insert(name.clone(), item.clone());
            continue;
        }

        if let Some(matchers) = &name_matchers
            && !matchers.iter().any(|matcher| matcher.matches(name))
        {
            continue;
        }

        let resolved = resolve_operations_source(item, doc);
        let operations: Vec<(&str, &Value)> = resolved
            .as_object()
            .map(|obj| {
                HTTP_METHODS
                    .iter()
                    .filter_map(|method| obj.get(*method).map(|op| (*method, op)))
                    .collect()
            })
            .unwrap_or_default();

        let matching_ops: Vec<(&str, &Value)> = operations
            .into_iter()
            .filter(|(method, operation)| {
                operation_matches(
                    method,
                    operation,
                    allowed_methods,
                    allowed_tags,
                    allowed_securities,
                )
            })
            .collect();

        if matching_ops.is_empty() {
            continue;
        }

        collect_operation_tags(
            matching_ops.iter().map(|(_, op)| *op).collect(),
            kept_tags,
            allowed_tags,
        );
        collect_operation_securities(
            matching_ops.iter().map(|(_, op)| *op).collect(),
            kept_securities,
            allowed_securities,
        );

        if item.get("$ref").is_some() {
            // Wholesale keep: the reusable path item itself is filtered as a component, not here.
            result.insert(name.clone(), item.clone());
            continue;
        }

        let mut new_item = item.clone();
        if let Some(obj) = new_item.as_object_mut() {
            for method in HTTP_METHODS {
                match matching_ops.iter().find(|(m, _)| *m == method) {
                    Some((_, operation)) => {
                        obj.insert(
                            method.to_string(),
                            clone_operation(operation, allowed_tags, allowed_securities),
                        );
                    }
                    None => {
                        obj.remove(method);
                    }
                }
            }
        }
        result.insert(name.clone(), new_item);
    }

    result
}

/// Clones an operation value, pruning its `tags` and `security` arrays down to the allowed sets
/// when either filter is active. All other fields (`requestBody`, `responses`, `parameters`,
/// `callbacks`, `links`, ...) are carried through unchanged, so any structural element under an
/// operation participates in the generic reference-closure walk without needing field-specific
/// handling here.
fn clone_operation(
    operation: &Value,
    allowed_tags: &HashSet<String>,
    allowed_securities: &HashSet<String>,
) -> Value {
    let filter_tags = !allowed_tags.is_empty();
    let filter_securities = !allowed_securities.is_empty();
    if !filter_tags && !filter_securities {
        return operation.clone();
    }

    let mut new_operation = operation.clone();
    if let Some(obj) = new_operation.as_object_mut() {
        if filter_tags {
            let new_tags: Vec<Value> = obj
                .get("tags")
                .and_then(Value::as_array)
                .map(|tags| {
                    tags.iter()
                        .filter(|tag| {
                            tag.as_str()
                                .map(|tag| allowed_tags.contains(tag))
                                .unwrap_or(false)
                        })
                        .cloned()
                        .collect()
                })
                .unwrap_or_default();
            if new_tags.is_empty() {
                obj.remove("tags");
            } else {
                obj.insert("tags".to_string(), Value::Array(new_tags));
            }
        }
        if filter_securities {
            let new_security: Vec<Value> = obj
                .get("security")
                .and_then(Value::as_array)
                .map(|requirements| {
                    requirements
                        .iter()
                        .filter_map(|requirement| {
                            let requirement_map = requirement.as_object()?;
                            let filtered: Map<String, Value> = requirement_map
                                .iter()
                                .filter(|(scheme, _)| allowed_securities.contains(*scheme))
                                .map(|(k, v)| (k.clone(), v.clone()))
                                .collect();
                            if filtered.is_empty() {
                                None
                            } else {
                                Some(Value::Object(filtered))
                            }
                        })
                        .collect()
                })
                .unwrap_or_default();
            if new_security.is_empty() {
                obj.remove("security");
            } else {
                obj.insert("security".to_string(), Value::Array(new_security));
            }
        }
    }
    new_operation
}

/// Builds the filtered `components` value: the generic reference-closure result (every category,
/// including 3.1's `pathItems`), with `securitySchemes` additionally re-filtered against
/// `kept_securities` — security requirements reference scheme names directly rather than via
/// `$ref`, so they aren't reachable by the generic walk and need this explicit pass, mirroring the
/// pre-existing special-case for security schemes.
fn build_components(
    doc: &Value,
    referenced: &mut HashSet<String>,
    kept_securities: &HashSet<String>,
) -> Option<Value> {
    let mut components = found_refs_to_components(doc, referenced).unwrap_or(Value::Object(Map::new()));

    let security_schemes = filter_security_schemes(doc, kept_securities);
    if let Some(obj) = components.as_object_mut() {
        match security_schemes {
            Some(schemes) => {
                obj.insert("securitySchemes".to_string(), schemes);
            }
            None => {
                obj.remove("securitySchemes");
            }
        }
    }

    let is_empty = components
        .as_object()
        .map(Map::is_empty)
        .unwrap_or(true);
    if is_empty { None } else { Some(components) }
}

/// Filters `components.securitySchemes` down to the names referenced by kept operations.
fn filter_security_schemes(doc: &Value, kept_securities: &HashSet<String>) -> Option<Value> {
    let schemes = doc.pointer("/components/securitySchemes")?.as_object()?;
    let filtered: Map<String, Value> = schemes
        .iter()
        .filter(|(name, _)| kept_securities.contains(*name))
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    if filtered.is_empty() {
        None
    } else {
        Some(Value::Object(filtered))
    }
}

/// Computes the generic reference-closure result over every `components.*` category.
///
/// This walks the whole `components` value tree once (category-agnostically, via
/// `reference_collector::collect_component_refs`), builds the transitive `$ref` reachability graph
/// from the directly-referenced set (`referenced`), and keeps every entry (and everything that
/// entry itself references) reachable from it. A component category introduced by a future
/// OpenAPI revision is handled the same way, as long as it's reachable via `$ref` — no
/// category-specific code is needed here.
fn found_refs_to_components(doc: &Value, referenced: &mut HashSet<String>) -> Option<Value> {
    let components = doc.get("components")?;

    let mut component_references: HashMap<String, Vec<String>> = HashMap::new();
    reference_collector::collect_component_refs(
        components,
        "#/components",
        &mut component_references,
        2,
        0,
    );

    // `get_kept_references` needs the *full* reference graph to walk multi-hop chains
    // transitively (A -> B -> C, ...); pre-filtering it down to only entries already in
    // `referenced` would strand any node reachable only through an intermediate hop.
    //
    // The graph's keys and edges are in canonical JSON Pointer form, so the directly-referenced
    // set is normalised the same way before the walk (`$ref`s in operations are raw strings).
    let referenced: HashSet<String> = referenced
        .iter()
        .map(|reference| pointer::canonical_ref(reference))
        .collect();
    let final_references = reference_processor::get_kept_references(&component_references, &referenced);

    // Keep paths as segment lists rather than dot-joined strings: component names such as
    // `issuing.authorization` contain dots and must stay one key.
    let component_paths: Vec<Vec<String>> = final_references
        .iter()
        .filter(|component| component.starts_with("#/components/"))
        .filter_map(|component| pointer::ref_segments(component))
        .map(|segments| segments[1..].to_vec())
        .collect();

    json_path_filter::filter_json_segments(components, &component_paths)
}

/// Filters the top-level `tags` array down to the tag names actually used by kept operations.
fn found_refs_to_tags(doc: &Value, kept_tags: &HashSet<String>) -> Option<Value> {
    let tags = doc.get("tags")?.as_array()?;
    let filtered: Vec<Value> = tags
        .iter()
        .filter(|tag| {
            tag.get("name")
                .and_then(Value::as_str)
                .map(|name| kept_tags.contains(name))
                .unwrap_or(false)
        })
        .cloned()
        .collect();
    if filtered.is_empty() {
        None
    } else {
        Some(Value::Array(filtered))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser;
    use insta::assert_json_snapshot;
    use serde_json::json;

    fn load(path: &str) -> Value {
        let contents = std::fs::read_to_string(path).expect("fixture should be readable");
        parser::parse(&contents).expect("fixture should parse")
    }

    #[test]
    fn it_filters_paths_with_no_matches() {
        let openapi = load("tests/resources/user-reference.yaml");
        let filtered_api = openapi.filter_by_parameters(FilteringParameters {
            paths: Some(vec![String::from("non-matching-path")]),
            ..Default::default()
        });
        assert_json_snapshot!(filtered_api);
    }

    #[test]
    fn it_filters_paths_with_partial_path_name_match() {
        let openapi = load("tests/resources/user-reference.yaml");
        let filtered_api = openapi.filter_by_parameters(FilteringParameters {
            paths: Some(vec![String::from("*userId*")]),
            ..Default::default()
        });
        assert_json_snapshot!(filtered_api);
    }

    #[test]
    fn it_filters_paths_with_method_name_match() {
        let openapi = load("tests/resources/user-reference.yaml");
        let filtered_api = openapi.filter_by_parameters(FilteringParameters {
            methods: Some(vec![String::from("post")]),
            ..Default::default()
        });
        assert_json_snapshot!(filtered_api);
    }

    #[test]
    fn it_filters_paths_with_tag_name_match() {
        let openapi = load("tests/resources/user-reference.yaml");
        let filtered_api = openapi.filter_by_parameters(FilteringParameters {
            tags: Some(vec![String::from("item")]),
            ..Default::default()
        });
        assert_json_snapshot!(filtered_api);
    }

    #[test]
    fn it_filters_paths_with_partial_path_tag_name_and_method_name_match() {
        let openapi = load("tests/resources/user-reference.yaml");
        let filtered_api = openapi.filter_by_parameters(FilteringParameters {
            methods: Some(vec![String::from("get")]),
            tags: Some(vec![String::from("item")]),
            paths: Some(vec![String::from("*userId*")]),
            ..Default::default()
        });
        assert_json_snapshot!(filtered_api);
    }

    #[test]
    fn it_filters_petstore_with_full_path() {
        let openapi = load("tests/resources/petstore.yaml");
        let filtered_api = openapi.filter_by_parameters(FilteringParameters {
            paths: Some(vec![String::from("/pet/{petId}")]),
            methods: Some(vec![String::from("get")]),
            ..Default::default()
        });
        assert_json_snapshot!(filtered_api);
    }

    #[test]
    fn it_filters_petstore_with_full_path_an_api_key_auth() {
        let openapi = load("tests/resources/petstore.yaml");
        let filtered_api = openapi.filter_by_parameters(FilteringParameters {
            paths: Some(vec![String::from("/pet/{petId}")]),
            methods: Some(vec![String::from("get")]),
            security: Some(vec![String::from("api_key")]),
            ..Default::default()
        });
        assert_json_snapshot!(filtered_api);
    }

    #[test]
    fn it_filters_petstore_with_partial_path_and_does_not_keep_unnecessary_security_schemes() {
        let openapi = load("tests/resources/petstore.yaml");
        let filtered_api = openapi.filter_by_parameters(FilteringParameters {
            paths: Some(vec![String::from("*createWithList")]),
            ..Default::default()
        });
        assert_json_snapshot!(filtered_api);
    }

    #[test]
    fn it_keeps_full_structural_coverage_with_no_filters_3_0() {
        let openapi = load("tests/resources/full-coverage-3.0.yaml");
        let filtered = openapi.filter_by_parameters(FilteringParameters::default());

        // Discriminator survives on the kept schema, mapping untouched...
        assert_eq!(
            filtered.pointer("/components/schemas/Item/discriminator/mapping/dog"),
            Some(&Value::String("#/components/schemas/Dog".to_string()))
        );
        // ...but the mapping target is not force-included: it's unreferenced by any $ref.
        assert!(filtered.pointer("/components/schemas/Dog").is_none());
        // A component unreachable from any kept operation is dropped.
        assert!(filtered.pointer("/components/schemas/Unused").is_none());

        // Reusable header, example, and callback referenced via $ref are all kept.
        assert!(filtered.pointer("/components/headers/RateLimit").is_some());
        assert!(
            filtered
                .pointer("/components/examples/ItemExample")
                .is_some()
        );
        assert!(
            filtered
                .pointer("/components/callbacks/ReusableCallback")
                .is_some()
        );
        // The schema referenced only from inside that reusable callback's request body is
        // pulled in transitively.
        assert!(
            filtered
                .pointer("/components/schemas/ItemChangeEvent")
                .is_some()
        );
        // An operation-level links entry survives unchanged.
        assert_eq!(
            filtered.pointer("/paths/~1items/get/responses/200/links/GetItemById/operationId"),
            Some(&Value::String("getItemById".to_string()))
        );

        assert_json_snapshot!(filtered);
    }

    #[test]
    fn it_filters_full_structural_coverage_by_tag_3_0() {
        let openapi = load("tests/resources/full-coverage-3.0.yaml");
        let filtered = openapi
            .filter_by_parameters(FilteringParameters {
                tags: Some(vec![String::from("items")]),
                ..Default::default()
            });

        assert!(filtered.pointer("/paths/~1items").is_some());
        assert!(filtered.pointer("/paths/~1unused").is_none());
    }

    #[test]
    fn it_keeps_full_structural_coverage_with_no_filters_3_1() {
        let openapi = load("tests/resources/full-coverage-3.1.yaml");
        let filtered = openapi.filter_by_parameters(FilteringParameters::default());

        // 3.1-only JSON Schema union type survives untouched.
        assert_eq!(
            filtered.pointer("/components/schemas/Item/properties/name/type"),
            Some(&Value::Array(vec![
                Value::String("string".to_string()),
                Value::String("null".to_string())
            ]))
        );

        // The reusable path item referenced via $ref stays a $ref in `paths`...
        assert_eq!(
            filtered.pointer("/paths/~1common/$ref"),
            Some(&Value::String(
                "#/components/pathItems/CommonCrud".to_string()
            ))
        );
        // ...while the referenced component itself, and its own transitive schema reference,
        // are pulled in by the generic closure walk.
        assert!(filtered.pointer("/components/pathItems/CommonCrud").is_some());
        assert!(filtered.pointer("/components/schemas/CommonItem").is_some());

        // No filters at all keeps every webhook, unchanged.
        assert!(filtered.pointer("/webhooks/userCreated").is_some());
        assert!(filtered.pointer("/webhooks/userDeleted").is_some());

        assert_json_snapshot!(filtered);
    }

    #[test]
    fn it_filters_webhooks_by_name() {
        let openapi = load("tests/resources/full-coverage-3.1.yaml");
        let filtered = openapi
            .filter_by_parameters(FilteringParameters {
                webhooks: Some(vec![String::from("userCreated")]),
                ..Default::default()
            });

        assert!(filtered.pointer("/webhooks/userCreated").is_some());
        assert!(filtered.pointer("/webhooks/userDeleted").is_none());
        // --webhook does not restrict paths: all path entries remain.
        assert!(filtered.pointer("/paths/~1items").is_some());
        assert!(filtered.pointer("/paths/~1unused").is_some());
    }

    #[test]
    fn it_filters_webhooks_by_method_affecting_only_webhooks() {
        let openapi = load("tests/resources/full-coverage-3.1.yaml");
        let filtered = openapi
            .filter_by_parameters(FilteringParameters {
                methods: Some(vec![String::from("post")]),
                ..Default::default()
            });

        // Every path operation in the fixture is a GET, so --method post excludes them all.
        assert_eq!(
            filtered.pointer("/paths"),
            Some(&Value::Object(Map::new()))
        );
        assert!(filtered.pointer("/webhooks/userCreated").is_some());
        assert!(filtered.pointer("/webhooks/userDeleted").is_some());
    }

    #[test]
    fn it_tag_filter_affects_both_a_path_and_a_webhook() {
        let openapi = load("tests/resources/full-coverage-3.1.yaml");
        let filtered = openapi
            .filter_by_parameters(FilteringParameters {
                tags: Some(vec![String::from("items")]),
                ..Default::default()
            });

        assert!(filtered.pointer("/paths/~1items").is_some());
        assert!(filtered.pointer("/paths/~1unused").is_none());
        // userCreated carries both `users` and `items` tags; userDeleted only `users`.
        assert!(filtered.pointer("/webhooks/userCreated").is_some());
        assert!(filtered.pointer("/webhooks/userDeleted").is_none());
    }

    #[test]
    fn it_path_filter_does_not_affect_webhooks() {
        let openapi = load("tests/resources/full-coverage-3.1.yaml");
        let filtered = openapi
            .filter_by_parameters(FilteringParameters {
                paths: Some(vec![String::from("/items")]),
                ..Default::default()
            });

        assert!(filtered.pointer("/paths/~1items").is_some());
        assert!(filtered.pointer("/paths/~1items~1{itemId}").is_none());
        // No --webhook filter given, so every webhook remains despite --path being active.
        assert!(filtered.pointer("/webhooks/userCreated").is_some());
        assert!(filtered.pointer("/webhooks/userDeleted").is_some());
    }

    #[test]
    fn it_webhook_filter_does_not_affect_paths() {
        let openapi = load("tests/resources/full-coverage-3.1.yaml");
        let filtered = openapi
            .filter_by_parameters(FilteringParameters {
                webhooks: Some(vec![String::from("userCreated")]),
                ..Default::default()
            });

        // No --path filter given, so every path remains despite --webhook being active.
        assert!(filtered.pointer("/paths/~1items").is_some());
        assert!(filtered.pointer("/paths/~1items~1{itemId}").is_some());
        assert!(filtered.pointer("/paths/~1unused").is_some());
        assert!(filtered.pointer("/webhooks/userDeleted").is_none());
    }

    #[test]
    fn it_preserves_the_openapi_version_string_after_filtering() {
        let openapi = load("tests/resources/full-coverage-3.1.yaml");
        let filtered = openapi
            .filter_by_parameters(FilteringParameters {
                methods: Some(vec![String::from("get")]),
                ..Default::default()
            });
        assert_eq!(
            filtered.get("openapi"),
            Some(&Value::String("3.1.0".to_string()))
        );
    }

    #[test]
    fn it_preserves_vendor_extensions_at_any_level() {
        let doc = json!({
            "openapi": "3.0.3",
            "info": { "title": "x", "version": "1.0.0" },
            "x-internal-id": "abc123",
            "paths": {
                "x-path-level-extension": { "note": "kept verbatim" },
                "/items": {
                    "get": {
                        "responses": { "200": { "description": "ok" } }
                    }
                }
            }
        });
        let filtered = doc.filter_by_parameters(FilteringParameters::default());

        assert_eq!(
            filtered.get("x-internal-id"),
            Some(&Value::String("abc123".to_string()))
        );
        assert_eq!(
            filtered.pointer("/paths/x-path-level-extension/note"),
            Some(&Value::String("kept verbatim".to_string()))
        );
        assert!(filtered.pointer("/paths/~1items").is_some());
    }

    #[test]
    fn it_matches_webhooks_by_wildcard_name() {
        let openapi = load("tests/resources/full-coverage-3.1.yaml");
        let filtered = openapi
            .filter_by_parameters(FilteringParameters {
                webhooks: Some(vec![String::from("user*")]),
                ..Default::default()
            });

        assert!(filtered.pointer("/webhooks/userCreated").is_some());
        assert!(filtered.pointer("/webhooks/userDeleted").is_some());
    }

    #[test]
    fn it_does_not_panic_on_a_dangling_ref() {
        let doc = json!({
            "openapi": "3.0.3",
            "info": { "title": "x", "version": "1.0.0" },
            "paths": {
                "/items": {
                    "get": {
                        "responses": {
                            "200": {
                                "description": "ok",
                                "content": {
                                    "application/json": {
                                        "schema": { "$ref": "#/components/schemas/DoesNotExist" }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        });
        let filtered = doc.filter_by_parameters(FilteringParameters::default());

        // The dangling $ref is preserved as-is (this tool filters structure, it does not
        // validate that references resolve); nothing in the pipeline panics on the missing target.
        assert_eq!(
            filtered.pointer("/paths/~1items/get/responses/200/content/application~1json/schema/$ref"),
            Some(&Value::String("#/components/schemas/DoesNotExist".to_string()))
        );
        assert!(filtered.get("components").is_none());
    }

    #[test]
    fn it_excludes_operations_whose_security_does_not_match_the_filter() {
        let openapi = load("tests/resources/full-coverage-3.1.yaml");
        let filtered = openapi
            .filter_by_parameters(FilteringParameters {
                security: Some(vec![String::from("ApiKeyAuth")]),
                ..Default::default()
            });

        // /items declares ApiKeyAuth security and is kept...
        assert!(filtered.pointer("/paths/~1items").is_some());
        // ...while /items/{itemId} has no security field at all, so it's excluded.
        assert!(filtered.pointer("/paths/~1items~1{itemId}").is_none());
        // userDeleted declares ApiKeyAuth and is kept, userCreated has none and is excluded.
        assert!(filtered.pointer("/webhooks/userDeleted").is_some());
        assert!(filtered.pointer("/webhooks/userCreated").is_none());
    }

    #[test]
    fn it_loads_json_input() {
        let openapi = load("tests/resources/user-reference.json");
        let filtered = openapi
            .filter_by_parameters(FilteringParameters {
                tags: Some(vec![String::from("item")]),
                ..Default::default()
            });
        assert!(filtered.pointer("/paths/~1users~1{userId}").is_some());
    }

    #[test]
    fn it_loads_json_input_for_full_coverage_3_0() {
        let openapi = load("tests/resources/full-coverage-3.0.json");
        let filtered = openapi
            .filter_by_parameters(FilteringParameters::default());

        assert_eq!(
            filtered.pointer("/components/schemas/Item/discriminator/mapping/dog"),
            Some(&Value::String("#/components/schemas/Dog".to_string()))
        );
        assert!(filtered.pointer("/components/schemas/Dog").is_none());
        assert!(filtered.pointer("/components/schemas/Unused").is_none());
        assert!(filtered.pointer("/components/headers/RateLimit").is_some());
        assert!(
            filtered
                .pointer("/components/examples/ItemExample")
                .is_some()
        );
        assert!(
            filtered
                .pointer("/components/callbacks/ReusableCallback")
                .is_some()
        );
    }

    #[test]
    fn it_loads_json_input_for_full_coverage_3_1() {
        let openapi = load("tests/resources/full-coverage-3.1.json");
        let filtered = openapi
            .filter_by_parameters(FilteringParameters::default());

        assert_eq!(
            filtered.pointer("/components/schemas/Item/properties/name/type"),
            Some(&Value::Array(vec![
                Value::String("string".to_string()),
                Value::String("null".to_string())
            ]))
        );
        assert!(filtered.pointer("/components/pathItems/CommonCrud").is_some());
        assert!(filtered.pointer("/webhooks/userCreated").is_some());
        assert!(filtered.pointer("/webhooks/userDeleted").is_some());
    }

    #[test]
    fn it_follows_a_deep_ref_chain_including_oneof_and_allof_branches_and_a_cycle() {
        let openapi = load("tests/resources/deep-ref-chain.yaml");
        let filtered = openapi
            .filter_by_parameters(FilteringParameters::default());

        for schema in [
            "Level1", "Level2", "Level3", "Level4", "Level4b", "Level4c", "Level5", "Shared",
            "CycleA", "CycleB", "Other",
        ] {
            assert!(
                filtered
                    .pointer(&format!("/components/schemas/{schema}"))
                    .is_some(),
                "expected {schema} to be kept"
            );
        }
        assert!(filtered.pointer("/components/schemas/Unused").is_none());
    }

    #[test]
    fn it_follows_a_deep_ref_chain_from_json_input() {
        let openapi = load("tests/resources/deep-ref-chain.json");
        let filtered = openapi
            .filter_by_parameters(FilteringParameters::default());

        assert!(filtered.pointer("/components/schemas/CycleB").is_some());
        assert!(filtered.pointer("/components/schemas/Unused").is_none());
    }

    #[test]
    fn it_prunes_the_unreached_branch_of_a_deep_ref_chain_when_filtered() {
        let openapi = load("tests/resources/deep-ref-chain.yaml");
        let filtered = openapi
            .filter_by_parameters(FilteringParameters {
                tags: Some(vec![String::from("chain")]),
                ..Default::default()
            });

        assert!(filtered.pointer("/paths/~1other").is_none());
        // Only reachable from the excluded /other operation.
        assert!(filtered.pointer("/components/schemas/Other").is_none());
        // Still reachable from the kept /chain operation's deep reference chain.
        assert!(filtered.pointer("/components/schemas/CycleB").is_some());
    }

    #[test]
    fn it_supports_a_webhooks_only_document_with_no_paths_field() {
        let doc = json!({
            "openapi": "3.1.0",
            "info": { "title": "x", "version": "1.0.0" },
            "webhooks": {
                "userCreated": {
                    "post": { "responses": { "200": { "description": "ok" } } }
                }
            }
        });
        let filtered = doc.filter_by_parameters(FilteringParameters::default());

        assert!(filtered.get("paths").is_none());
        assert!(filtered.pointer("/webhooks/userCreated").is_some());
    }

    #[test]
    fn operation_matches_rejects_operations_whose_security_does_not_intersect() {
        let operation = json!({ "security": [{ "basic_auth": [] }] });
        let allowed_securities: HashSet<String> = [String::from("api_key")].into_iter().collect();
        assert!(!operation_matches(
            "get",
            &operation,
            &HashSet::new(),
            &HashSet::new(),
            &allowed_securities,
        ));
    }

    #[test]
    fn clone_operation_drops_tags_and_security_fields_when_nothing_survives_the_filter() {
        let operation = json!({
            "tags": ["other"],
            "security": [{ "basic_auth": [] }],
        });
        let allowed_tags: HashSet<String> = [String::from("item")].into_iter().collect();
        let allowed_securities: HashSet<String> = [String::from("api_key")].into_iter().collect();

        let cloned = clone_operation(&operation, &allowed_tags, &allowed_securities);

        assert!(cloned.get("tags").is_none());
        assert!(cloned.get("security").is_none());
    }
}
