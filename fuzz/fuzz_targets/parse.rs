//! Arbitrary text through the public parser (JSON first, then lean YAML): it may reject the text,
//! but must not panic, and anything it accepts must survive a JSON round trip unchanged.
#![no_main]
use libfuzzer_sys::fuzz_target;
use openapiv3_filter_core::parser;

fuzz_target!(|text: &str| {
    if let Ok((value, _format)) = parser::parse_with_format(text) {
        let json = serde_json::to_string(&value).expect("a parsed value serializes");
        let again: serde_json::Value = serde_json::from_str(&json).expect("serialized JSON parses");
        assert_eq!(value, again, "JSON round trip changed the value");
    }
});
