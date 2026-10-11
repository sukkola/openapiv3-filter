## Context

Today this is a single Cargo package producing one binary (`openapiv3-filter`). The engine
(`src/filter/openapi.rs`, `src/filter/content/*`) already operates purely on `serde_json::Value`
with no CLI/IO coupling — this fell out of the `full-openapi-3-0-3-1-support` rewrite as a side
effect, not a deliberate design, but it means the hard part of "librarizing" is already done. The
one remaining seam is `src/parser.rs::parse_document(file_name: &str)`, which conflates acquiring
bytes (open a file, or read stdin when `file_name == "-"`) with parsing them into a `Value` and
validating the OpenAPI version. `src/main.rs` is cleanly CLI-only (clap, stdin-is-terminal
sniffing, `println!`, `ExitCode`) — confirmed by grep, nothing of that leaks into the engine.

Two small API smells exist at the current trait/function boundary, both easy to fix now and
breaking to fix later: `OpenAPIFilter::filter_by_parameters` returns `Option<Self>` but no code
path returns `None` (every caller unconditionally `.unwrap()`s it); and errors are
`Box<dyn std::error::Error>` everywhere, which is fine for a CLI's `main()` but gives a library
caller nothing to match on.

Empirically verified (throwaway scratch crate, not part of this repo): the full dependency stack —
`serde_json` with `preserve_order`, `serde_yaml` 0.9 (+ its `unsafe-libyaml` backend), `wildmatch`,
`indexmap` — compiles clean for `wasm32-unknown-unknown`, release profile, no errors. `serde_yaml`
reports itself as `0.9.34+deprecated` in the lockfile (archived upstream); not a blocker for this
change, but a standing risk for anyone depending on this crate long-term.

## Goals / Non-Goals

**Goals:**
- A `core` crate containing the parsing + filtering engine, with zero `clap`/`std::fs`/
  `std::io::stdin` dependencies, so it can be depended on by a Rust program, a future WASI CLI, or
  a future browser `wasm-bindgen` module without modification.
- A `cli` crate that is a thin consumer of `core`: argument parsing, file/stdin acquisition, output
  printing, exit codes — unchanged CLI flags, output shape, and binary name.
- Zero behavior change observable from the CLI. The three existing capability specs
  (`openapi-version-compatibility`, `full-structural-filtering`, `webhook-filtering`) continue to
  hold exactly as written; this change doesn't touch what they require, only where the code that
  satisfies them lives.
- A public API surface in `core` that doesn't have to be broken again immediately: a concrete error
  type, and `filter_by_parameters` returning something other than an always-`Some` `Option`.

**Non-Goals:**
- Implementing or publishing any wasm target. This change only ensures the crate boundary is drawn
  in the right place for that to be a small, additive follow-up later.
- Publishing `core` to crates.io. The restructuring makes it possible; doing it is a separate,
  later decision (needs a name-availability check, crates.io metadata, a decision on whether 0.x
  churn is acceptable).
- Replacing `serde_yaml` or otherwise reacting to its deprecation. Noted as a risk, not addressed
  here.
- Rewriting or changing the filtering algorithm itself — it was already corrected in the prior
  change and needs no logic changes, only relocation.

## Decisions

### 1. Workspace layout: `crates/core` + `crates/cli`

```
openapiv3-filter/                  (workspace root; no src/ of its own)
├── Cargo.toml                      [workspace] members = ["crates/core", "crates/cli"]
├── crates/
│   ├── core/
│   │   ├── Cargo.toml              name = "openapiv3-filter-core"  (candidate — see Open Questions)
│   │   └── src/
│   │       ├── lib.rs               pub mod parser; pub mod filter;
│   │       ├── parser.rs            pure parse(contents: &str) -> Result<Value, ParseError>
│   │       └── filter/               (moved verbatim: openapi.rs, content/*)
│   └── cli/
│       ├── Cargo.toml              name = "openapiv3-filter"  (unchanged — Homebrew tap depends on this)
│       │                            [[bin]] name = "openapiv3-filter"
│       └── src/
│           └── main.rs              clap Opts, read_input(file_name) -> String (fs/stdin), println!, ExitCode
├── tests/                           -> crates/cli/tests/ (CLI integration tests, spawn the binary)
└── openspec/                        unchanged location (workspace-level concern, not a crate)
```

**Alternatives considered:**
- *Single package, `clap` behind an optional feature* (Option A from exploration): rejected per
  explicit choice — Cargo's `[dependencies]` apply to every target in a package regardless of
  features used by any one target, so this only *reduces* what a library consumer pulls in if they
  remember `default-features = false`; it doesn't give the library its own version, and it doesn't
  leave a clean slot for a third wasm crate without more feature-flag layering.
- *Keep one package, just move `filter`/`content` under a `pub mod` and ship both lib+bin targets
  from it* (no workspace): rejected for the same reason — `clap` would still be an unconditional
  dependency of anyone pulling in the library, since lib and bin targets in one package share one
  `[dependencies]` table.

### 2. What crosses the boundary vs. what stays CLI-only

| Concern | Lives in |
|---|---|
| Format detection (YAML vs JSON) + OpenAPI version validation | `core` (pure, `&str -> Value`) |
| File path / stdin acquisition, `"-"` convention, is-terminal sniffing | `cli` only |
| Reference-closure filtering engine (`filter/openapi.rs`, `filter/content/*`) | `core`, unchanged |
| `FilteringParameters` struct, `OpenAPIFilter` trait | `core` |
| clap `Opts`, CLI help text, examples | `cli` only |
| Output serialization choice (echo input format back) | Stays a `cli`-level decision — `core::parse` returns a plain `Value`; deciding whether to print it back as YAML or JSON based on the original file's extension/content is presentation logic, not parsing logic, so the existing `ParsedType::{Json,Yaml}` tag is replaced by `core` returning just `Value`, and `cli` keeps its own (currently `wrap_response_type`/`detect_format`) logic to decide how to print the result. |
| `println!`, `ExitCode`, error-to-stdout formatting | `cli` only |

### 3. Error type: a concrete `core::Error` enum

Replace `Box<dyn std::error::Error>` at the `core` boundary with a concrete enum (e.g.
`thiserror`-derived) covering at least: I/O-independent parse failure (bad YAML/JSON) and
unsupported/missing OpenAPI version. `cli`'s `main()` can still box it into
`Box<dyn std::error::Error>` internally if that's convenient for its own signature — the
constraint is only that `core`'s public functions return something consumers can `match` on.

**Alternatives considered:** keep `Box<dyn Error>` in `core` too — rejected because it's the
single biggest ergonomics gap for a library consumer (can't distinguish "bad input" from "wrong
version" programmatically), and because a future wasm-bindgen wrapper will want to map concrete
error variants to structured JS errors rather than stringify an opaque `dyn Error`.

### 4. Fix `filter_by_parameters`'s `Option<Self>`

Confirmed via the current implementation: every path ends in `Some(result)`; there is no `None`
case. Change the signature to return `Self` directly (drop the `Option` entirely). If a genuinely
fallible filtering operation is wanted later (e.g. stricter validation that can reject a document),
that's a new, deliberate `Result<Self, core::Error>` — not resurrecting an `Option` that was never
really optional.

### 5. Wasm alignment (not implemented here, but load-bearing for the boundary)

- `wasm32-wasip1`/`wasip2` (WASI): has a virtualized filesystem and stdin, so `cli`'s existing
  file/stdin-acquisition code would likely need little to no change to also build for this target
  — it's a near-free follow-up once `core`/`cli` are separated, not blocked by anything in this
  design.
- `wasm32-unknown-unknown` (browser, via `wasm-bindgen`): no filesystem/stdin at all. A future
  `crates/wasm` member would depend on `core` only, take document content as a `&str` (passed in
  from JS) and call `core::parse` + `filter_by_parameters` directly — never touching `cli` or its
  file/stdin logic. This is exactly why `core` must not depend on `std::fs`/`std::io::stdin`/
  `clap`: those are the three things that don't exist (or don't make sense) on that target.
  Empirically confirmed the rest of `core`'s dependency stack (`serde_json`, `serde_yaml`,
  `wildmatch`) already compiles clean there.
- This change does not add a `crates/wasm` member, a `wasm-bindgen` dependency, or any
  `#[cfg(target_arch = "wasm32")]` code. It only guarantees `core` has nothing in it that would
  block adding that member later.

## Risks / Trade-offs

- **[Risk]** More ceremony than a single package (two `Cargo.toml`s, a workspace root, CI command
  changes) → **Mitigation**: accepted trade-off per explicit choice of Option B; `[workspace.
  dependencies]` keeps shared dependency versions (`serde`, `serde_json`, `serde_yaml`,
  `wildmatch`) pinned once, not duplicated per-crate.
- **[Risk]** Changing the package name backing the published binary could break the existing
  Homebrew tap (`sukkola/tap/openapiv3-filter`) or any install script that assumes a specific
  package/binary name → **Mitigation**: keep the `cli` crate's package name and `[[bin]] name`
  both as `openapiv3-filter`, unchanged; only the new `core` crate gets a new name.
  `release.yml`/`ci.yml` updates must be checked against the actual tap formula before merging.
- **[Risk]** Moving `tests/` under `crates/cli/tests/` could silently change which fixtures/
  snapshots are discovered (relative-path assumptions in existing tests) → **Mitigation**: audit
  every test's path literals (`tests/resources/...`) during the move; run the full suite and expect
  zero snapshot diffs, not just zero failures.
- **[Risk]** The `openapiv3` dev-dependency round-trip regression test currently spawns the
  *compiled binary* (`assert_cmd::cargo::cargo_bin`) — after the split this is either a `cli`-crate
  test (keeps testing the real end-to-end binary) or could be rewritten as a `core`-crate test
  calling the library function directly (faster, no subprocess, but tests the library instead of
  the shipped binary) → **Mitigation**: decide explicitly in tasks.md rather than let it default
  silently; leaning towards keeping it as a `cli` test since the thing being guarded against (output
  that doesn't round-trip through `openapiv3`) is specifically about the binary's *output*, which is
  a `cli`-level concern by this design's own split.

## Open Questions

- What should the `core` crate be named? Candidates: `openapiv3-filter-core`,
  `openapiv3-filter-engine`, or dropping the `openapiv3` prefix entirely now that it filters 3.0
  *and* 3.1 (e.g. `openapi-filter-core`). The `cli` package name must stay `openapiv3-filter`
  regardless, for the existing Homebrew tap. (Default assumption for tasks: `openapiv3-filter-core`,
  simplest/most discoverable pairing with the existing CLI name — revisit if a crates.io name
  collision is found.)
- Does `core` get published to crates.io as part of this change, or does this change only get the
  workspace structured and leave publishing as a separate, later decision? (Default assumption for
  tasks: structure only; publishing is a follow-up once the API has had a chance to settle.)
- Should the round-trip `openapiv3` regression test move to `core` (direct library call) or stay a
  `cli` end-to-end test (spawns the binary)? (Default assumption for tasks: stays in `cli`, see
  Risks above — revisit if this turns out to meaningfully slow down `cli`'s test suite.)

### Resolutions (implementation)

- **Core crate name**: `openapiv3-filter-core`, as the default assumption. No crates.io
  name-availability check was performed (publishing is out of scope for this change — see below —
  so a collision only matters at actual publish time, and can be resolved by renaming the package
  then).
- **Publishing to crates.io**: not done as part of this change. This change only produces the
  workspace structure (`crates/core` as an independently buildable/testable library crate with no
  `clap`/`fs`/`stdin` deps); publishing it to crates.io remains a separate, later decision.
- **Round-trip `openapiv3` regression test**: stayed in `crates/cli/tests/openapi_roundtrip_test.rs`
  as an end-to-end test spawning the compiled binary, per the default assumption. It adds ~0.01s to
  `cli`'s test run — no meaningful slowdown observed.
