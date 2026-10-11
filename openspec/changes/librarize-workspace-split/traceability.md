# Traceability: spec scenarios -> tests

Maps every requirement/scenario in `specs/library-api/spec.md` to the test(s) or verification
step that exercises it (task 6.3), following the pattern established in
`openspec/changes/full-openapi-3-0-3-1-support/traceability.md`.

## Core engine is consumable as a standalone library crate

| Scenario | Verification |
| --- | --- |
| Depending on the core crate alone does not pull in clap | Verified manually: `cargo tree -p openapiv3-filter-core` (including dev-dependencies) lists no `clap` entry. Not asserted by an automated test — a dependency-graph shape isn't something a `#[test]` can observe from inside the crate; it's a property of `crates/core/Cargo.toml` (no `clap` dependency declared) checked at implementation time. |
| Core crate builds for a browser wasm target | Verified manually: `cargo build -p openapiv3-filter-core --target wasm32-unknown-unknown --release` succeeds with zero errors. Per design.md's explicit non-goal, no wasm target/crate is scaffolded and no CI step was added for this — it's a one-time dependency-compatibility check, not an ongoing gate. |

## Public parsing API is pure (no I/O)

| Scenario | Verification |
| --- | --- |
| Parsing a string does not touch the filesystem | `parser::tests::it_parses_content_independent_of_any_file_extension` (parses in-memory content via `parser::parse`, independent of any file name/extension); more broadly, `crates/core/src/parser.rs`'s `parse` function has no `std::fs`/`std::io` import at all — file/stdin acquisition lives only in `crates/cli/src/main.rs::read_input`. |

## Public API uses a concrete, matchable error type

| Scenario | Verification |
| --- | --- |
| Caller distinguishes an unsupported version from a malformed document | `parser::tests::it_distinguishes_malformed_input_from_an_unsupported_version` (matches on `Error::UnsupportedVersion` vs `Error::Malformed` directly, no string parsing) |

## Filtering is not spuriously optional

| Scenario | Verification |
| --- | --- |
| Filtering a document returns the filtered value directly | `OpenAPIFilter::filter_by_parameters` returns `Self` (not `Option<Self>`) — every existing `filter::openapi::tests::*` test (50 tests) now consumes the return value directly, with no `.unwrap()`/`.is_some()` needed; this is a compile-time guarantee, not just a runtime-observed one. |

## CLI behavior is unchanged by the restructuring

| Scenario | Verification |
| --- | --- |
| Existing CLI invocations produce identical output | `crates/cli/tests/file_filtering_test.rs` (12 tests) and `crates/cli/tests/openapi_roundtrip_test.rs` (1 test), relocated unchanged and passing against the restructured binary; additionally, manually diffed 10 representative invocations (`--path`, `--webhook`, `--method` + `--tag`, `--security`, bare stdin, piped YAML, piped JSON, a parse-error case, an I/O-error case) between the pre-restructuring and post-restructuring release binaries — stdout, stderr, and exit codes were byte-for-byte identical (sha256-compared) in all 10 cases. |
| The installed binary keeps its name | `crates/cli/Cargo.toml` declares `name = "openapiv3-filter"` and `[[bin]] name = "openapiv3-filter"`; verified `cargo build --release -p openapiv3-filter` produces `target/release/openapiv3-filter` (same path as before the restructuring, since a Cargo workspace shares one `target/` directory across members, keyed by binary name). |

## Incidental test-suite changes from the split (not spec scenarios, noted for completeness)

- `filter::openapi::tests::it_loads_a_dot_yml_extensioned_file_as_yaml` (a core-crate unit test
  asserting the now-removed `ParsedType::Yaml` variant) was removed, since format detection from a
  file extension is now exclusively a `cli`-level concern (`core::parser::parse` has no notion of
  file names at all). The same scenario (a bare `.yml` file is round-tripped as YAML) remains
  covered end-to-end by `file_filtering_test::it_filters_a_dot_yml_extensioned_file`, unchanged. It
  was replaced one-for-one by a new, more accurate core-level test,
  `parser::tests::it_parses_content_independent_of_any_file_extension`, keeping the per-module test
  count stable while testing what `core::parse` actually does (parse content, with no file-name
  awareness) rather than a format-wrapping behavior that no longer exists in `core`.
