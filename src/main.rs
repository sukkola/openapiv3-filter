mod parser;
mod filter;

use clap::Parser;
use openapiv3::OpenAPI;

use crate::filter::openapi::{FilteringParameters, OpenAPIFilter};
use parser::ParsedType;
use std::process::ExitCode;
use std::io::{self, IsTerminal};

#[derive(Parser,Default)]
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
              openapiv3-filter api.json --path '/api/v1/*' --method post --tag admin"
)]

/// Filters document by matching specification paths
struct Opts {
        #[arg(help = "Input file or - for stdin", default_value = "-")]
        api_document: Option<String>,
        ///Matches the path name. Allows * wildcards in matching
        #[arg(short,long = "path",help = "full path or partial path with * wildcard depicting match for rest of the content\n\
            Examples:\n \
            --path '/pets' - Exact match\n \
            --path '/pets/*' - Match all paths under /pets\n \
            --path '*/pets' - Match all paths ending with /pets")]
        path_names: Option<Vec<String>>,
        #[arg(short ='m',long = "method",help = "http method name used in the operation mapping\n \
            Examples:\n \
            --method 'post' - mathches post methods in API specification\n \
            --method 'post' ----method 'get' - Matches both post and get methods in document")]
        http_methods: Option<Vec<String>>,
        #[arg(short,long = "tag",help = "tag name that is matched. Requires fully matched tag names\n \
            Examples:\n \
            --tag 'user_info' - mathches user_info tags in document\n \
            --tag 'user_info' ----method 'collection' - Matches both user_info and collection tags in document",)]
        tags: Option<Vec<String>>,
        #[arg(short,long = "security",help = "security name that is matched. Requires fully matched security names\n \
            Examples:\n \
            --security 'api_key' - mathches API document content that uses api_key security definitions\n \
            --security 'api_key' ----security 'basic_auth' - Matches both api_key and basic_auth security definitions in document",)]
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
                Err(_) => Self { api_document: Some(String::from("-")),..Default::default() }
            }
        } else {
            Self::parse()
        };

        Ok(opts)
    }
}

fn main() -> ExitCode {

    // Use our custom parse_args instead of the default parse()
        let opts = Opts::parse_args().expect("Argument parsing failed");

match opts {
    Opts {
        api_document,
        path_names,
        http_methods,
        tags,
        security
        } =>{
        let document: Result<ParsedType<OpenAPI>,Box<dyn (std::error::Error)>> = parser::parse_document(&api_document.expect("Could not parse input document paremeter"));
        match document {
            Ok(openapi) => {
                    match openapi {
                        ParsedType::JSON(val) => {
                            let res =val.filter_by_parameters(FilteringParameters{
                                paths:(path_names).clone(),
                                methods:(http_methods).clone(),
                                tags:(tags).clone(),
                                security:(security),
                                ..Default::default()
                            });
                            let text_res = serde_json::to_string(&res.unwrap()).unwrap();
                            println!("{}",text_res);
                            ExitCode::SUCCESS
                        },
                        ParsedType:: YAML(val) => {
                            let res =val.filter_by_parameters(FilteringParameters{
                                paths:(path_names).clone(),
                                methods:(http_methods).clone(),
                                tags:(tags).clone(),
                                security:(security),
                                ..Default::default()
                            });
                            let text_res = serde_yaml::to_string(&res.unwrap()).unwrap();
                            println!("{}", text_res);
                            ExitCode::SUCCESS
                        }
                    }

            }
            Err(error) => {
                        println!("{}",error.to_string());
                        ExitCode::FAILURE
            }
        }
    }
}

}
