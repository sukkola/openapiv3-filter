## ADDED Requirements

### Requirement: Accept OpenAPI 3.0.x and 3.1.x documents
The system SHALL detect the OpenAPI version from the document's top-level `openapi` field and
SHALL successfully parse any document whose `openapi` value's major.minor is `3.0` or `3.1`,
regardless of patch version, on a single parsing code path (no separate binary/code path per
version).

#### Scenario: Valid 3.0.x document parses
- **WHEN** the input document has `openapi: 3.0.3`
- **THEN** the document is parsed successfully and is available for filtering

#### Scenario: Valid 3.1.x document parses
- **WHEN** the input document has `openapi: 3.1.0`, including a top-level `webhooks` map and a
  schema using `type: [string, "null"]`
- **THEN** the document is parsed successfully and is available for filtering

#### Scenario: Unknown patch version within a supported minor still parses
- **WHEN** the input document has `openapi: 3.1.7` (a patch version not explicitly enumerated
  anywhere in the code)
- **THEN** the document is parsed successfully, since only major.minor is validated

### Requirement: Reject unsupported OpenAPI versions with a clear error
The system SHALL reject documents whose `openapi` field is missing, unparseable, or declares a
major.minor other than `3.0` or `3.1`, returning an error rather than attempting to filter a
partially-understood document.

#### Scenario: Missing openapi field
- **WHEN** the input document has no top-level `openapi` field
- **THEN** the system reports a parsing error and does not produce filtered output

#### Scenario: Unsupported major version
- **WHEN** the input document has `openapi: 2.0` (Swagger) or `openapi: 4.0.0`
- **THEN** the system reports a clear version-related error and does not produce filtered output

### Requirement: Preserve version and version-neutral fields verbatim
The system SHALL preserve the original `openapi` version string and any fields the filtering engine
does not actively reason about (e.g. `info`, `servers`, `externalDocs`, `jsonSchemaDialect`, `x-*`
vendor extensions at any level) unchanged in the filtered output, for both supported versions.

#### Scenario: Version string unchanged after filtering
- **WHEN** a 3.1.0 document is filtered by `--method get`
- **THEN** the filtered output's `openapi` field is still `3.1.0`

#### Scenario: Vendor extension survives filtering
- **WHEN** a document has a top-level `x-internal-id` extension field
- **THEN** that field is present, unchanged, in the filtered output
