//! The lean YAML parser must agree with serde-saphyr: the same value in the same key order, or
//! both reject the text. Documented differences (src/yaml/lean/NOTICE.md) are skipped.
#![no_main]
use libfuzzer_sys::fuzz_target;
use openapiv3_filter_core::yaml;
use serde_json::Value;

fn same_order(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Object(x), Value::Object(y)) => {
            x.len() == y.len()
                && x.iter().zip(y).all(|((ka, va), (kb, vb))| ka == kb && same_order(va, vb))
        }
        (Value::Array(x), Value::Array(y)) => {
            x.len() == y.len() && x.iter().zip(y).all(|(p, q)| same_order(p, q))
        }
        _ => a == b,
    }
}

fuzz_target!(|text: &str| {
    // Deliberate differences: `!!binary` and alias limits.
    if text.contains("binary") || text.contains('*') {
        return;
    }
    let lean = yaml::lean::parse(text);
    let saphyr = yaml::from_str::<Value>(text);
    match (&lean, &saphyr) {
        (Ok(a), Ok(b)) => assert!(same_order(a, b), "values differ\nlean:   {a}\nsaphyr: {b}"),
        (Err(_), Err(_)) => {}
        // A scalar document followed by a non-document is a deliberate lean-only error.
        (Err(_), Ok(Value::String(_) | Value::Number(_) | Value::Bool(_) | Value::Null)) => {}
        // serde-saphyr 1.3 ignores a syntax error after a root flow collection (`{a: 1}\nb: 2`
        // reads as `{a: 1}`): its implicit document end counts as the `...` after which trailing
        // garbage is tolerated. Lean reports the error, which is the correct reading.
        (Err(e), Ok(Value::Object(_) | Value::Array(_)))
            if e.to_string().starts_with("did not find expected <document start>") => {}
        // `!t a: 1\na: 2`: serde-saphyr sees two distinct keys (their tags differ) and the
        // second overwrites the first; lean compares the resolved strings and reports a
        // duplicate. Documented in src/yaml/mod.rs.
        (Err(e), Ok(_)) if text.contains('!') && e.to_string().starts_with("duplicate mapping key") => {}
        // serde-saphyr deliberately ignores malformed text after an explicit `...` document
        // end; lean reports it. Documented in src/yaml/mod.rs.
        (Err(_), Ok(_)) if text.contains("...") => {}
        // Number-like keys written differently (`0x70` and `0x070`): serde-saphyr compares the
        // numbers and reports a duplicate, lean compares the text. Documented in src/yaml/mod.rs.
        (Ok(_), Err(e)) if e.to_string().contains("duplicate mapping key") => {}
        _ => panic!("one parser accepted, the other rejected\nlean:   {lean:?}\nsaphyr: {saphyr:?}"),
    }
});
