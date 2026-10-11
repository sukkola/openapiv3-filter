# Traceability: spec scenarios -> tests

Maps every requirement/scenario in `specs/openapi-version-compatibility/spec.md`,
`specs/full-structural-filtering/spec.md`, and `specs/webhook-filtering/spec.md` to the test(s)
that exercise it (task 6.1).

## openapi-version-compatibility

| Scenario | Test |
| --- | --- |
| Valid 3.0.x document parses | `parser::tests::it_accepts_3_0_patch_versions` |
| Valid 3.1.x document parses (webhooks + `type: [string, "null"]`) | `parser::tests::it_accepts_3_1_patch_versions`; `file_filtering_test::it_filters_openapiv3_1_files` (end-to-end CLI parse of a 3.1 document with `webhooks` and the union-type schema) |
| Unknown patch version within a supported minor still parses | `parser::tests::it_accepts_unenumerated_patch_versions` |
| Missing openapi field | `parser::tests::it_rejects_missing_version_field`; `file_filtering_test::it_reports_parsing_errors` (end-to-end) |
| Unsupported major version | `parser::tests::it_rejects_swagger_2_0`; `parser::tests::it_rejects_future_major_version` |
| Version string unchanged after filtering | `filter::openapi::tests::it_preserves_the_openapi_version_string_after_filtering` |
| Vendor extension survives filtering | `filter::openapi::tests::it_preserves_vendor_extensions_at_any_level` |

## full-structural-filtering

| Scenario | Test |
| --- | --- |
| Reused schema is kept (transitively, Pet -> Category) | `filter::openapi::tests::it_filters_petstore_with_full_path` and siblings (petstore fixture) |
| Unreferenced component is dropped | `filter::openapi::tests::it_keeps_full_structural_coverage_with_no_filters_3_0` (`Unused` absent) |
| Reusable request body is kept | `filter::openapi::tests::it_filters_paths_with_tag_name_match` and siblings (`user-reference.yaml`, `UserCreateRequest`) |
| Reusable header is kept | `filter::openapi::tests::it_keeps_full_structural_coverage_with_no_filters_3_0` (`RateLimit` header) |
| Reusable example is kept | `filter::openapi::tests::it_keeps_full_structural_coverage_with_no_filters_3_0` (`ItemExample`) |
| Operation callback with its own nested $ref is kept | `filter::openapi::tests::it_keeps_full_structural_coverage_with_no_filters_3_0` (`ReusableCallback` + transitively-referenced `ItemChangeEvent`) |
| Operation link to another operation is preserved | `filter::openapi::tests::it_keeps_full_structural_coverage_with_no_filters_3_0` (`links.GetItemById`) |
| Referenced reusable path item is kept (`components.pathItems`) | `filter::openapi::tests::it_keeps_full_structural_coverage_with_no_filters_3_1` (`CommonCrud` + `CommonItem`) |
| Discriminator survives on a kept schema, mapping targets not force-included | `filter::openapi::tests::it_keeps_full_structural_coverage_with_no_filters_3_0` (`Item.discriminator.mapping.dog` unchanged, `Dog` schema absent) |

## webhook-filtering

| Scenario | Test |
| --- | --- |
| Method filter matches a webhook operation | `filter::openapi::tests::it_filters_webhooks_by_method_affecting_only_webhooks` |
| Tag filter excludes a non-matching webhook | `filter::openapi::tests::it_tag_filter_affects_both_a_path_and_a_webhook` (`userDeleted` excluded) |
| No filters keeps all webhooks | `filter::openapi::tests::it_keeps_full_structural_coverage_with_no_filters_3_1` |
| Exact webhook name match | `filter::openapi::tests::it_filters_webhooks_by_name`; `file_filtering_test::it_filters_webhooks_by_name_flag` (end-to-end `--webhook`) |
| Wildcard webhook name match | `filter::openapi::tests::it_matches_webhooks_by_wildcard_name` |
| `--path` does not affect webhooks | `filter::openapi::tests::it_path_filter_does_not_affect_webhooks` |
| Webhook request body schema is kept | `filter::openapi::tests::it_filters_webhooks_by_name` (`UserEvent` present in output, see accepted snapshot) |

(Implied direction, not separately stated as a scenario but covered anyway): `--webhook` does not
affect `paths` -> `filter::openapi::tests::it_webhook_filter_does_not_affect_paths`; tag filter
affecting both a path and a webhook simultaneously ->
`filter::openapi::tests::it_tag_filter_affects_both_a_path_and_a_webhook`.

## Additional robustness coverage (not spec scenarios, closes error-path gaps for task 6.3)

- Dangling/malformed `$ref` does not panic and is passed through unresolved:
  `filter::openapi::tests::it_does_not_panic_on_a_dangling_ref`.
- I/O error on a missing input file: `file_filtering_test::it_reports_io_errors`.
- Regression guard that filtered 3.0 output still round-trips through the (now dev-only)
  `openapiv3` crate: `openapi_roundtrip_test::filtered_3_0_output_round_trips_through_openapiv3`.
- Literal `.yml` file extension (as opposed to `.yaml`/`.json`):
  `filter::openapi::tests::it_loads_a_dot_yml_extensioned_file_as_yaml`;
  `file_filtering_test::it_filters_a_dot_yml_extensioned_file` (end-to-end).
- Both YAML and JSON input for both full-coverage fixtures (3.0 and 3.1):
  `filter::openapi::tests::it_loads_json_input_for_full_coverage_3_0`;
  `filter::openapi::tests::it_loads_json_input_for_full_coverage_3_1`
  (`tests/resources/full-coverage-{3.0,3.1}.{yaml,json}`).
- Deep multi-hop `$ref` chains (5 levels), a `oneOf`/`allOf` branch, and a mutual A<->B cycle, in
  both YAML and JSON (`tests/resources/deep-ref-chain.{yaml,json}`):
  `filter::openapi::tests::it_follows_a_deep_ref_chain_including_oneof_and_allof_branches_and_a_cycle`;
  `filter::openapi::tests::it_follows_a_deep_ref_chain_from_json_input`;
  `filter::openapi::tests::it_prunes_the_unreached_branch_of_a_deep_ref_chain_when_filtered`;
  `file_filtering_test::it_follows_a_deep_ref_chain_from_a_json_file` (end-to-end).
  This fixture caught two real bugs in the reference-closure engine (both pre-existing, inherited
  unchanged from the original typed-struct implementation, just never exercised by any fixture
  before): (1) `found_refs_to_components` pre-filtered the reference graph down to only
  already-known-referenced keys before walking it, so it could never discover a node reachable
  only through an intermediate hop (A -> B -> C would find B but never C); (2) the array branch of
  `collect_component_refs` always appended the array index to the path, so a `$ref` inside a
  `oneOf`/`anyOf`/`allOf` array got attributed to a path like `.../Level5/0` instead of the
  enclosing component's own key `.../Level5`, making it unreachable from the closure walk. Both
  are fixed in `src/filter/openapi.rs::found_refs_to_components` and
  `src/filter/content/reference_collector.rs::collect_component_refs`.
