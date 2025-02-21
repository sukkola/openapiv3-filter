 # OpenAPI v3 Filter

 This tool filters OpenAPI v3 document contents, keeping only the content and its dependencies that match the provided filters.

 ## Usage

 ```bash
 openapiv3-filter api.yaml [OPTIONS]
 ```

 ### Options

 *   `-h, --help`: Prints help information
 *   `-V, --version`: Prints version information
 *   `-a, --api-document <String>`: Input file or - for stdin (default: -)
 *   `-p, --path <String>`: Full path or partial path with `*` wildcard depicting a match for the rest of the content.

     Examples:

     *   `--path '/pets'` - Exact match
     *   `--path '/pets/*'` - Match all paths under `/pets`
     *   `--path '*/pets'` - Match all paths ending with `/pets`
 *   `-m, --method <String>`: HTTP method name used in the operation mapping.

     Examples:

     *   `--method 'post'` - Matches `post` methods in the API specification
     *   `--method 'post' --method 'get'` - Matches both `post` and `get` methods in the document
 *   `--tag <String>`: Tag name that is matched. Requires fully matched tag names.

     Examples:

     *   `--tag 'user_info'` - Matches `user_info` tags in the document
     *   `--tag 'user_info' --tag 'collection'` - Matches both `user_info` and `collection` tags in the document
 *   `--security <String>`: Security name that is matched. Requires fully matched security names.

     Examples:

     *   `--security 'api_key'` - Matches API document content that uses `api_key` security definitions
     *   `--security 'api_key' --security 'basic_auth'` - Matches both `api_key` and `basic_auth` security definitions in the document

 ### Examples

 ```bash
 # Filter operations with get method
openapiv3-filter api.yaml --method get

 # Filter operations with paths containing 'users'
 openapiv3-filter api.json --path '*users*'

 # Filter operations with multiple tags
 openapiv3-filter api.yaml --tag users --tag auth

 # Combine multiple filters
 openapiv3-filter api.json --path '/api/v1/*' --method post --tag admin
