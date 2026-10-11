## ADDED Requirements

### Requirement: Existing method/tag/security filters apply to webhooks
The system SHALL apply the existing `--method`, `--tag`, and `--security` filters to OpenAPI 3.1
top-level `webhooks` entries using the same matching semantics already used for `paths` operations,
since webhook operation objects share the same shape (`tags`, `security`, `responses`, etc.) as
path operations.

#### Scenario: Method filter matches a webhook operation
- **WHEN** a 3.1 document has `webhooks.userCreated.post` and the user runs with `--method post`
- **THEN** `webhooks.userCreated.post` is present in the filtered output

#### Scenario: Tag filter excludes a non-matching webhook
- **WHEN** a webhook operation has `tags: [internal]` and the user runs with `--tag public`
- **THEN** that webhook operation is excluded from the filtered output

#### Scenario: No filters keeps all webhooks
- **WHEN** the user provides no `--path`, `--webhook`, `--method`, `--tag`, or `--security` filters
- **THEN** all webhook entries are present in the filtered output, unchanged

### Requirement: New --webhook filter matches webhook names
The system SHALL support a `--webhook <pattern>` CLI option that filters top-level `webhooks` entries
by name using the same glob/wildcard matching (`wildmatch`) already used by `--path`, independent of
the `--path` filter (a `--path` pattern SHALL NOT match against webhook names, and `--webhook` SHALL
NOT match against URL paths).

#### Scenario: Exact webhook name match
- **WHEN** the user runs with `--webhook userCreated`
- **THEN** only the `userCreated` webhook entry (subject to any other active filters) is present in
  the output

#### Scenario: Wildcard webhook name match
- **WHEN** the user runs with `--webhook 'user*'`
- **THEN** all webhook entries whose name starts with `user` are present in the output

#### Scenario: --path does not affect webhooks
- **WHEN** the user runs with `--path /pets` and the document has both `paths` and `webhooks`
- **THEN** only matching path operations are filtered by `--path`; all webhook entries remain
  subject only to `--method`/`--tag`/`--security`/`--webhook` filters, not `--path`

### Requirement: Webhook component references participate in reference closure
The system SHALL include, via the generic reference-closure engine, any `components` entries
referenced by a kept webhook operation, with the same completeness guarantees as for `paths`
operations.

#### Scenario: Webhook request body schema is kept
- **WHEN** a kept webhook operation's request body references `#/components/schemas/EventPayload`
- **THEN** `EventPayload` is present in the filtered output's `components.schemas`
