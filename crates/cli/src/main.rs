use clap::Parser;

use openapiv3_filter_core::filter::openapi::{FilteringParameters, OpenAPIFilter};
use serde_json::Value as JsonValue;
use std::fs;
use std::io::{self, IsTerminal, Read};
use std::process::ExitCode;

#[derive(Parser, Default)]
#[command(version,
          about = "Filters openapi v3 document contents. Keeps only content and its dependencies in the document that matches the provided filters",
          long_about = None,
          arg_required_else_help = true,
          after_help = "EXAMPLES:
              # Filter operations with get method
              openapiv3-filter api.yaml --method get

              # Filter operations with paths containing 'users'
              openapiv3-filter api.json --path '*users*'

              # Filter operations with multiple tags
              openapiv3-filter api.yaml --tag users --tag auth

              # Combine multiple filters
              openapiv3-filter api.json --path '/api/v1/*' --method post --tag admin

              # Filter webhooks by name (OpenAPI 3.1)
              openapiv3-filter api.yaml --webhook 'user*'"
)]
/// Filters document by matching specification paths
struct Opts {
    #[arg(help = "Input file or - for stdin", default_value = "-")]
    api_document: Option<String>,
    ///Matches the path name. Allows * wildcards in matching
    #[arg(
        short,
        long = "path",
        help = "full path or partial path with * wildcard depicting match for rest of the content\n\
            Examples:\n \
            --path '/pets' - Exact match\n \
            --path '/pets/*' - Match all paths under /pets\n \
            --path '*/pets' - Match all paths ending with /pets"
    )]
    path_names: Option<Vec<String>>,
    ///Matches the webhook name (OpenAPI 3.1 `webhooks`). Allows * wildcards in matching, independent of --path
    #[arg(
        short = 'w',
        long = "webhook",
        help = "webhook name or partial name with * wildcard, matched independently of --path\n\
            Examples:\n \
            --webhook 'userCreated' - Exact match\n \
            --webhook 'user*' - Match all webhooks starting with user"
    )]
    webhook_names: Option<Vec<String>>,
    #[arg(
        short = 'm',
        long = "method",
        help = "http method name used in the operation mapping\n \
            Examples:\n \
            --method 'post' - mathches post methods in API specification\n \
            --method 'post' ----method 'get' - Matches both post and get methods in document"
    )]
    http_methods: Option<Vec<String>>,
    #[arg(
        short,
        long = "tag",
        help = "tag name that is matched. Requires fully matched tag names\n \
            Examples:\n \
            --tag 'user_info' - mathches user_info tags in document\n \
            --tag 'user_info' ----method 'collection' - Matches both user_info and collection tags in document"
    )]
    tags: Option<Vec<String>>,
    #[arg(
        short,
        long = "security",
        help = "security name that is matched. Requires fully matched security names\n \
            Examples:\n \
            --security 'api_key' - mathches API document content that uses api_key security definitions\n \
            --security 'api_key' ----security 'basic_auth' - Matches both api_key and basic_auth security definitions in document"
    )]
    security: Option<Vec<String>>,
}

impl Opts {
    pub fn parse_args() -> Result<Self, Box<dyn std::error::Error>> {
        // Check if stdin has data
        let has_stdin_data = !io::stdin().is_terminal();
        // If no stdin data, use parse() which shows help on no args
        // If there is stdin data, use try_parse() which doesn't show help
        let opts = if has_stdin_data {
            match Self::try_parse() {
                Ok(opts) => opts,
                Err(_) => Self {
                    api_document: Some(String::from("-")),
                    ..Default::default()
                },
            }
        } else {
            Self::parse()
        };

        Ok(opts)
    }
}

/// Which textual format the filtered result should be printed in.
enum OutputFormat {
    Json,
    Yaml,
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

/// Acquires document content from a file, or from stdin when `file_name` is `"-"`.
///
/// This is the CLI-local counterpart of `core::parser::parse`'s "already in memory" input: the
/// engine itself never touches the filesystem or stdin.
///
/// # Arguments
///
/// * `file_name` - A string slice representing the name of the file to read, or "-" for stdin.
///
/// # Returns
///
/// * `io::Result<String>` - A Result containing the acquired contents as a String, or an
///   io::Error if an error occurs.
fn read_input(file_name: &str) -> io::Result<String> {
    match file_name {
        "-" => std::io::read_to_string(std::io::stdin()),
        _ => read_file(file_name),
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
    if openapiv3_filter_core::yaml::parse_value(input).is_ok() {
        return Some("yaml");
    }
    None
}

/// Decides whether the filtered result should be printed as YAML or JSON, echoing the input
/// format back (by file extension when a real file was given, or by sniffing the content when
/// reading from stdin). This is presentation logic, not parsing logic: `core::parser::parse`
/// returns a plain `Value` with no notion of "the format this came from", so the CLI decides how
/// to print it independently, using the same file name/content it used to acquire the input.
///
/// # Arguments
///
/// * `file_name` - The input file name, or "-" for stdin.
/// * `input` - The raw document content (used only when reading from stdin).
///
/// # Returns
///
/// * `OutputFormat` - Whether to print the result as YAML or JSON.
fn output_format(file_name: &str, input: &str) -> OutputFormat {
    let file_postfix = if file_name != "-" {
        file_name.split(".").last()
    } else {
        detect_format(input)
    };
    match file_postfix {
        Some("yml") => OutputFormat::Yaml,
        Some("yaml") => OutputFormat::Yaml,
        Some("json") => OutputFormat::Json,
        _ => OutputFormat::Yaml,
    }
}

fn main() -> ExitCode {
    // Use our custom parse_args instead of the default parse()
    let opts = Opts::parse_args().expect("Argument parsing failed");

    let Opts {
        api_document,
        path_names,
        webhook_names,
        http_methods,
        tags,
        security,
    } = opts;
    let file_name = api_document.expect("Could not parse input document paremeter");

    let result: Result<(), Box<dyn std::error::Error>> = (|| -> Result<(), Box<dyn std::error::Error>> {
        let contents = read_input(&file_name)?;
        let document = openapiv3_filter_core::parser::parse(&contents)?;
        let filtered = document.filter_by_parameters(FilteringParameters {
            paths: path_names,
            webhooks: webhook_names,
            methods: http_methods,
            tags,
            security,
        });
        match output_format(&file_name, &contents) {
            OutputFormat::Json => {
                let text_res = serde_json::to_string(&filtered).unwrap();
                println!("{}", text_res);
            }
            OutputFormat::Yaml => {
                let text_res = openapiv3_filter_core::yaml::value_to_string(&filtered).unwrap();
                println!("{}", text_res);
            }
        }
        Ok(())
    })();

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            println!("{}", error);
            ExitCode::FAILURE
        }
    }
}
