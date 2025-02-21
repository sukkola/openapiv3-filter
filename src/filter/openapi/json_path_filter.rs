use serde_json::{Value, Map};

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
    // Convert paths into a Vec of Vec<&str> for efficient processing
    let path_parts: Vec<Vec<&str>> = paths
        .iter()
        .map(|path| path.split('.').collect())
        .collect();

    filter_value(value, &path_parts, &[])
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
pub fn filter_value(value: &Value, all_paths: &[Vec<&str>], current_path: &[&str]) -> Option<Value> {
    match value {
        Value::Object(map) => {
            let filtered_obj = filter_object(map, all_paths, current_path);
            if filtered_obj.is_empty() {
                None
            } else {
                Some(Value::Object(filtered_obj))
            }
        }
        Value::Array(arr) => {
            let filtered: Vec<Value> = arr
                .iter()
                .filter_map(|item| filter_value(item, all_paths, current_path))
                .collect();
            if filtered.is_empty() {
                None
            } else {
                Some(Value::Array(filtered))
            }
        }
        _ => {
            // Check if current path matches any of the requested paths
            if all_paths.iter().any(|path| path == current_path) {
                Some(value.clone())
            } else {
                None
            }
        }
    }
}

/// Filters the elements of a JSON object based on the provided paths.
///
/// This function iterates over the key-value pairs in a JSON object and recursively filters the values
/// based on whether their paths match the provided paths.
///
/// # Arguments
///
/// * `map` - A reference to the JSON object (Map<String, Value>) to filter.
/// * `all_paths` - A slice of Vec<&str> representing all the paths to filter by.
/// * `current_path` - A slice of string slices representing the current path being traversed.
///
/// # Returns
///
/// * `Map<String, Value>` - A new JSON object containing only the filtered key-value pairs.
fn filter_object(map: &Map<String, Value>, all_paths: &[Vec<&str>], current_path: &[&str]) -> Map<String, Value> {
    let mut result = Map::new();

    for (key, value) in map {
        let mut new_path = current_path.to_vec();
        new_path.push(key);

        // Check if this path or any subpath is in our target paths
        let path_relevant = all_paths.iter().any(|path| {
            path.len() >= new_path.len() &&
            path[..new_path.len()] == new_path[..]
        });

        if path_relevant {
            if let Some(filtered_value) = filter_value(value, all_paths, &new_path) {
                result.insert(key.clone(), filtered_value);
            }
        } else if all_paths.iter().any(|path| path.eq(current_path)){
             result.insert(key.clone(), value.clone());
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
            "user.orders.item"
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
}
