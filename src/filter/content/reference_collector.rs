use serde_json::Value;
use std::collections::{HashMap,HashSet};
use openapiv3::Operation;

/// Recursively collects `$ref` object keys from the API specification under a given path.
///
/// This function traverses the JSON-like `Value` to find all occurrences of `$ref`. When a `$ref` is found,
/// its string value (the reference path) is added to the `refs` HashSet. This is used to gather all
/// component references within a specific path of the OpenAPI document.
///
/// # Arguments
///
/// * `value` - A reference to the `Value` (JSON-like structure) to traverse.
/// * `refs` - A mutable reference to a `HashSet<String>` to store the collected `$ref` values.
/// * `key_name` - An optional reference to a `String` representing the key of the current value being processed.
pub fn collect_path_refs(value: &Value, refs: &mut HashSet<String>,key_name: Option<&String>) {
    match value {
        Value::Object(map) => {
            // Check if this object has a $ref key
            if let Some(ref_value) = map.get("$ref") {
                if let Some(ref_str) = ref_value.as_str() {
                    refs.insert(ref_str.to_string());
                }
            }

            // Recurse into all object values
            for (k, v) in map {
                collect_path_refs(v, refs,Some(k));
            }
        }
        Value::Array(arr) => {
            // Recurse into array elements
            for item in arr {
                collect_path_refs(item, refs,None);
            }
        }
        value => {
            if key_name.is_some() && key_name.unwrap() == "$ref"{
                if let Some(ref_str) = value.as_str() {
                            refs.insert(ref_str.to_string());

                        }
            }
        }
    }
}

/// Collects all tags from under HTTP operation elements.
///
/// This function iterates through a vector of `Operation` references and extracts all tags associated with each operation.
/// The extracted tags are then added to the provided `tags` HashSet. If `allowed_tags` is not empty, only tags
/// present in the `allowed_tags` set are collected.
///
/// # Arguments
///
/// * `operations` - A vector of references to `Operation` objects.
/// * `tags` - A mutable reference to a `HashSet<String>` to store the collected tags.
/// * `allowed_tags` - A reference to a `HashSet<String>` containing the allowed tags. If empty, all tags are collected.
pub fn collect_operation_tags(operations: Vec<&&Operation>, tags: &mut HashSet<String>,allowed_tags: &HashSet<String>) {
    let filter_tags = allowed_tags.iter().count() > 0;
    let found_tags: Vec<String> = operations.iter()
            .map(|operation|operation.tags.clone())
            .collect::<Vec<Vec<String>>>().into_iter().flatten().collect();

    tags.extend(if filter_tags  { found_tags.into_iter().filter(|item| allowed_tags.contains(item)).collect() } else { found_tags } );

}

/// Collects security definitions under operation.
///
/// This function iterates through a vector of `Operation` references and extracts all security requirements associated with each operation.
/// The extracted security requirements are then added to the provided `tags` HashSet. If `allowed_securities` is not empty, only security
/// requirements present in the `allowed_securities` set are collected.
///
/// # Arguments
///
/// * `operations` - A vector of references to `Operation` objects.
/// * `tags` - A mutable reference to a `HashSet<String>` to store the collected security requirements.
/// * `allowed_securities` - A reference to a `HashSet<String>` containing the allowed security requirements. If empty, all are collected.
pub fn collect_operation_securities(operations: Vec<&&Operation>, tags: &mut HashSet<String>,allowed_securities: &HashSet<String>) {
    let filter_securities = allowed_securities.iter().count() > 0;
    let found_securities: Vec<String> = operations.iter()
            .flat_map(|operation|operation.security.iter().flat_map(|vec_item|vec_item.iter().map(|item|item.iter().map(|(key,_)|key.clone()).collect::<String>())))
            .collect();

    tags.extend(if filter_securities  { found_securities.into_iter().filter(|item| allowed_securities.contains(item)).collect() } else { found_securities } );

}
/// Collects references from under the components element in the API specification.
///
/// This function recursively traverses the JSON-like `Value` representing the `components` section of an OpenAPI
/// specification. It identifies and collects all `$ref` values, storing them in the provided `refs` HashMap.
/// The function maintains a `current_path` to track the location of each reference within the components structure.
///
/// # Arguments
///
/// * `value` - A reference to the `Value` (JSON-like structure) representing the `components` section.
/// * `current_path` - A string slice representing the current path within the `components` structure.
/// * `refs` - A mutable reference to a `HashMap<String, Vec<String>>` to store the collected references.
///   The keys are the paths to the references, and the values are vectors of the reference strings.
/// * `allowed_key_recursion_levels` - maximum recursion levels
/// * `recursion_level` - current recursion level
pub fn collect_component_refs(value: &Value, current_path: &str, refs: &mut HashMap<String, Vec<String>>,allowed_key_recursion_levels:i8,recursion_level: i8) {

    match value {
        Value::Object(map) => {
            for (key, val) in map {
                let new_path = if current_path.is_empty() {
                    key.to_string()
                } else if recursion_level < allowed_key_recursion_levels {
                    format!("{}/{}", current_path, key)
                }else{
                    current_path.to_string()

                };

                if key == "$ref" {
                    if let Some(ref_value) = val.as_str() {
                        let key =  new_path.to_string();
                        refs.entry(key.clone())
                                .or_default()
                                .push(ref_value.to_string());
                    }
                } else {
                    collect_component_refs(val, &new_path, refs,allowed_key_recursion_levels,recursion_level+1);
                }
            }
        }
        Value::Array(array) => {
            for (index, item) in array.iter().enumerate() {
                let new_path = format!("{}/{}", current_path, index);
                collect_component_refs(item, &new_path, refs,allowed_key_recursion_levels,recursion_level+1);
            }
        }
        _ => {}
    }
}
