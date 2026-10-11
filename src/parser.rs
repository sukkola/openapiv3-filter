use serde::Deserialize;
use serde_json::Value as JsonValue;
use serde_yaml::Value as YamlValue;
use std::fs;
use std::io::{self, Read};

#[derive(Debug)]
pub enum ParsedType<T> {
    Json(T),
    Yaml(T),
}

/// Reads the contents of a file into a String.
///
/// # Arguments
///
/// * `file_name` - A string slice representing the name of the file to read.
///
/// # Returns
///
/// * `io::Result<String>` - A Result containing the file contents as a String, or an io::Error if an error occurs.
fn read_file(file_name: &str) -> io::Result<String> {
    let mut file = fs::File::open(file_name)?; // Open the file
    let mut contents = String::new();
    file.read_to_string(&mut contents)?; // Read the contents into a String
    Ok(contents)
}

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

/// Parses a YAML string into a struct.
///
/// # Arguments
///
/// * `contents` - A string slice representing the YAML string to parse.
///
/// # Returns
///
/// * `Result<T, serde_yaml::Error>` - A Result containing the parsed struct, or a serde_yaml::Error if an error occurs.
fn parse_yaml<T>(contents: &str) -> Result<T, serde_yaml::Error>
where
    T: for<'de> Deserialize<'de>,
{
    serde_yaml::from_str(contents)
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

/// Parses a document from a file or stdin, attempting to parse it as YAML first, then as JSON.
/// The parsed value is then checked for a supported `openapi` version before being returned.
///
/// # Arguments
///
/// * `file_name` - A string slice representing the name of the file to read, or "-" for stdin.
///
/// # Returns
///
/// * `Result<ParsedType<JsonValue>, Box<dyn std::error::Error>>` - A Result containing the parsed
///   value tree, or an error if parsing or version validation fails.
pub fn parse_document(
    file_name: &str,
) -> Result<ParsedType<JsonValue>, Box<dyn std::error::Error>> {
    let data = match file_name {
        "-" => std::io::read_to_string(std::io::stdin()),
        _ => read_file(file_name),
    };
    match data {
        Ok(contents) => match parse_yaml::<JsonValue>(&contents) {
            Ok(result) => validate_and_wrap(result, file_name, "yaml", &contents),
            Err(_) => match parse_json::<JsonValue>(&contents) {
                Ok(result) => validate_and_wrap(result, file_name, "json", &contents),
                Err(err) => Err(Box::new(err)),
            },
        },
        Err(e) => Err(Box::new(e)),
    }
}

fn validate_and_wrap(
    result: JsonValue,
    file_name: &str,
    default_type: &str,
    input: &str,
) -> Result<ParsedType<JsonValue>, Box<dyn std::error::Error>> {
    validate_openapi_version(&result)?;
    Ok(wrap_response_type(result, file_name, default_type, input))
}

fn wrap_response_type<T>(
    response: T,
    file_name: &str,
    default_type: &str,
    input: &str,
) -> ParsedType<T> {
    let file_postfix = if file_name != "-" {
        file_name.split(".").last().or(Some(default_type))
    } else {
        detect_format(input)
    };
    match file_postfix {
        Some("yml") => ParsedType::Yaml(response),
        Some("yaml") => ParsedType::Yaml(response),
        Some("json") => ParsedType::Json(response),
        _ => ParsedType::Yaml(response),
    }
}

/// If output format cannot be resolved by inputs filename, function can used to detect it from contents
///
/// # Arguments
///
/// * `input` - Contents of the openapi document to be filtered
///
/// # Returns
///
/// * `Option<&'static str>` - Whether the output content can be interpreted from the content
fn detect_format(input: &str) -> Option<&'static str> {
    if serde_json::from_str::<JsonValue>(input).is_ok() {
        return Some("json");
    }
    if serde_yaml::from_str::<YamlValue>(input).is_ok() {
        return Some("yaml");
    }
    None
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
    fn it_defaults_to_yaml_for_an_unrecognized_extension() {
        let wrapped = wrap_response_type(json!({ "a": 1 }), "api.spec", "yaml", "");
        assert!(matches!(wrapped, ParsedType::Yaml(_)));
    }
}
