//! Filtering OpenAPI-shaped documents with arbitrary parameters: no panics, and the owned
//! (`into_filtered`) and borrowed (`filter_by_parameters`) paths produce identical output,
//! key order included.
#![no_main]
use arbitrary::{Result, Unstructured};
use libfuzzer_sys::fuzz_target;
use openapiv3_filter_core::filter::openapi::{FilteringParameters, OpenAPIFilter};
use openapiv3_filter_fuzz::{patterns, value};
use serde_json::{Map, Value};

fn document(u: &mut Unstructured) -> Result<Value> {
    let mut doc = Map::new();
    for key in ["openapi", "info", "tags", "paths", "webhooks", "components", "security", "x-tail"] {
        if u.ratio(4, 5)? {
            let v = match key {
                "openapi" => Value::String("3.1.0".into()),
                _ => value(u, 5)?,
            };
            doc.insert(key.to_string(), v);
        }
    }
    Ok(Value::Object(doc))
}

fuzz_target!(|data: &[u8]| {
    let mut u = Unstructured::new(data);
    let Ok(doc) = document(&mut u) else { return };
    let Ok(raw) = (0..5).map(|_| patterns(&mut u)).collect::<Result<Vec<_>>>() else { return };
    let params = || FilteringParameters {
        paths: raw[0].clone(),
        webhooks: raw[1].clone(),
        methods: raw[2].clone(),
        tags: raw[3].clone(),
        security: raw[4].clone(),
    };
    let borrowed = doc.filter_by_parameters(params());
    let owned = doc.clone().into_filtered(params());
    assert_eq!(
        serde_json::to_string(&borrowed).unwrap(),
        serde_json::to_string(&owned).unwrap(),
        "into_filtered differs from filter_by_parameters for {doc}"
    );
});
