use serde::Deserialize;
use serde_json::Value as JsonValue;

use crate::Error;

/// Parses a JSON string into a struct.
///
/// # Arguments
///
/// * `contents` - A string slice representing the JSON string to parse.
///
/// # Returns
///
/// * `Result<T, serde_json::Error>` - A Result containing the parsed struct, or a serde_json::Error if an error occurs.
fn parse_json<T>(contents: &str) -> Result<T, serde_json::Error>
where
    T: for<'de> Deserialize<'de>,
{
    serde_json::from_str(contents)
}

/// Parses a YAML string into a struct (see [`crate::yaml`] for how YAML is read).
///
/// # Arguments
///
/// * `contents` - A string slice representing the YAML string to parse.
///
/// # Returns
///
/// * `Result<T, serde_saphyr::Error>` - A Result containing the parsed struct, or a serde_saphyr::Error if an error occurs.
fn parse_yaml<T>(contents: &str) -> Result<T, serde_saphyr::Error>
where
    T: for<'de> Deserialize<'de>,
{
    crate::yaml::from_str(contents)
}

/// Checks whether a document's top-level `openapi` field declares a supported
/// major.minor version (`3.0` or `3.1`). Patch versions are not enumerated, so
/// any `3.0.x`/`3.1.x` patch release is accepted without a code change.
///
/// # Arguments
///
/// * `document` - The parsed document value to check.
///
/// # Returns
///
/// * `Result<(), String>` - `Ok` if the version is supported, or an error message otherwise.
pub fn validate_openapi_version(document: &JsonValue) -> Result<(), String> {
    match document.get("openapi").and_then(JsonValue::as_str) {
        None => Err(String::from(
            "Missing required 'openapi' field: unable to determine OpenAPI version",
        )),
        Some(version) => {
            let major_minor = version.split('.').take(2).collect::<Vec<_>>().join(".");
            if major_minor == "3.0" || major_minor == "3.1" {
                Ok(())
            } else {
                Err(format!(
                    "Unsupported OpenAPI version '{}': only 3.0.x and 3.1.x documents are supported",
                    version
                ))
            }
        }
    }
}

/// Parses document content already in memory, attempting to parse it as YAML first, then as
/// JSON. The parsed value is then checked for a supported `openapi` version before being
/// returned.
///
/// This function performs no file or stdin access itself: acquiring the bytes to pass in here
/// (reading a file, reading stdin, ...) is the caller's responsibility.
///
/// # Arguments
///
/// * `contents` - The document content to parse.
///
/// # Returns
///
/// * `Result<Value, Error>` - A Result containing the parsed value tree, or an error if parsing
///   or version validation fails.
pub fn parse(contents: &str) -> Result<JsonValue, Error> {
    let value = match parse_yaml::<JsonValue>(contents) {
        Ok(result) => result,
        Err(_) => parse_json::<JsonValue>(contents)?,
    };
    validate_openapi_version(&value).map_err(Error::UnsupportedVersion)?;
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn it_accepts_3_0_patch_versions() {
        let doc = json!({ "openapi": "3.0.3" });
        assert!(validate_openapi_version(&doc).is_ok());
    }

    #[test]
    fn it_accepts_3_1_patch_versions() {
        let doc = json!({ "openapi": "3.1.0" });
        assert!(validate_openapi_version(&doc).is_ok());
    }

    #[test]
    fn it_accepts_unenumerated_patch_versions() {
        let doc = json!({ "openapi": "3.1.7" });
        assert!(validate_openapi_version(&doc).is_ok());
    }

    #[test]
    fn it_rejects_missing_version_field() {
        let doc = json!({ "info": {} });
        assert!(validate_openapi_version(&doc).is_err());
    }

    #[test]
    fn it_rejects_swagger_2_0() {
        let doc = json!({ "openapi": "2.0" });
        assert!(validate_openapi_version(&doc).is_err());
    }

    #[test]
    fn it_rejects_future_major_version() {
        let doc = json!({ "openapi": "4.0.0" });
        assert!(validate_openapi_version(&doc).is_err());
    }

    #[test]
    fn it_parses_json_content_directly() {
        let value: JsonValue = parse_json(r#"{"openapi": "3.0.3"}"#).unwrap();
        assert_eq!(value, json!({ "openapi": "3.0.3" }));
    }

    #[test]
    fn it_parses_content_independent_of_any_file_extension() {
        // core::parse works on in-memory content only: it has no notion of a file name or
        // extension at all (that's a `cli`-level presentation concern), so parsing succeeds the
        // same way regardless of what file the content happened to come from.
        let contents = std::fs::read_to_string("tests/resources/minimal-api.yml")
            .expect("fixture should be readable");
        let value = parse(&contents).expect("fixture should parse");
        assert!(value.is_object());
    }

    #[test]
    fn it_distinguishes_malformed_input_from_an_unsupported_version() {
        let unsupported = parse(r#"{"openapi": "2.0"}"#);
        assert!(matches!(unsupported, Err(Error::UnsupportedVersion(_))));

        // Content that is neither valid YAML nor valid JSON. YAML's grammar accepts almost any
        // plain text as a scalar, so an unparseable document needs an actual structural
        // violation to fail both parsers.
        let malformed = parse("{\"unterminated");
        assert!(matches!(malformed, Err(Error::Malformed(_))));
    }
}
