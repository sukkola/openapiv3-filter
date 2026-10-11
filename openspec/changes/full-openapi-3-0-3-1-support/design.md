## Context

`openapiv3-filter` currently parses input straight into `openapiv3::OpenAPI` (crate v2.0.0), a
struct that models OpenAPI 3.0.x only. Filtering (`src/filter/openapi.rs`) is written against that
struct: `PathItem` is destructured field-by-field (`get`/`put`/`post`/.../`trace` — no slot for a
3.1 `webhooks` sibling of `paths`), and operations are filtered/cloned via typed `Operation`
manipulation. Underneath that, however, the component-reference engine
(`src/filter/content/reference_collector.rs`, `reference_processor.rs`) already works generically:
it serializes `components` to `serde_json::Value` and walks arbitrary `$ref` occurrences without
knowing component category names. That generic half is the part worth keeping; the typed half is
the part blocking both 3.1 support and full structural coverage.

Confirmed locally (this session): a minimal 3.1.0 fixture with `webhooks`, `type: [string, "null"]`,
and `info.summary` fails to parse into `openapiv3::OpenAPI` and produces empty filtered output —
there is no silent partial-success path to build on; 3.1 input is a hard failure today.

Grep of `src/filter/` confirms zero references to `callbacks`, `webhooks`, `discriminator`, or
`external_docs` by name, and `links` only appears inside snapshot fixture text (API description
prose), never as handled structure. `servers` is passed through only because `OpenAPI { ..self.clone() }`
copies it verbatim, not because it is filtered.

## Goals / Non-Goals

**Goals:**
- Accept OpenAPI 3.0.x and 3.1.x input on one code path, with version detected from the `openapi:`
  field and preserved in output.
- Make the reference-closure engine the single source of truth for what gets kept, driven by
  structure (any `$ref`-bearing node reachable from a kept operation/webhook) rather than by a
  hand-enumerated list of known component categories.
- Bring every structural element currently invisible to filtering under the engine: all
  `components.*` categories (`schemas`, `responses`, `parameters`, `examples`, `requestBodies`,
  `headers`, `securitySchemes`, `links`, `callbacks`), 3.1's `components.pathItems`, operation-level
  `callbacks` and `links`, `discriminator` mappings, and `$ref`s inside `externalDocs`-adjacent or
  vendor-extension content if present.
- Add `webhooks` (3.1) as a first-class, filterable sibling of `paths`.

**Non-Goals:**
- Full JSON Schema 2020-12 *validation* (e.g. verifying `prefixItems`/`const`/`unevaluatedProperties`
  semantics). The tool filters and prunes; it does not need to understand schema keyword semantics
  beyond finding `$ref`.
- OpenAPI 3.1 "webhooks-only" documents with no `paths` at all are supported by construction (both
  containers are optional), but no new document-level validation beyond the existing parse step is
  added.
- Supporting OpenAPI 2.0 (Swagger) or pre-release 3.2 drafts.
- Preserving the `openapiv3` crate as the parse target. It is dropped from the hot path (see
  Decisions); if kept at all it is an optional, non-load-bearing validation pass.

## Decisions

### 1. Internal model: order-preserving `serde_json::Value`, not a per-version typed struct
Parse YAML/JSON input into `serde_json::Value` (via `serde_yaml` → `serde_json::Value` conversion,
or direct `serde_json` parse for JSON input) built with the `preserve_order` feature so object key
order round-trips. Drop the `openapiv3::OpenAPI` deserialization step from the filter pipeline.

**Alternatives considered:**
- *Fork/extend `openapiv3` to add 3.1 fields*: rejected — would require modeling the full JSON
  Schema 2020-12 dialect inside the crate's `Schema` type to avoid deserialize failures, is a much
  larger surface than this tool needs, and still leaves the "only the fields the struct names get
  filtered" problem that caused the original structural-coverage gap.
- *Maintain two typed models (3.0 via `openapiv3`, 3.1 via a hand-rolled or third-party 3.1 struct)
  with a shared trait*: rejected — doubles the filtering code (today's bug class, where an element
  is handled for one version's struct and missed for the other's) and doesn't fix structural
  coverage for either version.
- *Generic value tree* (chosen): operation-level shape is identical across 3.0/3.1, so the only
  version-sensitive logic is (a) which version string is accepted and (b) which top-level/under-
  `components` container names exist (`paths`+`webhooks`, `schemas`+`pathItems`). Both are small,
  explicit branches; everything else — including every current and future `components.*` category —
  is handled once, generically, by the reference-closure walk.

### 2. Reference-closure engine becomes the whole filtering mechanism, not just the components step
Today, `filter_by_parameters` first does typed, path/operation-specific filtering (methods/tags/
security on typed `Operation`), *then* runs the generic `$ref` walk only over what's left under
`components`. The design inverts the ratio of "typed, per-element code" to "generic walk": operation
selection (does this operation match `--path`/`--method`/`--tag`/`--security`/`--webhook`) still
needs explicit logic, because filter semantics are specific to those four concerns — but everything
*downstream* of "this operation is kept" (what components/webhooks/pathItems/links/callbacks it
transitively needs) is one generic walk over the value tree, run uniformly over `paths`, `webhooks`,
and `components` (all categories, including `pathItems`).

**Alternatives considered:**
- *Keep current approach, just add more hand-written cases (webhooks, callbacks, links, ...)*:
  rejected — this is exactly the pattern that produced the current gap, and guarantees the next
  OpenAPI structural addition (e.g. a future spec revision) silently falls through again.

### 3. Version detection and acceptance
Read `document["openapi"]` as a string before any other processing. Accept iff it matches `3.0.x`
or `3.1.x` (regex/semver-prefix check on major.minor only; patch version is not validated against a
known list, since new 3.0.z/3.1.z patch releases should keep working without a code change). Any
other value (missing field, `2.0`, `4.0.0`, garbage) is a parse error returned to the user, not a
silent fallback — this matches the existing "fail with a clear error" behavior for unparseable
input (`it_reports_parsing_errors` test). The detected version is stored only to decide which
top-level container names are legal to look for (`webhooks`/`pathItems` only apply when `3.1.x`);
it is not otherwise branched on.

### 4. Output re-serialization preserves structure the engine didn't touch
Fields the engine has no opinion about (`info`, `servers`, `externalDocs`, `jsonSchemaDialect`,
`x-*` extensions at any level) are carried through unchanged because the whole document is one
value tree with surgical removals (pruned paths/webhooks/unused components), not a
reconstruct-from-typed-fields step. This removes the current need for `..self.clone()` struct-spread
tricks and the risk of a newly-added OpenAPI field being dropped because no struct field models it.

## Risks / Trade-offs

- **[Risk]** Losing `openapiv3`'s compile-time guarantee that output is structurally valid OpenAP
  3.0 → **Mitigation**: add a test-suite-level round-trip check (not necessarily on the hot path)
  that validates representative filtered 3.0 outputs still deserialize via `openapiv3` as a
  regression guard, without making it a runtime dependency of the CLI.
- **[Risk]** Generic `$ref`-following could over-collect in pathological documents (e.g. a schema
  that legitimately references nearly everything) → **Mitigation**: this is existing behavior
  (today's components walk already does full closure), not a new risk introduced by this change.
- **[Risk]** Rewriting the filtering core is a large, behavior-sensitive change touching every
  existing snapshot → **Mitigation**: stage it (see tasks) — land the generic value-tree model and
  reference engine first with 3.0 fixtures and re-recorded snapshots proving parity, *then* add
  3.1/webhooks support on top, so a regression in either step is isolated and bisectable.
- **[Risk]** `--webhook` is a new CLI flag with no precedent in the current flag set →
  **Mitigation**: mirror `--path`'s existing glob/wildcard semantics (`wildmatch`) exactly, so users
  already familiar with `--path` need no new mental model.

## Open Questions

- Should webhooks be included by default when no `--webhook`/`--path` filter is given at all (i.e.
  "no path filter" currently means "all paths"), and should that same "no filter = keep all" rule
  extend to webhooks? (Default assumption for tasks: yes, symmetric with `--path`.)
- Should `openapiv3` be kept as a dev-dependency only (for the regression round-trip check in
  Risks) or dropped entirely? (Default assumption for tasks: keep as a dev-dependency only.)
