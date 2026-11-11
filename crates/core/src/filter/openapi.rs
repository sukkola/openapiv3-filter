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

    /// Like [`filter_by_parameters`](Self::filter_by_parameters), but consumes the document, so
    /// that the parts which are kept can be moved into the result instead of being copied. Prefer
    /// this when the unfiltered document is not needed afterwards; the result is the same.
    fn into_filtered(self, filters: FilteringParameters) -> Self
    where
        Self: Sized,
    {
        self.filter_by_parameters(filters)
    }
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
///
/// Both methods share every decision (see [`Selection`]); they differ only in whether the kept
/// values are copied out of the document or moved out of it.
impl OpenAPIFilter for Value {
    fn filter_by_parameters(&self, filters: FilteringParameters) -> Self {
        let selection = Selection::new(self, filters);

        let paths = selection.materialize_container(
            borrowed_entries(self.get("paths"), &selection.paths),
            &selection.paths,
        );
        let webhooks = selection.materialize_container(
            borrowed_entries(self.get("webhooks"), &selection.webhooks),
            &selection.webhooks,
        );
        let referenced = collect_refs(&paths, &webhooks);

        let components = self.get("components").map(|components| {
            let kept = kept_component_paths(components, &referenced);
            let security_schemes = filter_security_schemes(components, &selection.kept_securities);
            let filtered = json_path_filter::filter_json_segments(components, &kept);
            (filtered, security_schemes)
        });
        let tags = found_refs_to_tags(self, &selection.kept_tags);

        let mut result = clone_except(self, |key| {
            matches!(key, "paths" | "webhooks" | "components" | "tags")
        });
        assemble(&mut result, paths, webhooks, components, tags);
        result
    }

    fn into_filtered(mut self, filters: FilteringParameters) -> Self {
        // Every decision is made against the intact document (a path item `$ref` may point
        // anywhere in it) before anything is moved out.
        let selection = Selection::new(&self, filters);

        // Taking a value leaves `null` under its key, which keeps the key's position for the
        // final edits in `assemble`, exactly as the borrowed variant's placeholders do.
        let mut take = |key: &str| {
            self.as_object_mut()
                .and_then(|obj| obj.get_mut(key))
                .map(std::mem::take)
        };
        let paths = selection.materialize_container(
            owned_entries(take("paths")),
            &selection.paths,
        );
        let webhooks = selection.materialize_container(
            owned_entries(take("webhooks")),
            &selection.webhooks,
        );
        let referenced = collect_refs(&paths, &webhooks);

        let components = take("components").map(|components| {
            let kept = kept_component_paths(&components, &referenced);
            let security_schemes = filter_security_schemes(&components, &selection.kept_securities);
            let filtered = json_path_filter::filter_json_segments_owned(components, &kept);
            (filtered, security_schemes)
        });
        let tags = found_refs_to_tags(&self, &selection.kept_tags);

        assemble(&mut self, paths, webhooks, components, tags);
        self
    }
}

/// Writes the filtered parts into `result`, a copy of the document whose `paths`, `webhooks`,
/// `components` and `tags` keys (where present) hold placeholders.
fn assemble(
    result: &mut Value,
    paths: Map<String, Value>,
    webhooks: Map<String, Value>,
    components: Option<(Option<Value>, Option<Value>)>,
    tags: Option<Value>,
) {
    let components = components.and_then(|(filtered, security_schemes)| {
        build_components(filtered, security_schemes)
    });
    let Some(obj) = result.as_object_mut() else {
        return;
    };
    if obj.contains_key("paths") {
        obj.insert("paths".to_string(), Value::Object(paths));
    }
    if obj.contains_key("webhooks") {
        obj.insert("webhooks".to_string(), Value::Object(webhooks));
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

/// Every `$ref` string in the filtered `paths` and `webhooks`.
fn collect_refs(paths: &Map<String, Value>, webhooks: &Map<String, Value>) -> HashSet<String> {
    let mut referenced: HashSet<String> = HashSet::new();
    for (key, value) in paths.iter().chain(webhooks) {
        collect_path_refs(value, &mut referenced, Some(key));
    }
    referenced
}

/// Clones `value`, except that when it is an object, the values under keys `skip` accepts are
/// replaced with `null` instead of being deep-copied. The keys themselves stay in place, so the
/// caller can overwrite or remove them afterwards and get exactly the key order a full clone
/// followed by the same edits would have produced, without paying for copies that are about to be
/// discarded.
fn clone_except(value: &Value, skip: impl Fn(&str) -> bool) -> Value {
    match value {
        Value::Object(map) => Value::Object(
            map.iter()
                .map(|(key, value)| {
                    let value = if skip(key) { Value::Null } else { value.clone() };
                    (key.clone(), value)
                })
                .collect(),
        ),
        other => other.clone(),
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

/// What filtering does with one entry of a `paths` or `webhooks` container.
#[derive(Debug, PartialEq)]
enum EntryDecision {
    Drop,
    /// Kept unchanged: a vendor extension, or a `$ref` to a reusable path item (which is
    /// filtered as a component instead).
    Whole,
    /// Kept with only these operations, each pruned by [`prune_operation`].
    Methods(Vec<&'static str>),
}

/// Every filtering decision, made by reading the unfiltered document: which entries of `paths`
/// and `webhooks` (and which of their operations) are kept, and which tags and security scheme
/// names the kept operations use.
struct Selection {
    allowed_tags: HashSet<String>,
    allowed_securities: HashSet<String>,
    kept_tags: HashSet<String>,
    kept_securities: HashSet<String>,
    /// One decision per entry of `paths`, in document order.
    paths: Vec<EntryDecision>,
    /// One decision per entry of `webhooks`, in document order.
    webhooks: Vec<EntryDecision>,
}

impl Selection {
    fn new(doc: &Value, filters: FilteringParameters) -> Self {
        let allowed_methods: HashSet<String> = filters
            .methods
            .into_iter()
            .flatten()
            .map(|m| m.to_lowercase())
            .collect();
        let mut selection = Selection {
            allowed_tags: filters.tags.into_iter().flatten().collect(),
            allowed_securities: filters.security.into_iter().flatten().collect(),
            kept_tags: HashSet::new(),
            kept_securities: HashSet::new(),
            paths: Vec::new(),
            webhooks: Vec::new(),
        };
        selection.paths =
            selection.select_container(doc, "paths", &filters.paths, &allowed_methods);
        selection.webhooks =
            selection.select_container(doc, "webhooks", &filters.webhooks, &allowed_methods);
        selection
    }

    /// Decides the fate of every entry of a `paths` or `webhooks` container (identical shape: a
    /// map from name to a Path/Webhook Item object, each potentially a `$ref`, each potentially
    /// holding HTTP-method operation entries).
    ///
    /// Vendor extension siblings (`x-*` keys) at the container level are preserved verbatim,
    /// matching the fact that the OpenAPI Paths/Webhooks Object permits extensions alongside
    /// entries.
    fn select_container(
        &mut self,
        doc: &Value,
        container: &str,
        name_patterns: &Option<Vec<String>>,
        allowed_methods: &HashSet<String>,
    ) -> Vec<EntryDecision> {
        let Some(container) = doc.get(container).and_then(Value::as_object) else {
            return Vec::new();
        };
        let name_matchers: Option<Vec<WildMatch>> = name_patterns
            .as_ref()
            .map(|patterns| patterns.iter().map(|p| WildMatch::new(p)).collect());

        container
            .iter()
            .map(|(name, item)| {
                if name.starts_with("x-") {
                    return EntryDecision::Whole;
                }
                if let Some(matchers) = &name_matchers
                    && !matchers.iter().any(|matcher| matcher.matches(name))
                {
                    return EntryDecision::Drop;
                }

                let resolved = resolve_operations_source(item, doc);
                let matching_ops: Vec<(&'static str, &Value)> = resolved
                    .as_object()
                    .map(|obj| {
                        HTTP_METHODS
                            .iter()
                            .filter_map(|method| obj.get(*method).map(|op| (*method, op)))
                            .filter(|(method, operation)| {
                                operation_matches(
                                    method,
                                    operation,
                                    allowed_methods,
                                    &self.allowed_tags,
                                    &self.allowed_securities,
                                )
                            })
                            .collect()
                    })
                    .unwrap_or_default();

                if matching_ops.is_empty() {
                    return EntryDecision::Drop;
                }

                collect_operation_tags(
                    matching_ops.iter().map(|(_, op)| *op).collect(),
                    &mut self.kept_tags,
                    &self.allowed_tags,
                );
                collect_operation_securities(
                    matching_ops.iter().map(|(_, op)| *op).collect(),
                    &mut self.kept_securities,
                    &self.allowed_securities,
                );

                if item.get("$ref").is_some() {
                    // Wholesale keep: the reusable path item itself is filtered as a component,
                    // not here.
                    EntryDecision::Whole
                } else {
                    EntryDecision::Methods(matching_ops.into_iter().map(|(m, _)| m).collect())
                }
            })
            .collect()
    }

    /// Builds a filtered container from its entries, which must line up one to one with the
    /// `decisions` made for them. A dropped entry may be given as a placeholder (it is never
    /// looked at); an entry kept with [`EntryDecision::Methods`] may have a placeholder for every
    /// operation that is not kept.
    fn materialize_container(
        &self,
        entries: impl Iterator<Item = (String, Value)>,
        decisions: &[EntryDecision],
    ) -> Map<String, Value> {
        let mut result = Map::new();
        for ((name, mut item), decision) in entries.zip(decisions) {
            match decision {
                EntryDecision::Drop => continue,
                EntryDecision::Whole => {}
                EntryDecision::Methods(methods) => {
                    if let Some(obj) = item.as_object_mut() {
                        for method in HTTP_METHODS {
                            if !methods.contains(&method) {
                                obj.remove(method);
                            } else if let Some(operation) = obj.get_mut(method) {
                                *operation = prune_operation(
                                    std::mem::take(operation),
                                    &self.allowed_tags,
                                    &self.allowed_securities,
                                );
                            }
                        }
                    }
                }
            }
            result.insert(name, item);
        }
        result
    }
}

/// The entries of a container in `doc`, copied as far as `decisions` keeps them: dropped entries
/// and operations that are not kept become placeholders instead of copies.
fn borrowed_entries<'a>(
    container: Option<&'a Value>,
    decisions: &'a [EntryDecision],
) -> impl Iterator<Item = (String, Value)> + 'a {
    container
        .and_then(Value::as_object)
        .into_iter()
        .flatten()
        .zip(decisions)
        .map(|((name, item), decision)| match decision {
            EntryDecision::Drop => (String::new(), Value::Null),
            EntryDecision::Whole => (name.clone(), item.clone()),
            EntryDecision::Methods(methods) => {
                let item = clone_except(item, |key| {
                    HTTP_METHODS.contains(&key) && !methods.contains(&key)
                });
                (name.clone(), item)
            }
        })
}

/// The entries of a container taken out of the document.
fn owned_entries(container: Option<Value>) -> impl Iterator<Item = (String, Value)> {
    match container {
        Some(Value::Object(map)) => map,
        _ => Map::new(),
    }
    .into_iter()
}

/// Prunes an operation's `tags` and `security` arrays down to the allowed sets when either filter
/// is active. All other fields (`requestBody`, `responses`, `parameters`, `callbacks`, `links`,
/// ...) are carried through unchanged, so any structural element under an operation
/// participates in the generic reference-closure walk without needing field-specific handling
/// here.
fn prune_operation(
    mut operation: Value,
    allowed_tags: &HashSet<String>,
    allowed_securities: &HashSet<String>,
) -> Value {
    let filter_tags = !allowed_tags.is_empty();
    let filter_securities = !allowed_securities.is_empty();
    let Some(obj) = operation.as_object_mut() else {
        return operation;
    };
    if filter_tags {
        let keep = match obj.get_mut("tags") {
            Some(Value::Array(tags)) => {
                tags.retain(|tag| tag.as_str().is_some_and(|tag| allowed_tags.contains(tag)));
                !tags.is_empty()
            }
            _ => false,
        };
        if !keep {
            obj.remove("tags");
        }
    }
    if filter_securities {
        let keep = match obj.get_mut("security") {
            Some(Value::Array(requirements)) => {
                requirements.retain_mut(|requirement| match requirement {
                    Value::Object(schemes) => {
                        schemes.retain(|scheme, _| allowed_securities.contains(scheme));
                        !schemes.is_empty()
                    }
                    _ => false,
                });
                !requirements.is_empty()
            }
            _ => false,
        };
        if !keep {
            obj.remove("security");
        }
    }
    operation
}

/// Builds the filtered `components` value: the generic reference-closure result (every category,
/// including 3.1's `pathItems`), with `securitySchemes` replaced by `security_schemes` — security
/// requirements reference scheme names directly rather than via `$ref`, so they aren't reachable
/// by the generic walk and need this explicit pass, mirroring the pre-existing special-case for
/// security schemes.
fn build_components(filtered: Option<Value>, security_schemes: Option<Value>) -> Option<Value> {
    let mut components = filtered.unwrap_or(Value::Object(Map::new()));

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
fn filter_security_schemes(components: &Value, kept_securities: &HashSet<String>) -> Option<Value> {
    let schemes = components.get("securitySchemes")?.as_object()?;
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

/// Computes the generic reference-closure over every `components.*` category: the paths, as
/// segment lists relative to `components`, of every component to keep.
///
/// This walks the whole `components` value tree once (category-agnostically, via
/// `reference_collector::collect_component_refs`), builds the transitive `$ref` reachability graph
/// from the directly-referenced set (`referenced`), and keeps every entry (and everything that
/// entry itself references) reachable from it. A component category introduced by a future
/// OpenAPI revision is handled the same way, as long as it's reachable via `$ref` — no
/// category-specific code is needed here.
fn kept_component_paths(components: &Value, referenced: &HashSet<String>) -> Vec<Vec<String>> {
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
    final_references
        .iter()
        .filter(|component| component.starts_with("#/components/"))
        .filter_map(|component| pointer::ref_segments(component))
        .map(|segments| segments[1..].to_vec())
        .collect()
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
    fn prune_operation_drops_tags_and_security_fields_when_nothing_survives_the_filter() {
        let operation = json!({
            "tags": ["other"],
            "security": [{ "basic_auth": [] }],
        });
        let allowed_tags: HashSet<String> = [String::from("item")].into_iter().collect();
        let allowed_securities: HashSet<String> = [String::from("api_key")].into_iter().collect();

        let cloned = prune_operation(operation, &allowed_tags, &allowed_securities);

        assert!(cloned.get("tags").is_none());
        assert!(cloned.get("security").is_none());
    }

    /// A document exercising the corners where moving and copying could disagree: key order
    /// after removals, empty and scalar components, refs into the middle of a component, to a
    /// whole category and to `#/components`, percent-encoded and `~`-escaped names, `$ref` path
    /// items (resolved, dangling and pointing into `paths`), non-object items and operations.
    fn tricky_document() -> Value {
        json!({
            "x-top": 1,
            "tags": [{ "name": "a" }, { "name": "b" }, { "name": "c" }],
            "components": {
                "x-ext": { "keep": true },
                "schemas": {
                    "Empty": {},
                    "EmptyArr": [],
                    "Prim": 5,
                    "Deep": { "properties": { "p": { "nested": { "$ref": "#/components/schemas/Deep2" } }, "q": {} } },
                    "Deep2": { "oneOf": [{ "$ref": "#/components/schemas/Empty" }, {}, [], { "a": [[]] }] },
                    "a.b": { "$ref": "#/components/schemas/a~1b" },
                    "a/b": { "$ref": "#/components/schemas/pct%20name" },
                    "pct name": { "enum": [0, 1.5] },
                    "Unused": { "type": "string" }
                },
                "responses": {
                    "R1": { "content": { "a/b": { "schema": { "$ref": "#/components/schemas/Deep/properties/p" } } } },
                    "R2": { "x": { "$ref": "#/components/parameters" } }
                },
                "parameters": { "P1": { "name": "p1" }, "P2": { "schema": { "$ref": "#/components/schemas/a.b" } } },
                "pathItems": {
                    "Reusable": {
                        "get": { "tags": ["b"], "responses": { "200": { "$ref": "#/components/responses/R1" } } },
                        "post": { "tags": ["c"], "security": [{ "k1": [], "k2": [] }] }
                    }
                },
                "securitySchemes": { "k1": {}, "k2": {}, "k3": {} }
            },
            "paths": {
                "x-first": { "v": 1 },
                "/p1": {
                    "summary": "s",
                    "trace": { "tags": ["a"] },
                    "x-mid": true,
                    "get": {
                        "tags": ["a", "b", 7],
                        "security": [{ "k1": [] }, { "k2": [], "k3": [] }, {}, "odd"],
                        "responses": { "200": { "$ref": "#/components/responses/R2" } }
                    },
                    "parameters": [{ "$ref": "#/components/parameters/P1" }],
                    "post": { "tags": "b", "requestBody": { "$ref": "#/components/schemas/a.b" } },
                    "delete": { "tags": ["c"], "security": [{ "k3": [] }] },
                    "patch": "not an object",
                    "x-last": [1, 2]
                },
                "/p2": { "$ref": "#/components/pathItems/Reusable" },
                "/p3": { "$ref": "#/components/pathItems/Missing" },
                "/p4": "scalar item",
                "/p5": { "put": { "responses": { "default": { "$ref": "#/components" } } } },
                "/p6": { "$ref": "#/paths/~1p1" }
            },
            "webhooks": {
                "hookA": { "post": { "tags": ["a"], "x": { "$ref": "#/components/schemas/Prim" } } },
                "hookB": { "$ref": "#/components/pathItems/Reusable" },
                "x-hook": 1
            },
            "openapi": "3.1.0",
            "x-tail": null
        })
    }

    fn filter_combinations() -> Vec<FilteringParameters> {
        let strings = |items: &[&str]| Some(items.iter().map(|s| s.to_string()).collect());
        vec![
            FilteringParameters::default(),
            FilteringParameters { paths: strings(&["nomatch"]), ..Default::default() },
            FilteringParameters { paths: strings(&["/p1", "/p6", "*{*}*", "/pet*"]), ..Default::default() },
            FilteringParameters { webhooks: strings(&["hookB", "user*"]), ..Default::default() },
            FilteringParameters { methods: strings(&["GET", "post"]), ..Default::default() },
            FilteringParameters { tags: strings(&["a", "b", "items", "pet", "item"]), ..Default::default() },
            FilteringParameters { security: strings(&["k1", "k2", "ApiKeyAuth", "api_key"]), ..Default::default() },
            FilteringParameters {
                paths: strings(&["/p*", "/pet*", "/items*"]),
                methods: strings(&["get", "post", "trace"]),
                tags: strings(&["a", "c", "pet", "items"]),
                security: strings(&["k1", "k3", "petstore_auth", "ApiKeyAuth"]),
                webhooks: strings(&["hook*"]),
            },
        ]
    }

    #[test]
    fn into_filtered_produces_exactly_what_filter_by_parameters_does() {
        let mut documents = vec![tricky_document(), json!("not an object"), json!({})];
        for fixture in [
            "deep-ref-chain.json",
            "full-coverage-3.0.yaml",
            "full-coverage-3.1.yaml",
            "petstore.yaml",
            "special-component-names.json",
            "user-reference.yaml",
        ] {
            documents.push(load(&format!("tests/resources/{fixture}")));
        }

        for document in &documents {
            for (borrowed_filters, owned_filters) in
                filter_combinations().into_iter().zip(filter_combinations())
            {
                let description = format!("{borrowed_filters:?}");
                let borrowed = document.filter_by_parameters(borrowed_filters);
                let owned = document.clone().into_filtered(owned_filters);
                // Compare the serialized text: `Value` equality ignores map key order.
                assert_eq!(
                    serde_json::to_string(&owned).unwrap(),
                    serde_json::to_string(&borrowed).unwrap(),
                    "{description}"
                );
            }
        }
    }

    #[test]
    fn it_keeps_the_key_order_the_clone_then_edit_filter_produced() {
        // Pinned output of the original implementation, which cloned the document and then
        // inserted/removed keys: `Map::remove` swaps the last key into the removed one's place.
        let filtered = tricky_document().filter_by_parameters(FilteringParameters {
            methods: Some(vec![String::from("get")]),
            tags: Some(vec![String::from("a")]),
            security: Some(vec![String::from("k1")]),
            ..Default::default()
        });

        let keys = |value: &Value| value.as_object().unwrap().keys().cloned().collect::<Vec<_>>();
        assert_eq!(
            keys(&filtered),
            ["x-top", "tags", "components", "paths", "webhooks", "openapi", "x-tail"]
        );
        assert_eq!(
            keys(&filtered["paths"]["/p1"]),
            ["summary", "x-last", "x-mid", "get", "parameters"]
        );
        assert_eq!(
            filtered["paths"]["/p1"]["get"],
            json!({
                "tags": ["a"],
                "security": [{ "k1": [] }],
                "responses": { "200": { "$ref": "#/components/responses/R2" } }
            })
        );
    }
}
