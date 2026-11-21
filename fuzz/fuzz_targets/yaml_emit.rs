//! Emitting: the lean emitter must produce exactly serde-saphyr's output, and what it writes must
//! read back as the same value with both parsers.
#![no_main]
use arbitrary::Unstructured;
use libfuzzer_sys::fuzz_target;
use openapiv3_filter_core::yaml;
use openapiv3_filter_fuzz::value;
use serde_json::Value;

fuzz_target!(|data: &[u8]| {
    let mut u = Unstructured::new(data);
    let Ok(v) = value(&mut u, 4) else { return };
    let lean = yaml::lean::to_string(&v);
    let saphyr = yaml::to_string(&v);
    let (lean, saphyr) = match (lean, saphyr) {
        (Ok(a), Ok(b)) => (a, b),
        (Err(_), Err(_)) => return,
        (a, b) => panic!("one emitter failed\nvalue: {v}\nlean: {a:?}\nsaphyr: {b:?}"),
    };
    assert_eq!(lean, saphyr, "emitters differ for {v}");
    let back = yaml::lean::parse(&lean)
        .unwrap_or_else(|e| panic!("lean output does not parse: {e}\nvalue: {v}\nyaml:\n{lean}"));
    assert_eq!(back, v, "lean round trip changed the value\nyaml:\n{lean}");
    let back = yaml::from_str::<Value>(&lean)
        .unwrap_or_else(|e| panic!("saphyr cannot read lean output: {e}\nvalue: {v}\nyaml:\n{lean}"));
    assert_eq!(back, v, "saphyr round trip changed the value\nyaml:\n{lean}");
});
