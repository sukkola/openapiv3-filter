//! JSON Pointer (RFC 6901) helpers for local `$ref` strings.
//!
//! A local `$ref` such as `#/components/schemas/a~1b` is a URI fragment holding a JSON Pointer.
//! To compare references with the keys of the document they point into, both sides must be
//! brought to the same canonical form: the fragment is percent-decoded first (RFC 6901 §6), then
//! split on `/`, and each segment is unescaped (`~1` -> `/`, `~0` -> `~`, in that order). Segments
//! are kept as separate values (or re-escaped when a single string is needed) so that a key which
//! itself contains `.`, `/`, `~` or `%` can never be confused with a path separator.

use std::borrow::Cow;

/// Escapes one reference token for use inside a JSON Pointer (`~` -> `~0`, `/` -> `~1`).
pub fn escape_segment(segment: &str) -> Cow<'_, str> {
    if segment.contains(['~', '/']) {
        Cow::Owned(segment.replace('~', "~0").replace('/', "~1"))
    } else {
        Cow::Borrowed(segment)
    }
}

/// Reverses [`escape_segment`] (`~1` -> `/` first, then `~0` -> `~`, as RFC 6901 §4 requires).
pub fn unescape_segment(segment: &str) -> Cow<'_, str> {
    if segment.contains('~') {
        Cow::Owned(segment.replace("~1", "/").replace("~0", "~"))
    } else {
        Cow::Borrowed(segment)
    }
}

/// Percent-decodes a URI fragment. Malformed escapes (a `%` not followed by two hex digits) and
/// decoded bytes that are not valid UTF-8 leave the input untouched rather than failing.
fn percent_decode(input: &str) -> Cow<'_, str> {
    if !input.contains('%') {
        return Cow::Borrowed(input);
    }
    let bytes = input.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%'
            && index + 2 < bytes.len()
            && let (Some(high), Some(low)) = (hex(bytes[index + 1]), hex(bytes[index + 2]))
        {
            decoded.push(high << 4 | low);
            index += 3;
        } else {
            decoded.push(bytes[index]);
            index += 1;
        }
    }
    match String::from_utf8(decoded) {
        Ok(text) => Cow::Owned(text),
        Err(_) => Cow::Borrowed(input),
    }
}

fn hex(byte: u8) -> Option<u8> {
    (byte as char).to_digit(16).map(|digit| digit as u8)
}

/// Splits a local `$ref` (`#/a/b/c`) into its unescaped, percent-decoded segments.
/// Returns `None` for anything that is not a local pointer reference.
pub fn ref_segments(reference: &str) -> Option<Vec<String>> {
    let pointer = reference.strip_prefix("#/")?;
    Some(
        percent_decode(pointer)
            .split('/')
            .map(|segment| unescape_segment(segment).into_owned())
            .collect(),
    )
}

/// Joins segments back into the canonical `#/a/b/c` form (each segment escaped).
pub fn segments_to_ref(segments: &[String]) -> String {
    let mut reference = String::from("#");
    for segment in segments {
        reference.push('/');
        reference.push_str(&escape_segment(segment));
    }
    reference
}

/// Normalises a `$ref` string so that equivalent spellings of the same local pointer (percent
/// encoded or not) compare equal. Non-local references are returned unchanged.
pub fn canonical_ref(reference: &str) -> String {
    match ref_segments(reference) {
        Some(segments) => segments_to_ref(&segments),
        None => reference.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_escapes_and_unescapes_pointer_tokens() {
        assert_eq!(escape_segment("a/b~c"), "a~1b~0c");
        assert_eq!(unescape_segment("a~1b~0c"), "a/b~c");
        // `~01` must decode to `~1`, not `/` (RFC 6901 §4 ordering).
        assert_eq!(unescape_segment("~01"), "~1");
    }

    #[test]
    fn it_splits_refs_into_segments_without_treating_dots_as_separators() {
        assert_eq!(
            ref_segments("#/components/schemas/issuing.authorization"),
            Some(vec![
                "components".to_string(),
                "schemas".to_string(),
                "issuing.authorization".to_string()
            ])
        );
        assert_eq!(
            ref_segments("#/components/schemas/a~1b~0c")
                .unwrap()
                .last()
                .unwrap(),
            "a/b~c"
        );
    }

    #[test]
    fn it_percent_decodes_before_splitting() {
        assert_eq!(
            ref_segments("#/components/schemas/Foo%20Bar")
                .unwrap()
                .last()
                .unwrap(),
            "Foo Bar"
        );
        assert_eq!(
            ref_segments("#/components/schemas/100%25")
                .unwrap()
                .last()
                .unwrap(),
            "100%"
        );
        // RFC 6901 §6: the fragment is decoded first, so %2F is a pointer separator.
        assert_eq!(ref_segments("#/a%2Fb").unwrap(), vec!["a", "b"]);
        // `%7E1` decodes to `~1`, which is then the escape for `/`.
        assert_eq!(ref_segments("#/a%7E1b").unwrap(), vec!["a/b"]);
    }

    #[test]
    fn it_leaves_malformed_percent_sequences_alone() {
        assert_eq!(ref_segments("#/a/100%").unwrap().last().unwrap(), "100%");
        assert_eq!(ref_segments("#/a/%zz").unwrap().last().unwrap(), "%zz");
        // Invalid UTF-8 after decoding keeps the original text.
        assert_eq!(ref_segments("#/a/%FF").unwrap().last().unwrap(), "%FF");
    }

    #[test]
    fn it_canonicalises_equivalent_spellings() {
        assert_eq!(
            canonical_ref("#/components/schemas/Foo%20Bar"),
            canonical_ref("#/components/schemas/Foo Bar")
        );
        assert_eq!(canonical_ref("#/c/a%2Fb"), "#/c/a/b");
        assert_eq!(canonical_ref("#/c/a%7E1b"), "#/c/a~1b");
        assert_eq!(canonical_ref("other.yaml#/x"), "other.yaml#/x");
        assert_eq!(canonical_ref("#"), "#");
    }
}
