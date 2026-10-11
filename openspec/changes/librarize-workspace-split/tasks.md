## 1. Workspace scaffolding

- [x] 1.1 Convert the root `Cargo.toml` into a workspace manifest (`[workspace] members =
      ["crates/core", "crates/cli"]`, `resolver = "2"`); move shared dependency version pins
      (`serde`, `serde_json`, `serde_yaml`, `wildmatch`) into `[workspace.dependencies]`.
- [x] 1.2 Create `crates/core/Cargo.toml` (package name per design.md Open Questions, default
      `openapiv3-filter-core`) with no `clap` dependency.
- [x] 1.3 Create `crates/cli/Cargo.toml` with package name `openapiv3-filter` and an explicit
      `[[bin]] name = "openapiv3-filter"`, depending on `crates/core` via a path dependency plus
      `clap`.
- [x] 1.4 Delete the stray, unreferenced `src/mod.rs`.

## 2. Move the engine into `crates/core`

- [x] 2.1 Move `src/filter/openapi.rs` and `src/filter/content/*` into `crates/core/src/filter/`
      verbatim (no logic changes — this code already has zero CLI/IO coupling).
- [x] 2.2 Split `src/parser.rs::parse_document` into a pure `core::parse(contents: &str) ->
      Result<Value, core::Error>` (format detection via YAML-then-JSON fallback + OpenAPI version
      validation, no `std::fs`/`std::io`) living in `crates/core/src/parser.rs`.
- [x] 2.3 Introduce `crates/core`'s concrete error type (e.g. `thiserror`-derived `enum Error`)
      covering at least "malformed input" and "unsupported/missing OpenAPI version"; replace
      `Box<dyn std::error::Error>` at every `core` public function boundary.
- [x] 2.4 Change `OpenAPIFilter::filter_by_parameters`'s return type from `Option<Self>` to `Self`
      (confirmed no path returns `None`); update the trait definition and its single `impl`.
- [x] 2.5 Create `crates/core/src/lib.rs` exposing `pub mod parser;` and `pub mod filter;`, plus
      crate-level rustdoc with a minimal usage example (parse a string, filter it, print the
      result).
- [x] 2.6 Move the engine's existing unit tests (`filter::openapi::tests`, `filter::content::*::
      tests`, `parser::tests`) with their code into `crates/core`; update any test fixture paths
      that assumed the old root-relative `tests/resources/...` layout (decide: fixtures used only by
      core-crate unit tests move to `crates/core/tests/resources/` or stay shared at the workspace
      root — pick one and apply consistently).

## 3. Wire up `crates/cli`

- [x] 3.1 Move `src/main.rs` into `crates/cli/src/main.rs`; replace its `mod filter; mod parser;`
      with `use openapiv3_filter_core::{parser, filter::openapi::{...}};` (adjust to whatever crate
      name 1.2 settled on).
- [x] 3.2 Add the CLI-local file/stdin-acquisition helper (`read_input(file_name: &str) ->
      io::Result<String>`, handling the `"-"` stdin convention) in `crates/cli`, calling
      `core::parser::parse` on the acquired string.
- [x] 3.3 Re-implement the output-format echo behavior (YAML in -> YAML out, JSON in -> JSON out)
      entirely in `crates/cli` using the acquired file name/content, now that `core::parse` returns
      a plain `Value` instead of the `ParsedType::{Json,Yaml}` wrapper.
- [x] 3.4 Move `tests/file_filtering_test.rs`, `tests/openapi_roundtrip_test.rs`,
      `tests/resources/*`, `tests/snapshots/*`, and `src/filter/snapshots/*` (the ones exercised via
      `crates/cli`'s CLI-level tests) into `crates/cli/tests/` and `crates/cli/tests/snapshots/`
      per design.md's decision to keep the round-trip regression test as an end-to-end CLI test.

## 4. CI, release, and tooling updates

- [x] 4.1 Update `.github/workflows/ci.yml`: `cargo clippy --workspace --all-targets`, `cargo build
      -p openapiv3-filter --release` (or `--workspace`), `cargo test --workspace`, and the
      `cargo llvm-cov --summary-only --fail-under-file-lines 90` invocation to target the moved
      engine modules under `crates/core/src/`.
- [x] 4.2 Update `.github/workflows/release.yml` and any packaging/upload steps for the new binary
      output path under the workspace `target/` directory; confirm the produced artifact is still
      named/packaged as `openapiv3-filter`.
- [x] 4.3 Update (or confirm unaffected) the Homebrew tap formula's expectations about the binary
      name and build command.
- [x] 4.4 Re-run `cargo deny check licenses` against the workspace and fix any new warnings from
      the restructuring (not pre-existing ones).

## 5. Documentation

- [x] 5.1 Add a "Using this as a library" section to `README.md`: minimal code example calling
      `core::parser::parse` + `filter_by_parameters`, and a note on the workspace layout
      (`crates/core` vs `crates/cli`).
- [x] 5.2 Update the existing "Development" section's commands (`cargo test`, `cargo llvm-cov`,
      ...) to their workspace-aware forms if they change.
- [x] 5.3 Note in design.md's Open Questions (or resolve, if decided during implementation) whether
      `crates/core` is published to crates.io as part of this change or left for later.

## 6. Verification

- [x] 6.1 Run the full test suite (`cargo test --workspace`) and `cargo insta review` to confirm
      zero pending snapshots and all tests green after the move.
- [x] 6.2 Diff CLI output for a representative set of invocations (covering `--path`, `--webhook`,
      `--method`, `--tag`, `--security`, piped stdin, both YAML and JSON input) between the
      pre-restructuring binary and the post-restructuring binary; confirm byte-for-byte identical
      output, per the `library-api` spec's CLI-unchanged requirement.
- [x] 6.3 Build a traceability checklist mapping every requirement/scenario in
      `specs/library-api/spec.md` to the specific test(s) that exercise it, following the pattern
      established in `openspec/changes/full-openapi-3-0-3-1-support/traceability.md`.
- [x] 6.4 Re-run `cargo llvm-cov --summary-only --fail-under-file-lines 90` against
      `crates/core/src/parser.rs`, `crates/core/src/filter/openapi.rs`, and
      `crates/core/src/filter/content/*`; confirm the gate still passes post-move.
- [x] 6.5 Run `cargo clippy --workspace --all-targets` and fix any new warnings introduced by the
      restructuring (leave pre-existing warnings in untouched code as-is, consistent with prior
      practice).
