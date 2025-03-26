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

/// Parses a document from a file or stdin, attempting to parse it as YAML first, then as JSON.
///
/// # Arguments
///
/// * `file_name` - A string slice representing the name of the file to read, or "-" for stdin.
///
/// # Returns
///
/// * `Result<ParsedType<T>, Box<dyn std::error::Error>>` - A Result containing the parsed struct, or an error if parsing fails.
pub fn parse_document<T>(file_name: &str) -> Result<ParsedType<T>, Box<dyn (std::error::Error)>>
where
    T: for<'de> Deserialize<'de>,
{
    let data = match file_name {
        "-" => std::io::read_to_string(std::io::stdin()),
        _ => read_file(file_name),
    };
    match data {
        Ok(contents) => match parse_yaml(&contents) {
            Ok(result) => Ok(wrap_response_type(result, file_name, "yaml", &contents)),
            Err(_) => match parse_json(&contents) {
                Ok(result) => Ok(wrap_response_type(result, file_name, "json", &contents)),
                Err(err) => Err(Box::new(err)),
            },
        },
        Err(e) => Err(Box::new(e)),
    }
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
