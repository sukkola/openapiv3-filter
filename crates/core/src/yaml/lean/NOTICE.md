# Lean YAML layer (`yaml-lean`): provenance and differences

`mod.rs` reads YAML into a `serde_json::Value` on top of [`granit-parser`](https://crates.io/crates/granit-parser)
(an optional dependency enabled by `yaml-lean`, licensed MIT OR Apache-2.0, listed in `LICENSE-THIRD-PARTY`). `scalar.rs` and
`emit.rs` are **adapted from [serde-saphyr](https://github.com/bourumir-wyngs/serde-saphyr) 1.3.0**
(MIT OR Apache-2.0, used here under the MIT terms):

| file | adapted from (serde-saphyr 1.3.0) |
|------|-----------------------------------|
| `scalar.rs` | `src/parse_scalars.rs`, `src/de/tags.rs`, the `deserialize_any` scalar rules in `src/de/deserializer.rs` |
| `emit.rs` | `src/ser/quoting.rs`, `src/ser/wrapping.rs`, `serialize_str` / layout rules in `src/ser/serializer.rs`, `src/ser/zmij_format.rs` |

The adaptation drops everything the filter does not need (serde integration, readers, snippet
diagnostics, locations, includes, properties, comments) and keeps the *behaviour* the default `yaml-saphyr`
implementation (`src/yaml/saphyr.rs`) gets from its serde-saphyr configuration: YAML 1.2 core booleans only,
`<<` as an ordinary key, duplicate keys rejected, non-finite floats rejected, and the default implementation's
output styling (quoting, literal/folded block scalars, compact lists). `crates/core/tests/yaml_differential.rs`
runs the default implementation side by side with this one to keep them in agreement.

## Deliberate differences from the default implementation

* Syntax errors are reported with the parser's own message and position (`yaml-saphyr` mode surfaces a
  misleading JSON error such as "expected value at line 1 column 1" for invalid YAML, via `Error::Malformed`;
  here it is `Error::MalformedYaml`).
* `!!binary` scalars are rejected (`yaml-saphyr` base64-decodes them to text).
* A scalar document followed by something that is not a document (`1\n: v`) is a syntax error here;
  `yaml-saphyr` stops reading after the first value. OpenAPI documents are mappings, where both reject it.
* Alias limits are approximated (anchors, aliases, replayed nodes, alias/anchor ratio) at the
  serde-saphyr defaults rather than counted event by event.

## Maintenance

These files are a fork of ~700 lines. To re-sync with a newer serde-saphyr, diff the files in the
table against the new release, bump `serde-saphyr` and run
`cargo test -p openapiv3-filter-core --features yaml-lean --test yaml_differential` (also with
`YAML_DIFF_DIR` pointing at large real-world specs). A divergence from the default implementation
shows up there first. Ownership: this repository (licensed MIT OR Apache-2.0 like the rest of it);
the upstream attribution below must stay with the files.

## serde-saphyr licence text (MIT)

```text
Permission is hereby granted, free of charge, to any
person obtaining a copy of this software and associated
documentation files (the "Software"), to deal in the
Software without restriction, including without
limitation the rights to use, copy, modify, merge,
publish, distribute, sublicense, and/or sell copies of
the Software, and to permit persons to whom the Software
is furnished to do so, subject to the following
conditions:

The above copyright notice and this permission notice
shall be included in all copies or substantial portions
of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF
ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED
TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A
PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT
SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY
CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION
OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR
IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER
DEALINGS IN THE SOFTWARE.
```
