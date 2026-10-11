 # OpenAPI v3 Filter

This tool filters OpenAPI v3 document contents, keeping only the content and its dependencies that match the provided filters.

Both OpenAPI **3.0.x** and **3.1.x** documents are supported on one code path (version is detected
from the document's `openapi` field). Filtering works generically across the full structural
surface of a document — every `components.*` category (including 3.1's `pathItems`),
operation-level `callbacks` and `links`, and discriminator mappings are all preserved/pruned
correctly, not just the specific fields the tool happens to special-case.

## Installation

### Homebrew (macOS/Linux)

```bash
brew tap sukkola/tap
brew install sukkola/tap/openapiv3-filter
```

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
 *   `-w, --webhook <String>`: Webhook name or partial name with `*` wildcard, matched
     independently of `--path`. Applies to OpenAPI 3.1 `webhooks`; a `--path` pattern never
     matches a webhook name, and `--webhook` never matches a URL path.

     Examples:

     *   `--webhook 'userCreated'` - Exact match
     *   `--webhook 'user*'` - Match all webhooks starting with `user`
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

 # Filter webhooks by name (OpenAPI 3.1)
 openapiv3-filter api.yaml --webhook 'user*'
 ```

## Development

Run the test suite with `cargo test`. Snapshot tests use [`insta`](https://insta.rs/); after an
intentional output change, review and accept with `cargo insta review` (or `cargo insta accept`).

Test coverage for the filtering engine (`src/parser.rs`, `src/filter/openapi.rs`,
`src/filter/content/*`) is measured with
[`cargo-llvm-cov`](https://github.com/taiki-e/cargo-llvm-cov):

```bash
cargo install cargo-llvm-cov
rustup component add llvm-tools-preview
cargo llvm-cov --summary-only
```

CI enforces a minimum of 90% line coverage per file on those modules
(`cargo llvm-cov --summary-only --fail-under-file-lines 90`); at the time of writing, actual
coverage on them ranges from ~94% to 100%.
