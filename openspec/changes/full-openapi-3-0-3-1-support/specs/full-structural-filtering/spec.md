## ADDED Requirements

### Requirement: Generic reference-closure filtering over every component category
The system SHALL compute the set of retained `components` entries by following `$ref` occurrences
transitively from every kept operation/webhook, across every category under `components`
(`schemas`, `responses`, `parameters`, `examples`, `requestBodies`, `headers`, `securitySchemes`,
`links`, `callbacks`, and `pathItems`), without requiring category-specific filtering code — a
component category introduced by a future OpenAPI revision SHALL be handled by the same generic
walk without source changes, as long as it is reachable via `$ref`.

#### Scenario: Reused schema is kept
- **WHEN** a kept operation's response references `#/components/schemas/Pet`, and `Pet` references
  `#/components/schemas/Category`
- **THEN** both `Pet` and `Category` are present in the filtered output's `components.schemas`

#### Scenario: Unreferenced component is dropped
- **WHEN** `#/components/schemas/Unused` is not reachable from any kept operation
- **THEN** `Unused` is absent from the filtered output

#### Scenario: Reusable request body is kept
- **WHEN** a kept operation's `requestBody` is `$ref: '#/components/requestBodies/PetBody'`
- **THEN** `PetBody` is present in the filtered output's `components.requestBodies`, along with any
  schema it in turn references

#### Scenario: Reusable header is kept
- **WHEN** a kept operation's response references `#/components/headers/RateLimit`
- **THEN** `RateLimit` is present in the filtered output's `components.headers`

#### Scenario: Reusable example is kept
- **WHEN** a kept operation's response content references `#/components/examples/PetExample`
- **THEN** `PetExample` is present in the filtered output's `components.examples`

### Requirement: Operation-level callbacks and links are preserved and filtered
The system SHALL retain a kept operation's `callbacks` and `links` objects, and SHALL include, via
the generic reference-closure walk, any `components` entries those callbacks/links reference.

#### Scenario: Operation callback with its own nested $ref is kept
- **WHEN** a kept operation defines `callbacks.onEvent` whose request body schema is
  `$ref: '#/components/schemas/EventPayload'`
- **THEN** the output keeps the operation's `callbacks.onEvent` content and includes
  `EventPayload` in `components.schemas`

#### Scenario: Operation link to another operation is preserved
- **WHEN** a kept operation's response defines a `links` entry
- **THEN** that `links` entry is present unchanged in the output

### Requirement: Reusable path items (components.pathItems) participate in reference closure
The system SHALL treat `components.pathItems` entries referenced via `$ref` the same as any other
component category: reachable entries are kept (including their own transitively-referenced
components), unreachable entries are dropped.

#### Scenario: Referenced reusable path item is kept
- **WHEN** a kept path's item is `$ref: '#/components/pathItems/CommonCrud'`, and `CommonCrud`'s
  operations reference `#/components/schemas/Item`
- **THEN** `CommonCrud` is present in `components.pathItems` and `Item` is present in
  `components.schemas`

### Requirement: Discriminator mappings are preserved
The system SHALL preserve a kept schema's `discriminator` object (including its `mapping`) unchanged
in the filtered output; discriminator mapping targets are not treated as filterable references that
could cause a schema to be dropped, but any schema they point to that is otherwise unreferenced is
not force-included.

#### Scenario: Discriminator survives on a kept schema
- **WHEN** a kept schema has `discriminator: { propertyName: petType, mapping: { dog: '#/components/schemas/Dog' } }`
- **THEN** the filtered output's schema still has that `discriminator` object unchanged
