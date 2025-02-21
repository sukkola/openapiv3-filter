use std::collections::{HashMap, HashSet};

/// Constructs reference path arrays from all the component references.
///
/// This function recursively explores a map of component references to build a vector of reference paths.
/// It uses depth-first search (DFS) to traverse the map and identify all possible paths from a given key.
/// The function also handles cyclic references by keeping track of visited keys in the current path.
///
/// # Arguments
///
/// * `map` - A reference to a `HashMap<String, Vec<String>>` representing the component references.
///   The keys are component names, and the values are vectors of component names that the key component references.
/// * `key` - A string slice representing the starting component name for the path.
/// * `current_path` - A vector of strings representing the current path being explored.
/// * `visited` - A mutable reference to a `HashSet<String>` to track visited keys in the current path.
/// * `result` - A mutable reference to a `Vec<Vec<String>>` to store the resulting reference paths.
fn collect_reference_paths(
    map: &HashMap<String, Vec<String>>,
    key: &str,
    current_path: Vec<String>,
    visited: &mut HashSet<String>,
    result: &mut Vec<Vec<String>>,
) {
    // If we already visited this key in the same path, avoid infinite loops
    if visited.contains(key) {
        return;
    }

    // Add key to visited set to prevent cycles
    visited.insert(key.to_string());

    // Create new path with current key
    let mut new_path = current_path.clone();
    new_path.push(key.to_string());

    // Add the path to the result
    result.push(new_path.clone());

    // Recursively explore references in the map
    if let Some(references) = map.get(key) {
        for ref_key in references {
            collect_reference_paths(map, ref_key, new_path.clone(), visited, result);
        }
    }

    // Backtrack: Remove from visited to allow other paths to use it
    visited.remove(key);
}

/// Generates all possible reference paths from a map of component references.
///
/// This function iterates through the keys of the provided map and initiates a depth-first search (DFS)
/// from each key to construct all possible reference paths. The resulting paths are stored in a vector of vectors.
///
/// # Arguments
///
/// * `map` - A reference to a `HashMap<String, Vec<String>>` representing the component references.
///   The keys are component names, and the values are vectors of component names that the key component references.
///
/// # Returns
///
/// * `Vec<Vec<String>>` - A vector of vectors, where each inner vector represents a reference path.
fn reference_paths(map: &HashMap<String, Vec<String>>) -> Vec<Vec<String>> {
    let mut result = Vec::new();

    // Start DFS traversal from each key
    for key in map.keys() {
        let mut visited = HashSet::new();
        collect_reference_paths(map, key, Vec::new(), &mut visited, &mut result);
    }

    result
}

/// Filters out references that are not needed according to filtering parameters.
///
/// This function takes a map of component references and a set of referenced components as input.
/// It filters the reference paths to include only those that start with a component in the set of referenced components.
/// The function returns a set of all unique references that are kept after filtering.
///
/// # Arguments
///
/// * `map` - A reference to a `HashMap<String, Vec<String>>` representing the component references.
///   The keys are component names, and the values are vectors of component names that the key component references.
/// * `referenced_components` - A reference to a `HashSet<String>` containing the names of the referenced components.
///
/// # Returns
///
/// * `HashSet<String>` - A set of all unique references that are kept after filtering.
pub fn get_kept_references(map: &HashMap<String, Vec<String>>,referenced_components: &HashSet<String>)-> HashSet<String>{
    let mut kept_references = reference_paths(map).iter()
        .filter(|reference_path|!reference_path.is_empty())
        .filter(|reference_path|referenced_components.contains(&reference_path[0]))
        .flatten()
        .cloned().collect::<HashSet<_>>();
    kept_references.extend(referenced_components.iter().cloned().collect::<HashSet<String>>());
    kept_references
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_collects_multi_level_references() {

        let mut map = HashMap::new();
         map.insert("A".to_string(), vec!["B".to_string(), "C".to_string()]);
         map.insert("B".to_string(), vec!["D".to_string()]);
         map.insert("C".to_string(), vec!["A".to_string()]); // Cyclic reference
         map.insert("D".to_string(), vec![]);
         map.insert("E".to_string(), vec!["F".to_string()]); // Unrelated component
         map.insert("F".to_string(), vec!["G".to_string()]);
         map.insert("G".to_string(), vec![]);
         let long_vector = vec![String::from("C"), String::from("A"), String::from("B"), String::from("D")];
         let short_vector = vec![String::from("E"), String::from("F"), String::from("G")];
         let result =reference_paths(&map);

        assert!(contains_all(&result,&vec![long_vector,short_vector]));
    }

    #[test]
    fn it_filters_out_non_referenced_paths() {

        let mut map = HashMap::new();
         map.insert("A".to_string(), vec!["B".to_string(), "C".to_string()]);
         map.insert("B".to_string(), vec!["D".to_string()]);
         map.insert("C".to_string(), vec!["A".to_string()]); // Cyclic reference
         map.insert("D".to_string(), vec![]);
         map.insert("E".to_string(), vec!["F".to_string()]); // Unrelated component
         map.insert("F".to_string(), vec!["G".to_string()]);
         map.insert("G".to_string(), vec![]);
         let result = get_kept_references(&map,&vec![String::from("E"),String::from("D")].iter().map(|item|item.clone()).collect::<HashSet<_>>());

        assert_eq!(result.len(), 4 );
        assert!(result.contains("D"));
        assert!(result.contains("E"));
        assert!(result.contains("F"));
        assert!(result.contains("G"));
    }

    fn contains_all(vec_of_vecs: &Vec<Vec<String>>, target_vec_of_vecs: &Vec<Vec<String>>) -> bool {
        // Check if each target vector exists in the vec_of_vecs
        target_vec_of_vecs.iter().all(|target| vec_of_vecs.contains(target))
    }
}
