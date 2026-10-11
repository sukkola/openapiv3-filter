## 1. Version-agnostic parsing foundation

- [x] 1.1 Add an `openapi` version check in `src/parser.rs`: read the top-level `openapi` field
      before/alongside full parsing, accept major.minor `3.0` or `3.1` (any patch), and return a
      clear error for missing/unsupported versions.
- [x] 1.2 Switch `parse_document`'s target type from a generic `T: Deserialize` used for
      `openapiv3::OpenAPI` to an order-preserving `serde_json::Value` (enable the `preserve_order`
      feature on `serde_json`, or confirm `serde_yaml`'s YAML->JSON conversion preserves key order)
      so JSON and YAML input converge on one in-memory representation.
- [x] 1.3 Update `src/main.rs` call sites that currently expect `ParsedType<OpenAPI>` to work with
      the new value-tree type.
- [x] 1.4 Add parser-level tests: 3.0.3 document parses; 3.1.0 document with `webhooks` and
      `type: [string, "null"]` parses; missing/garbage `openapi` field errors; unsupported major
      (`2.0`, `4.0.0`) errors.

## 2. Generic reference-closure engine (3.0 parity first)

- [x] 2.1 Design the internal "kept set" representation: a set of JSON-pointer-style paths (e.g.
      `#/components/schemas/Pet`) that are known to be retained, built by a single generic walker
      over the whole document value tree (extending `reference_collector::collect_component_refs`
      to run over the *entire* document, not just `components`).
- [x] 2.2 Implement operation selection (the only version/filter-specific logic): for each entry
      under `paths`, evaluate `--path`/`--method`/`--tag`/`--security` against the operation value
      tree (replacing typed `Operation` matching in `src/filter/openapi.rs`).
- [x] 2.3 Implement the generic closure walk: starting from every kept operation's subtree, follow
      all `$ref` targets transitively (fixed-point iteration) into `components.*` (every category,
      not an enumerated subset), marking each reached entry as kept; unreached entries under
      `components` are dropped.
- [x] 2.4 Re-implement tag and security-scheme pruning (currently
      `found_refs_to_tags`/`filter_securities`) on top of the generic kept-set rather than typed
      `Vec<Tag>`/`IndexMap<_, SecurityScheme>`.
- [x] 2.5 Re-serialize the pruned value tree back to YAML/JSON for output, preserving field order
      and all fields the engine doesn't touch (`info`, `servers`, `externalDocs`,
      `jsonSchemaDialect`, `x-*` extensions).
- [x] 2.6 Remove (or demote to dev-dependency-only, per design Open Questions) the `openapiv3`
      crate from `src/filter/openapi.rs` and `src/filter/content/*`; if kept, isolate it behind a
      test-only round-trip-validation helper, not the runtime filter path.
- [x] 2.7 Re-run the full existing snapshot suite against 3.0 fixtures (`petstore.yaml`,
      `user-reference.yaml/json`, etc.) and re-record snapshots with `cargo insta review`, manually
      diffing each change against the previous accepted snapshot to confirm only intended structure
      differs (e.g. newly-kept `requestBodies`/`headers`/`examples`/`links`/`callbacks` that were
      previously dropped or left unfiltered), not a regression.

## 3. Full structural coverage (components.*, callbacks, links, pathItems, discriminator)

- [x] 3.1 Add fixtures exercising each previously-unhandled component category reachable via
      `$ref` from a kept operation: `requestBodies`, `headers`, `examples`, reusable `links`,
      reusable `callbacks` under `components`.
- [x] 3.2 Add a fixture with an operation-level `callbacks` object whose nested request/response
      schemas reference `components.schemas`; verify the callback content and its transitive
      component dependencies are kept.
- [x] 3.3 Add a fixture with an operation response `links` entry; verify it survives filtering
      unchanged.
- [x] 3.4 Add 3.1-only fixture content for `components.pathItems` referenced via `$ref` from a
      `paths` entry; verify the closure walk pulls in the referenced path item and its own
      transitive component references.
- [x] 3.5 Add a fixture with a schema `discriminator`/`mapping`; verify it is preserved unchanged
      on a kept schema and does not force-include unreferenced mapping targets.
- [x] 3.6 Add snapshot tests pairing each of the above with both an "included" and "excluded by
      filter" case, for both a 3.0.3 and a 3.1.0 fixture where the construct is valid in both
      (everything except `pathItems`, which is 3.1-only).

## 4. Webhook filtering (OpenAPI 3.1)

- [x] 4.1 Extend operation selection (task 2.2) to also walk `webhooks` entries, applying
      `--method`/`--tag`/`--security` with identical matching semantics to `paths` operations.
- [x] 4.2 Add a new `--webhook <pattern>` CLI flag in `src/main.rs`/`FilteringParameters`, matched
      with `wildmatch` against webhook names, independent of `--path`.
- [x] 4.3 Ensure `--path` never matches webhook entries and `--webhook` never matches URL paths
      (explicit test for both directions).
- [x] 4.4 Ensure the generic closure walk (task 2.3) runs over kept `webhooks` subtrees exactly
      like kept `paths` subtrees, so webhook component/callback/link references are included.
- [x] 4.5 Add snapshot tests: filter by `--webhook`, by `--method` affecting only webhooks, by
      `--tag` affecting both a path and a webhook simultaneously, and the "no filters keeps all
      webhooks" default case.

## 5. CLI, docs, and existing WIP cleanup

- [x] 5.1 Update `README.md` with `--webhook` usage and examples, and a note that both OpenAPI 3.0
      and 3.1 input are supported.
- [x] 5.2 Fold the existing WIP fixtures (`tests/resources/test-api-3.0.3.yaml`,
      `tests/resources/test-api-3.1.0.yaml`) and the pending `it_filters_openapiv3_1_files` test
      into the new test suite from section 3/4; delete the stale
      `file_filtering_test__it_filters_openapiv3_1_files.snap.new` once a real passing snapshot is
      recorded.
- [x] 5.3 Run the full test suite (`cargo test`) and `cargo insta review` to confirm zero pending
      snapshots and all tests green.
- [x] 5.4 Run `cargo clippy` and fix any new warnings introduced by the rewrite.

## 6. Full coverage testing

- [x] 6.1 Build a traceability checklist mapping every requirement/scenario in
      `specs/openapi-version-compatibility/spec.md`, `specs/full-structural-filtering/spec.md`,
      and `specs/webhook-filtering/spec.md` to the specific test(s) that exercise it; fill any gap
      with a new test rather than closing the change with an unmapped scenario.
- [x] 6.2 Add `cargo-llvm-cov` (preferred) or `cargo-tarpaulin` to the dev workflow and generate a
      coverage report for `src/parser.rs`, `src/filter/openapi.rs`, and `src/filter/content/*`
      (the rewritten engine), since these are exactly the modules this change touches.
- [x] 6.3 Set and document a minimum line/branch coverage threshold for those modules (e.g. in
      README or a CI config comment) and confirm the rewritten engine meets it; add targeted unit
      tests for any uncovered branch (especially error paths: unsupported version, malformed
      `$ref`, empty `paths`/`webhooks`/`components`).
- [x] 6.4 Wire the coverage check into CI (or document the local command if CI wiring is out of
      scope) so a future change that drops coverage on these modules is caught automatically.
