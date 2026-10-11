## Why

The filtering engine (`src/filter/openapi.rs` + `src/filter/content/*`) already operates purely on
`serde_json::Value` with zero CLI/IO coupling — confirmed by grep: no `println!`, `std::fs`,
`std::io`, `std::process`, or `clap` outside `src/main.rs` and the file/stdin-reading half of
`src/parser.rs`. That means the tool is one seam away from being usable as a Rust library, not just
a CLI binary: callers who want "filter this OpenAPI document" inside their own Rust programs
currently have to shell out to the compiled binary. Splitting the engine into its own crate removes
that seam and, done correctly (byte-acquisition — file/stdin reads — staying entirely out of the
engine crate), also removes the one real blocker to a future WebAssembly build: the dependency
stack (`serde_json` with `preserve_order`, `serde_yaml`, `wildmatch`) has been empirically confirmed
to compile clean for `wasm32-unknown-unknown`.

We're choosing a Cargo **workspace split** (a `core` library crate with zero `clap`/`fs`/`stdin`
dependencies, plus a `cli` binary crate that depends on it) over a single feature-gated crate.
Cargo cannot exclude a dependency from a library consumer's build within one package — every
`[dependencies]` entry applies to every target in that package — so a feature flag on `clap` would
still require consumers to opt out explicitly, and would permanently couple the library's version
number to the CLI's. A workspace gives each crate its own `Cargo.toml`, dependency set, and
version, and — not incidentally — gives a future wasm crate a clean third slot to join without
touching either of the other two.

## What Changes

- **BREAKING (packaging, not behavior)**: restructure the single package into a Cargo workspace
  with two member crates: `crates/core` (the parsing + filtering engine, no `clap`, no `std::fs`,
  no `std::io::stdin`) and `crates/cli` (the existing `openapiv3-filter` binary: argument parsing,
  stdin/file acquisition, output printing, exit codes). The installed binary name and all CLI flags
  are unchanged.
- Split `src/parser.rs::parse_document` into a pure `core::parse(contents: &str) -> Result<Value,
  ParseError>` (format detection + OpenAPI version validation, no I/O) and a CLI-local
  file/stdin-acquisition helper that stays in `crates/cli`.
- Introduce a concrete public error type (implementing `std::error::Error`) in `core`, replacing
  `Box<dyn std::error::Error>` at the library boundary.
- Fix `OpenAPIFilter::filter_by_parameters`'s return type: it currently returns `Option<Self>` but
  no code path returns `None` (confirmed: `main.rs` unconditionally `.unwrap()`s it). Change it to
  return `Self` directly (or `Result<Self, _>` if we want room for genuine future fallibility —
  settled in design.md).
- Move the filtering engine (`filter/openapi.rs`, `filter/content/*`) into `crates/core` verbatim;
  it needs no logic changes since it already has no CLI/IO coupling.
- Update CI (`.github/workflows/ci.yml`), the release workflow, and `cargo llvm-cov`/`cargo deny`
  invocations for the workspace layout; keep the produced binary named `openapiv3-filter` so the
  existing Homebrew tap keeps working unchanged.
- Add a "Using this as a library" section to `README.md` with a minimal code example.
- Delete the stray, unreferenced `src/mod.rs` (dead leftover, not wired into anything).

## Capabilities

### New Capabilities
- `library-api`: the filtering engine is consumable as a standalone Rust library crate —
  importable without pulling in `clap`, `std::fs`, or `std::io::stdin`; exposes a concrete error
  type and an infallible (or explicitly fallible) `filter_by_parameters`; builds for
  `wasm32-unknown-unknown` as a dependency check, without implementing any wasm target itself.

### Modified Capabilities
_None — this change restructures how the existing capabilities (`openapi-version-compatibility`,
`full-structural-filtering`, `webhook-filtering`) are packaged and consumed internally; it does not
change any of their requirements or CLI-observable behavior. Those specs stand as-is._

## Impact

- Affected code: the entire `src/` tree moves under `crates/core/src/` and `crates/cli/src/`; root
  `Cargo.toml` becomes a workspace manifest; `.github/workflows/ci.yml` and `release.yml` need
  path/command updates (`-p <crate>`, `--workspace` for coverage); `README.md` gains a library-usage
  section.
- Dependencies: `clap` becomes `crates/cli`-only; `openapiv3` (dev-only, used by the round-trip
  regression test) is scoped to whichever crate ends up owning that test (see design.md); shared
  dependency versions (`serde`, `serde_json`, `serde_yaml`, `wildmatch`) are pinned once via
  `[workspace.dependencies]` to avoid drift between the two crates.
- Tests: existing unit tests in `filter/openapi.rs` and `filter/content/*` move with the code into
  `crates/core` unchanged (they already only exercise the public-shaped API). CLI integration tests
  (`tests/file_filtering_test.rs`, snapshots, fixtures) move into `crates/cli` unchanged — they
  spawn the compiled binary and assert on stdout, which doesn't change.
- Backward compatibility: CLI flags, output shape, and the installed binary name are all unchanged.
  This is a packaging/internal-architecture change, not a behavior change — the three existing
  capability specs' requirements continue to hold as-is.
- Not in scope: actually implementing or publishing a wasm target (browser `wasm-bindgen` module or
  a `wasm32-wasip1` CLI build), and not in scope: publishing `core` to crates.io (the restructuring
  makes that possible later; doing it is a separate decision).
