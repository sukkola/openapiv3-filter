use std::collections::HashMap;

use serde_json::{Map, Value};

/// Finds paths in a JSON value that match the provided paths.
///
/// This function takes a JSON value and a slice of path strings as input.
/// It filters the JSON value to include only the parts that match the provided paths.
///
/// # Arguments
///
/// * `value` - A reference to the JSON value to filter.
/// * `paths` - A slice of string slices representing the paths to filter by. Each path is a dot-separated string.
///
/// # Returns
///
/// * `Option<Value>` - An Option containing the filtered JSON value, or None if no paths match.
pub fn filter_json(value: &Value, paths: &[&str]) -> Option<Value> {
    let trie = PathTrie::from_paths(paths.iter().map(|path| path.split('.')));
    filter_node(value, &trie)
}

/// Like [`filter_json`], but each path is given as a list of already-separated segments, so keys
/// that themselves contain `.` (e.g. the component name `issuing.authorization`) are matched as a
/// single key instead of being split into several.
///
/// # Returns
///
/// * `Option<Value>` - An Option containing the filtered JSON value, or None if no paths match.
pub fn filter_json_segments(value: &Value, paths: &[Vec<String>]) -> Option<Value> {
    let trie = PathTrie::from_paths(paths.iter().map(|path| path.iter().map(String::as_str)));
    filter_node(value, &trie)
}

/// Recursively filters a JSON value based on the provided paths.
///
/// This function recursively traverses the JSON value, filtering it based on the provided paths.
/// It handles objects, arrays, and primitive values.
///
/// # Arguments
///
/// * `value` - A reference to the JSON value to filter.
/// * `all_paths` - A slice of Vec<&str> representing all the paths to filter by.
/// * `current_path` - A slice of string slices representing the current path being traversed.
///
/// # Returns
///
/// * `Option<Value>` - An Option containing the filtered JSON value, or None if no paths match at this level.
pub fn filter_value(
    value: &Value,
    all_paths: &[Vec<&str>],
    current_path: &[&str],
) -> Option<Value> {
    let trie = PathTrie::from_paths(all_paths.iter().map(|path| path.iter().copied()));
    let mut node = &trie;
    for segment in current_path {
        // No path runs through `current_path`: nothing below it can match.
        node = node.children.get(segment)?;
    }
    filter_node(value, node)
}

/// The set of filter paths as a prefix tree, so that "is the current path one of the requested
/// paths" and "does any requested path continue through this key" are single lookups instead of
/// a scan over every path at every key.
#[derive(Default)]
struct PathTrie<'a> {
    /// Some requested path ends exactly here.
    terminal: bool,
    children: HashMap<&'a str, PathTrie<'a>>,
}

impl<'a> PathTrie<'a> {
    fn from_paths<P, S>(paths: P) -> Self
    where
        P: IntoIterator<Item = S>,
        S: IntoIterator<Item = &'a str>,
    {
        let mut root = PathTrie::default();
        for path in paths {
            let mut node = &mut root;
            for segment in path {
                node = node.children.entry(segment).or_default();
            }
            node.terminal = true;
        }
        root
    }
}

/// Filters `value`, which sits at the path `node` represents.
fn filter_node(value: &Value, node: &PathTrie) -> Option<Value> {
    match value {
        Value::Object(map) => {
            let filtered_obj = filter_object(map, node);
            if filtered_obj.is_empty() {
                None
            } else {
                Some(Value::Object(filtered_obj))
            }
        }
        Value::Array(arr) => {
            let filtered: Vec<Value> = arr
                .iter()
                .filter_map(|item| filter_node(item, node))
                .collect();
            if filtered.is_empty() {
                None
            } else {
                Some(Value::Array(filtered))
            }
        }
        // A scalar is kept only when the current path is one of the requested paths.
        _ => node.terminal.then(|| value.clone()),
    }
}

/// Filters the elements of a JSON object: a key some requested path continues through is
/// filtered recursively, and any other key is copied whole when the object's own path is one of
/// the requested paths (and dropped otherwise).
fn filter_object(map: &Map<String, Value>, node: &PathTrie) -> Map<String, Value> {
    if node.children.is_empty() {
        // Nothing to narrow: every entry is copied whole, or none is.
        return if node.terminal {
            map.clone()
        } else {
            Map::new()
        };
    }

    let mut result = Map::new();
    for (key, value) in map {
        match node.children.get(key.as_str()) {
            Some(child) => {
                if let Some(filtered_value) = filter_node(value, child) {
                    result.insert(key.clone(), filtered_value);
                }
            }
            None if node.terminal => {
                result.insert(key.clone(), value.clone());
            }
            None => {}
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn it_does_basic_filtering() {
        let json = json!({
            "user": {
                "name": "John",
                "age": 30,
                "address": {
                    "street": "123 Main St",
                    "city": "Springfield",
                    "country": "USA"
                },
                "orders": [
                    {
                        "id": 1,
                        "item": "Book",
                        "price": 29.99
                    },
                    {
                        "id": 2,
                        "item": "Pen",
                        "price": 4.99
                    }
                ]
            },
            "metadata": {
                "timestamp": "2024-01-31",
                "version": "1.0"
            }
        });

        let paths = vec![
            "user.name",
            "user.address.city",
            "user.orders.id",
            "user.orders.item",
        ];

        let filtered = filter_json(&json, &paths).unwrap();

        // Expected filtered structure
        let expected = json!({
            "user": {
                "name": "John",
                "address": {
                    "city": "Springfield"
                },
                "orders": [
                    {
                        "id": 1,
                        "item": "Book"
                    },
                    {
                        "id": 2,
                        "item": "Pen"
                    }
                ]
            }
        });

        assert_eq!(filtered, expected);
    }

    #[test]
    fn it_works_with_empty_paths() {
        let json = json!({
            "a": 1,
            "b": 2
        });

        let paths: Vec<&str> = vec![];
        let filtered = filter_json(&json, &paths);
        assert!(filtered.is_none());
    }

    #[test]
    fn it_works_with_paths_that_do_not_exist() {
        let json = json!({
            "a": 1,
            "b": 2
        });

        let paths = vec!["c.d", "e.f"];
        let filtered = filter_json(&json, &paths);
        assert!(filtered.is_none());
    }

    #[test]
    fn it_works_with_arrays() {
        let json = json!({
            "items": [
                {"id": 1, "name": "Item 1", "desc": "Description 1"},
                {"id": 2, "name": "Item 2", "desc": "Description 2"}
            ]
        });

        let paths = vec!["items.id", "items.name"];
        let filtered = filter_json(&json, &paths).unwrap();

        let expected = json!({
            "items": [
                {"id": 1, "name": "Item 1"},
                {"id": 2, "name": "Item 2"}
            ]
        });

        assert_eq!(filtered, expected);
    }

    #[test]
    fn it_works_with_nested_objects() {
        let json = json!({
            "level1": {
                "level2": {
                    "level3": {
                        "value": "deep",
                        "other": "not needed"
                    }
                }
            }
        });

        let paths = vec!["level1.level2.level3.value"];
        let filtered = filter_json(&json, &paths).unwrap();

        let expected = json!({
            "level1": {
                "level2": {
                    "level3": {
                        "value": "deep"
                    }
                }
            }
        });

        assert_eq!(filtered, expected);
    }

    #[test]
    fn it_filters_embedded_json_objects() {
        let json = json!({
            "level1": {
                "level2": {
                    "level3": {
                        "value": "deep",
                        "other": "needed"
                    }
                }
            }
        });

        let paths = vec!["level1.level2.level3"];

        let filtered = filter_json(&json, &paths).unwrap();

        let expected = json!({
            "level1": {
                "level2": {
                    "level3": {
                        "value": "deep",
                        "other": "needed"
                    }
                }
            }
        });

        assert_eq!(filtered, expected);
    }

    #[test]
    fn it_matches_segment_paths_whose_keys_contain_dots() {
        let json = json!({
            "schemas": {
                "issuing.authorization": { "type": "object" },
                "issuing": { "authorization": "decoy" },
                "Other": { "type": "string" }
            }
        });

        let paths = vec![vec!["schemas".to_string(), "issuing.authorization".to_string()]];
        let filtered = filter_json_segments(&json, &paths).unwrap();

        assert_eq!(
            filtered,
            json!({ "schemas": { "issuing.authorization": { "type": "object" } } })
        );
        // The dot-separated form cannot express that key: it addresses the nested decoy instead.
        let dotted = filter_json(&json, &["schemas.issuing.authorization"]).unwrap();
        assert_eq!(dotted, json!({ "schemas": { "issuing": { "authorization": "decoy" } } }));
    }

    #[test]
    fn it_drops_an_empty_object_or_array_at_a_matched_path() {
        let json = json!({ "s": { "Empty": {}, "EmptyArr": [], "Full": { "a": {} } } });

        let filtered = filter_json(&json, &["s.Empty", "s.EmptyArr", "s.Full"]).unwrap();

        // Children of a matched object are copied as-is (even empty ones); the matched value
        // itself is dropped when empty.
        assert_eq!(filtered, json!({ "s": { "Full": { "a": {} } } }));
        assert_eq!(filter_json(&json, &["s.Empty"]), None);
    }

    #[test]
    fn it_prunes_empty_items_of_an_array_at_a_matched_path() {
        let json = json!({ "s": { "A": [{}, [], 1, { "x": {} }, [[]], [2]] } });

        let filtered = filter_json(&json, &["s.A"]).unwrap();

        assert_eq!(filtered, json!({ "s": { "A": [1, { "x": {} }, [2]] } }));
    }

    #[test]
    fn it_narrows_a_matched_prefix_when_a_longer_path_also_runs_through_it() {
        // `s` alone would keep everything under it, but because `s.A.p` also passes through `s.A`,
        // `s.A` is filtered down to `p` instead of being copied wholesale. `s.B` (not on the longer
        // path) is still copied whole.
        let json = json!({ "s": { "A": { "p": { "k": 1 }, "q": 2 }, "B": { "r": 3 } } });

        let filtered = filter_json(&json, &["s", "s.A.p"]).unwrap();

        assert_eq!(filtered, json!({ "s": { "A": { "p": { "k": 1 } }, "B": { "r": 3 } } }));
    }

    #[test]
    fn it_keeps_everything_for_an_empty_path() {
        let json = json!({ "a": { "b": 1 }, "c": [] });

        assert_eq!(filter_json_segments(&json, &[vec![]]), Some(json.clone()));
        assert_eq!(filter_json_segments(&json!({}), &[vec![]]), None);
    }

    #[test]
    fn it_keeps_a_scalar_only_at_an_exact_path() {
        let json = json!({ "a": 1, "b": { "c": 2 } });

        assert_eq!(filter_json(&json, &["a"]), Some(json!({ "a": 1 })));
        assert_eq!(filter_json(&json, &["a.deeper"]), None);
        assert_eq!(filter_json(&json, &["b.c", "a"]), Some(json));
    }
}
