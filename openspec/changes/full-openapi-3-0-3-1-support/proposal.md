## Why

`openapiv3-filter` only accepts OpenAPI 3.0.x input today, because parsing is done by deserializing
straight into the `openapiv3` crate's `OpenAPI` struct, and that struct models the 3.0.x shape only
(e.g. it has no `webhooks` field, and its `Type`/`Schema` model rejects 3.1-only JSON Schema
constructs such as `type: [string, "null"]`). Confirmed locally: feeding a minimal 3.1.0 document
through the current parser fails to produce any output. Separately, even for 3.0 input, filtering
behavior is pinned to the specific struct fields the crate's filter code enumerates by hand (HTTP
methods on `PathItem`, a fixed walk of `components`), so any OpenAPI structural element the code
doesn't explicitly name is either silently dropped or passed through unfiltered rather than
correctly included/excluded by the user's filters. Users need the tool to (a) accept both OpenAPI
3.0.x and 3.1.x documents and (b) correctly filter the *entire* structural surface of an OpenAPI
document, not just the subset the current code happens to enumerate.

## What Changes

- **BREAKING (internal architecture)**: replace typed-struct-driven parsing/filtering
  (`openapiv3::OpenAPI`) with a version-agnostic internal document model built on an
  order-preserving JSON value tree (`serde_json::Value` with `preserve_order`, or an equivalent
  `IndexMap`-backed value type). The document is no longer deserialized into a crate-specific
  struct; structural elements are addressed generically by key/path, so the engine is not limited
  to a fixed list of fields a crate author chose to model.
  - Rationale (per project rule): OpenAPI 3.0 and 3.1 operation-level shapes (`tags`, `security`,
    `parameters`, `requestBody`, `responses`, `callbacks`) are identical; the differences are
    confined to a handful of top-level fields (`webhooks`, `components.pathItems`,
    `jsonSchemaDialect`) and to the `Schema`/JSON Schema dialect. A single generic value-tree model
    handles both without parallel code paths; only version *detection* (reading `openapi:`) and a
    couple of top-level container names (`paths` + `webhooks`, `components.schemas` +
    `components.pathItems`) need to branch on version.
  - Dependency decision: drop the hard dependency on `openapiv3` for parsing/filtering (its typed
    model is what blocks 3.1). It may still be used narrowly for CLI-level validation/diagnostics
    of 3.0 documents if useful, but is not load-bearing for the filter pipeline.
- Accept and correctly round-trip the `openapi:` version string for both `3.0.x` and `3.1.x`;
  reject other major/minor versions with a clear error instead of attempting to filter them.
- Generalize the reference-closure engine (today's ad hoc `$ref` walk already operates on
  `serde_json::Value`, so this extends naturally) to traverse and prune **every** component
  category under `components` (schemas, responses, parameters, examples, requestBodies, headers,
  securitySchemes, links, callbacks, and 3.1's `pathItems`), plus operation-level `callbacks` and
  `links`, plus `discriminator`/`externalDocs` references — instead of only the categories the
  current hand-written code happens to touch.
- Add first-class filtering for OpenAPI 3.1 top-level `webhooks` using the same
  method/tag/security filters that already apply to `paths`, plus a new `--webhook <pattern>`
  filter (glob match on webhook name, mirroring `--path`).
- Preserve document fields the filter doesn't actively reason about (e.g. `info`, `servers`,
  `externalDocs`, `jsonSchemaDialect`, vendor extensions `x-*`) verbatim rather than requiring each
  one to be explicitly modeled.

## Capabilities

### New Capabilities
- `openapi-version-compatibility`: detect the OpenAPI version of an input document, accept both
  3.0.x and 3.1.x (reject anything else with a clear error), and parse into a version-agnostic
  internal representation that keeps both versions on one code path.
- `full-structural-filtering`: the generic, version-agnostic reference-closure engine that filters
  and prunes the complete OpenAPI structural surface (every `components.*` category including 3.1
  `pathItems`, operation-level `callbacks`/`links`, `discriminator`/`externalDocs` references) so
  no structural element is silently dropped or left unfiltered because it wasn't individually
  coded for.
- `webhook-filtering`: apply the existing method/tag/security filters to OpenAPI 3.1 `webhooks`,
  plus a new `--webhook <pattern>` CLI filter for webhook names, analogous to `--path`.

### Modified Capabilities
_None — no existing specs to modify (this is the project's first set of capability specs)._

## Impact

- Affected code: `src/parser.rs` (version-agnostic parsing), `src/filter/openapi.rs` (currently
  `openapiv3`-struct-typed filtering orchestration), `src/filter/content/*` (reference
  collection/processing — already partly generic, extended to drive the whole pipeline),
  `src/main.rs` (new `--webhook` CLI flag).
- Dependencies: likely drop `openapiv3` from the filtering hot path (kept only if useful for
  narrow validation); no other new runtime dependencies expected (`serde_json`'s `preserve_order`
  feature, or continued use of `indexmap`, covers order preservation).
- Tests: all existing snapshot tests need re-recording against the new engine; new fixtures/tests
  required for 3.1 documents (`webhooks`, `components.pathItems`, 3.1-only schema shapes) and for
  previously-unfiltered structural elements (`callbacks`, `links`, `examples`, `requestBodies`,
  `headers`). The existing WIP fixtures `tests/resources/test-api-3.0.3.yaml` /
  `test-api-3.1.0.yaml` and the pending `it_filters_openapiv3_1_files` test are the starting point
  and should be folded into this change's test plan.
- Backward compatibility: CLI flags and output shape for existing 3.0 filters stay the same;
  behavior changes only in that more structural elements are now correctly included/pruned by
  existing filters (previously-unfiltered elements may now be removed from output when they
  shouldn't be kept, or kept when they reference something that matches — this is a correctness
  fix, not a new flag).
