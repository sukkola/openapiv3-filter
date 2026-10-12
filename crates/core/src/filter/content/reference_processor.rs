use std::collections::{HashMap, HashSet};

/// Computes the set of components to keep: every referenced component plus everything reachable
/// from it through the component reference graph.
///
/// This is a plain graph reachability walk (an iterative depth-first search with a visited set),
/// so every component and every edge is processed at most once: `O(components + references)`.
/// Cycles terminate naturally because a component already in the kept set is never expanded again.
///
/// An earlier implementation enumerated every simple path from every component and only then
/// discarded the paths that did not start at a referenced component. The number of simple paths in
/// a graph with shared components grows exponentially, so on large real-world documents (e.g.
/// Stripe's ~8 MB spec) it ran for minutes and exhausted memory, and it did so regardless of the
/// filters because the enumeration happened before filtering.
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
pub fn get_kept_references(
    map: &HashMap<String, Vec<String>>,
    referenced_components: &HashSet<String>,
) -> HashSet<String> {
    let mut kept_references = referenced_components.clone();
    let mut pending: Vec<&str> = referenced_components.iter().map(String::as_str).collect();

    while let Some(key) = pending.pop() {
        for reference in map.get(key).into_iter().flatten() {
            if kept_references.insert(reference.clone()) {
                pending.push(reference);
            }
        }
    }

    kept_references
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    fn graph(edges: &[(&str, &[&str])]) -> HashMap<String, Vec<String>> {
        edges
            .iter()
            .map(|(from, to)| {
                (
                    from.to_string(),
                    to.iter().map(|target| target.to_string()).collect(),
                )
            })
            .collect()
    }

    fn set(items: &[&str]) -> HashSet<String> {
        items.iter().map(|item| item.to_string()).collect()
    }

    #[test]
    fn it_follows_multi_level_references_through_cycles() {
        let map = graph(&[
            ("A", &["B", "C"]),
            ("B", &["D"]),
            ("C", &["A"]), // Cyclic reference
            ("D", &[]),
            ("E", &["F"]), // Unrelated component
            ("F", &["G"]),
            ("G", &[]),
        ]);

        let result = get_kept_references(&map, &set(&["C"]));

        assert_eq!(result, set(&["A", "B", "C", "D"]));
    }

    #[test]
    fn it_filters_out_non_referenced_paths() {
        let map = graph(&[
            ("A", &["B", "C"]),
            ("B", &["D"]),
            ("C", &["A"]), // Cyclic reference
            ("D", &[]),
            ("E", &["F"]), // Unrelated component
            ("F", &["G"]),
            ("G", &[]),
        ]);

        let result = get_kept_references(&map, &set(&["E", "D"]));

        assert_eq!(result.len(), 4);
        assert_eq!(result, set(&["D", "E", "F", "G"]));
    }

    #[test]
    fn it_keeps_referenced_components_that_have_no_entry_in_the_graph() {
        let map = graph(&[("A", &["B"]), ("B", &[])]);

        let result = get_kept_references(&map, &set(&["Z"]));

        assert_eq!(result, set(&["Z"]));
    }

    #[test]
    fn it_keeps_nothing_when_nothing_is_referenced() {
        let map = graph(&[("A", &["B"]), ("B", &[])]);

        assert!(get_kept_references(&map, &HashSet::new()).is_empty());
    }

    #[test]
    fn it_terminates_on_self_references() {
        let map = graph(&[("A", &["A"])]);

        assert_eq!(get_kept_references(&map, &set(&["A"])), set(&["A"]));
    }

    /// A chain of diamonds (`N{i}` references both `L{i}` and `R{i}`, which both reference
    /// `N{i+1}`) has 2^depth distinct simple paths. Enumerating them took exponential time and
    /// memory; reachability must stay linear.
    #[test]
    fn it_is_linear_on_graphs_with_exponentially_many_paths() {
        let depth = 200;
        let mut map = HashMap::new();
        for i in 0..depth {
            map.insert(format!("N{i}"), vec![format!("L{i}"), format!("R{i}")]);
            map.insert(format!("L{i}"), vec![format!("N{}", i + 1)]);
            map.insert(format!("R{i}"), vec![format!("N{}", i + 1)]);
        }
        map.insert(format!("N{depth}"), vec![]);

        let started = Instant::now();
        let result = get_kept_references(&map, &set(&["N0"]));

        assert_eq!(result.len(), 3 * depth + 1);
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "reachability took {:?}",
            started.elapsed()
        );
    }
}
