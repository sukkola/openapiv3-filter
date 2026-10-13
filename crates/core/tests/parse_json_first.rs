//! `parser::parse` tries JSON before YAML. That must only be an optimisation: for every JSON
//! document the result is the same value, in the same key order, as reading it as YAML.
//!
//! Set `YAML_DIFF_DIR` to a directory of extra `.json` documents (e.g. large public API
//! descriptions) to check those too.

use openapiv3_filter_core::{parser, yaml};
use serde_json::Value;

fn ordered(v: &Value) -> String {
    serde_json::to_string(v).unwrap()
}

fn json_documents() -> Vec<std::path::PathBuf> {
    let mut dirs = vec![std::path::PathBuf::from("tests/resources")];
    if let Ok(dir) = std::env::var("YAML_DIFF_DIR") {
        dirs.push(dir.into());
    }
    dirs.into_iter()
        .filter_map(|dir| std::fs::read_dir(dir).ok())
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
        .collect()
}

#[test]
fn json_first_parsing_agrees_with_yaml_parsing_of_json_documents() {
    std::thread::Builder::new()
        .stack_size(64 * 1024 * 1024)
        .spawn(|| {
            let documents = json_documents();
            assert!(!documents.is_empty(), "no JSON fixtures found");
            for path in documents {
                let text = std::fs::read_to_string(&path).unwrap();
                let via_yaml = yaml::parse_value(&text).expect("JSON is valid YAML");
                if parser::validate_openapi_version(&via_yaml).is_err() {
                    continue; // not an OpenAPI document; `parse` would reject it either way
                }
                let via_parse = parser::parse(&text).unwrap();
                assert_eq!(
                    ordered(&via_parse),
                    ordered(&via_yaml),
                    "{}: JSON-first and YAML parsing differ",
                    path.display()
                );
            }
        })
        .unwrap()
        .join()
        .unwrap();
}
