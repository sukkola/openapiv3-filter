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

## Using this as a library

The parsing and filtering engine is also published as a standalone library crate,
[`openapiv3-filter-core`](crates/core), with no `clap`/`std::fs`/`std::io::stdin` dependencies —
useful for embedding "filter this OpenAPI document" in another Rust program without shelling out
to the compiled binary.

```rust
use openapiv3_filter_core::filter::openapi::{FilteringParameters, OpenAPIFilter};
use openapiv3_filter_core::parser;

let contents = std::fs::read_to_string("api.yaml")?;
let document = parser::parse(&contents)?;
let filtered = document.filter_by_parameters(FilteringParameters {
    paths: Some(vec![String::from("/pets")]),
    ..Default::default()
});
println!("{}", serde_json::to_string_pretty(&filtered)?);
```

Add it as a path/git dependency, or once published, from crates.io:

```toml
[dependencies]
openapiv3-filter-core = "0.1"
```

YAML is read and written by `serde-saphyr` by default. Size-sensitive builds (notably
`wasm32-unknown-unknown`) can switch to a small in-crate YAML layer, which roughly halves the wasm
module, with `default-features = false, features = ["yaml-lean"]`:

```toml
[dependencies]
openapiv3-filter-core = { version = "0.1", default-features = false, features = ["yaml-lean"] }
```

| feature | YAML implementation | syntax errors | notes |
|---------|---------------------|---------------|-------|
| `yaml-saphyr` (default) | `serde-saphyr` | `Error::Malformed` (JSON error) | used by the CLI |
| `yaml-lean` | in-crate layer on `granit-parser` (`crates/core/src/yaml/lean`) | `Error::MalformedYaml` (parser message and position) | `!!binary` rejected, alias limits approximated; see its `NOTICE.md` |

Both read and write the same documents (checked by differential tests); at least one feature must
be enabled and, because Cargo features are additive, `yaml-lean` wins when both are on. The CLI
always uses the default `yaml-saphyr`.

## Repository layout

This repository is a Cargo workspace with two members:

* [`crates/core`](crates/core) (package `openapiv3-filter-core`) — the OpenAPI parsing and
  reference-closure filtering engine. No CLI/IO dependencies; usable as a library on its own.
* [`crates/cli`](crates/cli) (package and binary `openapiv3-filter`) — the command-line tool
  documented above: argument parsing, file/stdin acquisition, and output printing on top of
  `crates/core`.

## Development

Run the full workspace test suite with `cargo test --workspace`. The lean YAML layer is tested
separately, alone and against the default implementation:

```sh
cargo test -p openapiv3-filter-core --no-default-features --features yaml-lean
cargo test -p openapiv3-filter-core --features yaml-lean --test yaml_differential
# optionally also compare large real-world documents (.yaml/.yml/.json):
YAML_DIFF_DIR=/path/to/specs cargo test -p openapiv3-filter-core --features yaml-lean --release
```
 Snapshot tests use
[`insta`](https://insta.rs/); after an intentional output change, review and accept with
`cargo insta review` (or `cargo insta accept`).

Test coverage for the filtering engine (`crates/core/src/parser.rs`,
`crates/core/src/filter/openapi.rs`, `crates/core/src/filter/content/*`) is measured with
[`cargo-llvm-cov`](https://github.com/taiki-e/cargo-llvm-cov):

```bash
cargo install cargo-llvm-cov
rustup component add llvm-tools-preview
cargo llvm-cov --workspace --summary-only
```

CI enforces a minimum of 90% line coverage per file on those modules
(`cargo llvm-cov --workspace --summary-only --fail-under-file-lines 90`); at the time of writing,
actual coverage on them ranges from ~98% to 100%.
