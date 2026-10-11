## ADDED Requirements

### Requirement: Core engine is consumable as a standalone library crate
The system SHALL provide a `core` crate containing the OpenAPI parsing and filtering engine that
can be depended on independently of the CLI, without requiring `clap`, `std::fs`, or
`std::io::stdin` as transitive dependencies of that crate.

#### Scenario: Depending on the core crate alone does not pull in clap
- **WHEN** a Rust program adds only the core crate (not the CLI crate) as a dependency
- **THEN** `clap` does not appear anywhere in that program's resolved dependency graph

#### Scenario: Core crate builds for a browser wasm target
- **WHEN** the core crate is compiled with `--target wasm32-unknown-unknown`
- **THEN** the build succeeds, with no code path in the core crate requiring filesystem or stdin
  access

### Requirement: Public parsing API is pure (no I/O)
The system SHALL expose a core-crate function that parses document content already in memory
(e.g. `&str`) into the internal value representation and validates its OpenAPI version, performing
no file or stdin access itself.

#### Scenario: Parsing a string does not touch the filesystem
- **WHEN** a caller passes OpenAPI document content as a string directly to the core crate's parse
  function
- **THEN** the document is parsed and version-validated without the core crate opening any file or
  reading from stdin

### Requirement: Public API uses a concrete, matchable error type
The system SHALL return a concrete error type implementing `std::error::Error` from the core
crate's public parsing and filtering functions, rather than an opaque `Box<dyn std::error::Error>`.

#### Scenario: Caller distinguishes an unsupported version from a malformed document
- **WHEN** a caller parses a document that fails for an OpenAPI-version reason versus a document
  that fails because it is not valid YAML/JSON
- **THEN** the caller can distinguish the two failure cases by matching on the returned error type,
  without parsing an error message string

### Requirement: Filtering is not spuriously optional
The system SHALL expose the filtering operation with a return type that reflects its actual
fallibility: if no code path can fail, the operation SHALL return the filtered value directly
rather than wrapping it in an `Option` that is always `Some`.

#### Scenario: Filtering a document returns the filtered value directly
- **WHEN** a caller filters a parsed document with any combination of filter parameters, including
  parameters that match nothing
- **THEN** the caller receives the filtered value directly (not wrapped in an `Option`), since
  filtering a structurally valid, already-parsed document cannot fail

### Requirement: CLI behavior is unchanged by the restructuring
The system SHALL preserve the existing CLI's flags, output format, exit codes, and installed binary
name after moving the engine into a separate crate.

#### Scenario: Existing CLI invocations produce identical output
- **WHEN** any existing CLI invocation (e.g. filtering by `--path`, `--webhook`, `--method`,
  `--tag`, `--security`, piped stdin input) is run against the restructured workspace
- **THEN** the output is byte-for-byte identical to the pre-restructuring output for the same input
  and flags

#### Scenario: The installed binary keeps its name
- **WHEN** the workspace is built in release mode
- **THEN** the produced binary is still named `openapiv3-filter`
