//! Differential tests: the `yaml-lean` layer must agree with the default `serde-saphyr` based
//! implementation. The default implementation is the reference, so a failure here means the lean
//! parser or emitter drifted from it.
//!
//! Needs both implementations compiled in: `cargo test -p openapiv3-filter-core --features yaml-lean`.
//!
//! * parsing: both succeed with the same value *in the same key order*, or both fail.
//! * emitting: byte-identical output.
//!
//! Set `YAML_DIFF_DIR` to a directory of extra `.yaml`/`.yml`/`.json` documents to compare as
//! well (used with the public Stripe and OpenAI specs, which are too large to check in).

#![cfg(all(feature = "yaml-saphyr", feature = "yaml-lean"))]

use openapiv3_filter_core::yaml as engine;
use openapiv3_filter_core::yaml::lean as ours;
use serde_json::{Map, Number, Value};

/// Small deterministic xorshift generator so failures are reproducible without extra crates.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
    fn pick<'a, T>(&mut self, items: &'a [T]) -> &'a T {
        &items[self.below(items.len())]
    }
}

const FRAGMENTS: &[&str] = &[
    "a",
    "b",
    "word",
    "y",
    "n",
    "yes",
    "No",
    "on",
    "OFF",
    "true",
    "False",
    "null",
    "Null",
    "~",
    "0",
    "1",
    "42",
    "-",
    "+",
    "-7",
    "+5",
    "1.5",
    ".5",
    "5.",
    "1e3",
    "1E-2",
    "-1.5e+10",
    ".inf",
    "-.inf",
    ".nan",
    "inf",
    "nan",
    "0x1F",
    "0o17",
    "0b101",
    "1_000",
    "0127",
    "-0127",
    "1e999",
    "9223372036854775807",
    "9223372036854775808",
    "-9223372036854775808",
    "18446744073709551615",
    "18446744073709551616",
    " ",
    "  ",
    ":",
    "#",
    "?",
    "'",
    "\"",
    "\\",
    "\n",
    "\t",
    "é",
    "日本",
    "!",
    "&",
    "*",
    "|",
    ">",
    "[",
    "]",
    "{",
    "}",
    ",",
    "@",
    "`",
    "%",
    "<<",
    "=",
    "---",
    "...",
    "2001-01-01",
    "2001-12-14t21:59:43.10-05:00",
    "12:30:45",
    "1:20",
    "1:20.5",
    "/pets/{id}",
    "application/json",
    "#/components/schemas/Pet",
    "a: b",
    "a #b",
    "- x",
    "key:",
    "\u{7}",
    "\u{feff}",
    "\u{2028}",
    "\u{85}",
    "\r",
];

fn fragment_string(rng: &mut Rng, max_parts: usize) -> String {
    let parts = 1 + rng.below(max_parts);
    (0..parts).map(|_| *rng.pick(FRAGMENTS)).collect()
}

fn long_text(rng: &mut Rng) -> String {
    let words = [
        "alpha", "beta", "gamma", "delta", "epsilon", "x", "lorem", "ipsum",
    ];
    let n = 10 + rng.below(40);
    let mut s = String::new();
    for i in 0..n {
        if i > 0 {
            s.push_str(rng.pick(&[" ", " ", " ", "  ", "   "]));
            if rng.below(30) == 0 {
                s.push('\n');
            }
        }
        s.push_str(rng.pick(&words));
    }
    if rng.below(5) == 0 {
        s.push('\n');
    }
    s
}

fn random_scalar(rng: &mut Rng) -> Value {
    match rng.below(10) {
        0 => Value::Null,
        1 => Value::Bool(rng.below(2) == 0),
        2 => Value::from(rng.next() as i64 >> rng.below(60)),
        3 => Value::from(rng.next() >> rng.below(60)),
        4 => {
            let f = match rng.below(6) {
                0 => (rng.next() as f64) * 1e-9,
                1 => (rng.below(1000) as f64) / 8.0,
                2 => 1e21 * (rng.below(9) + 1) as f64,
                3 => 1.0 / (rng.below(1000) + 1) as f64,
                4 => -(rng.below(100) as f64),
                _ => 1e-7 * (rng.below(9) + 1) as f64,
            };
            Value::Number(Number::from_f64(f).unwrap())
        }
        5 => Value::String(long_text(rng)),
        _ => Value::String(fragment_string(rng, 4)),
    }
}

fn random_value(rng: &mut Rng, depth: usize) -> Value {
    if depth == 0 || rng.below(3) == 0 {
        return random_scalar(rng);
    }
    if rng.below(2) == 0 {
        let n = rng.below(5);
        Value::Array((0..n).map(|_| random_value(rng, depth - 1)).collect())
    } else {
        let n = rng.below(5);
        let mut map = Map::new();
        for _ in 0..n {
            map.insert(fragment_string(rng, 3), random_value(rng, depth - 1));
        }
        Value::Object(map)
    }
}

/// Runs `f` on a thread with a large stack: the reference parser recurses per nesting level and
/// debug builds use a lot of stack per frame.
fn big_stack(f: impl FnOnce() + Send + 'static) {
    std::thread::Builder::new()
        .stack_size(256 * 1024 * 1024)
        .spawn(f)
        .unwrap()
        .join()
        .unwrap_or_else(|e| std::panic::resume_unwind(e));
}

fn ordered(v: &Value) -> String {
    serde_json::to_string(v).unwrap()
}

#[test]
fn emitter_is_byte_identical_to_the_engine_on_random_values() {
    big_stack(|| {
        let mut rng = Rng(0x9E3779B97F4A7C15);
        for case in 0..20_000 {
            let mut value = random_value(&mut rng, 4);
            if rng.below(4) != 0 && !value.is_object() {
                let mut m = Map::new();
                m.insert("openapi".into(), Value::String("3.0.3".into()));
                m.insert("v".into(), value);
                value = Value::Object(m);
            }
            let expected = engine::to_string(&value).unwrap();
            let actual = ours::to_string(&value);
            assert_eq!(
                actual,
                expected,
                "case {case}: emitter output differs for {}",
                ordered(&value)
            );
            // And our output must read back to the same value, in our own parser.
            let back =
                ours::parse(&actual).unwrap_or_else(|e| panic!("case {case}: {e}\n{actual}"));
            assert_eq!(
                ordered(&back),
                ordered(&value),
                "case {case}: round trip\n{actual}"
            );
        }
    });
}

fn compare_parse(doc: &str) -> Result<(), String> {
    let e = engine::from_str::<Value>(doc);
    let o = ours::parse(doc);
    match (e, o) {
        (Ok(e), Ok(o)) => {
            if ordered(&e) == ordered(&o) {
                Ok(())
            } else {
                Err(format!(
                    "values differ\n engine: {}\n ours:   {}",
                    ordered(&e),
                    ordered(&o)
                ))
            }
        }
        (Err(_), Err(_)) => Ok(()),
        // Known difference (see `known_differences_are_the_documented_ones`): a scalar root
        // followed by something that is not a document.
        (Ok(e), Err(o))
            if !e.is_object() && !e.is_array() && o.to_string().contains("document start") =>
        {
            Ok(())
        }
        (Ok(e), Err(o)) => Err(format!(
            "engine accepted ({}) but ours failed: {o}",
            ordered(&e)
        )),
        (Err(e), Ok(o)) => Err(format!(
            "ours accepted ({}) but engine failed: {e}",
            ordered(&o)
        )),
    }
}

fn check(doc: &str, failures: &mut Vec<String>) {
    if let Err(m) = compare_parse(doc) {
        failures.push(format!("--- document: {doc:?}\n{m}"));
    }
}

#[test]
fn parser_agrees_with_the_engine_on_scalar_fuzz() {
    big_stack(|| {
        let mut rng = Rng(0xD1B54A32D192ED03);
        let mut failures = Vec::new();
        for _ in 0..15_000 {
            let s = fragment_string(&mut rng, 3);
            let escaped: String = serde_json::to_string(&s).unwrap();
            for doc in [
                format!("k: {s}\n"),
                format!("- {s}\n"),
                format!("k: [{s}]\n"),
                format!("k: {{a: {s}}}\n"),
                format!("{s}: v\n"),
                format!("k: {escaped}\n"),
                format!("k: '{}'\n", s.replace('\'', "''")),
                format!("k: |\n  {}\n", s.replace('\n', "\n  ")),
                format!("k: !!str {s}\n"),
                format!("k: !!int {s}\n"),
                format!("k: !!float {s}\n"),
                format!("k: !!bool {s}\n"),
                format!("k: !!null {s}\n"),
                format!("k: ! {s}\n"),
                format!("k: !custom {s}\n"),
            ] {
                check(&doc, &mut failures);
            }
        }
        report(failures);
    });
}

fn report(failures: Vec<String>) {
    if !failures.is_empty() {
        let shown: Vec<_> = failures.iter().take(30).cloned().collect();
        panic!(
            "{} mismatches; first {}:\n{}",
            failures.len(),
            shown.len(),
            shown.join("\n")
        );
    }
}

#[test]
fn parser_agrees_with_the_engine_on_structure() {
    big_stack(|| {
        let docs = [
            "",
            "\n",
            "---\n",
            "--- \n...\n",
            "# only a comment\n",
            "null\n",
            "~\n",
            "42\n",
            "text\n",
            "[]\n",
            "{}\n",
            "a: 1\n",
            "a: 1\na: 2\n",
            "a: 1\n---\nb: 2\n",
            "---\na: 1\n...\n",
            "a: 1\n...\n---\nb: 2\n",
            "%YAML 1.2\n---\na: 1\n",
            "%YAML 1.1\n---\na: yes\n",
            "a: &x [1, 2]\nb: *x\n",
            "a: &x {k: v}\nb: *x\nc: *x\n",
            "a: *missing\n",
            "a: &x\n  b: *x\n",
            "&a x: 1\n",
            "? complex\n: v\n",
            "? [a, b]\n: v\n",
            "? {a: 1}\n: v\n",
            "a: &k key\n*k : v\n",
            "base: &b {x: 1}\nd:\n  <<: *b\n  y: 2\n",
            "<<: {a: 1}\n",
            "a:\n- 1\n- 2\n",
            "a:\n  - 1\n  - 2\n",
            "a: [1, 2,]\n",
            "a: {b: 1, c: 2,}\n",
            "a: [1, [2, [3]]]\n",
            "a: !!seq [1]\n",
            "a: !!map {b: 1}\n",
            "a: !!str [1]\n",
            "a: !!str {b: 1}\n",
            "a: !!set {b}\n",
            "a: !!omap [b: 1]\n",
            "200: ok\n404: no\n",
            "1.5: f\n",
            "true: t\nnull: n\n",
            "~: tilde\n",
            ": empty\n",
            "a:\nb:\n",
            "a: \"\"\nb: ''\n",
            "a: |\n  one\n  two\n",
            "a: |-\n  one\n",
            "a: |+\n  one\n\n",
            "a: >\n  one\n  two\n",
            "a: >-\n  one\n\n  two\n",
            "a: |2\n    indented\n",
            "a: \"tab\\there\"\n",
            "a: \"\\u00e9\\x41\\n\"\n",
            "a: 'it''s'\n",
            "a: \"unterminated\n",
            "a: [1, 2\n",
            "a: {b: 1\n",
            "- - - deep\n",
            "a: b: c\n",
            "a: - b\n",
            "\ta: 1\n",
            "a:\n\t- 1\n",
            "key with spaces: v\n",
            "a: 'x' 'y'\n",
            "a: b #comment\nc: d#nocomment\n",
            "a: 0x1G\n",
            "a: 1_000\n",
            "a: 1__0\n",
            "a: _1\n",
            "a: 1e\n",
            "a: 1.2.3\n",
            "a: 0.\n",
            "a: +.5\n",
            "a: -.5e-3\n",
            "a: 0o8\n",
            "a: 0b2\n",
            "a: 0x\n",
            "a: 00\n",
            "a: -0\n",
            "a: +0\n",
            "a: 0\n",
            "openapi: 3.0.3\ninfo:\n  title: t\n  version: 1.0\npaths: {}\n",
            "required: [x, y, n, yes, no, on, off]\n",
            "a: .inf\n",
            "a: .NaN\n",
            "a: 1e400\n",
            "a: -1e400\n",
            "a: 1e-400\n",
            "a: 123456789012345678901234567890\n",
            "a: -123456789012345678901234567890\n",
            "a: 0.1\nb: 1e2\nc: 1E2\nd: 1.0e+2\n",
        ];
        let mut failures = Vec::new();
        for doc in docs {
            check(doc, &mut failures);
        }
        // Depth: containers nested n deep.
        for n in [100, 126, 127, 128, 129, 130, 200] {
            check(
                &format!("{}1{}", "[".repeat(n), "]".repeat(n)),
                &mut failures,
            );
            let mut block = String::new();
            for i in 0..n {
                block.push_str(&" ".repeat(i));
                block.push_str("k:\n");
            }
            block.push_str(&" ".repeat(n));
            block.push_str("v\n");
            check(&block, &mut failures);
        }
        // Alias storms.
        let mut bomb = String::from("a: &a [x, x, x, x, x, x, x, x, x, x]\n");
        for i in 0..8 {
            bomb.push_str(&format!(
                "l{}: &l{} [*{}, *{}, *{}, *{}, *{}, *{}, *{}, *{}, *{}, *{}]\n",
                i + 1,
                i + 1,
                if i == 0 {
                    "a".to_string()
                } else {
                    format!("l{i}")
                },
                if i == 0 {
                    "a".to_string()
                } else {
                    format!("l{i}")
                },
                if i == 0 {
                    "a".to_string()
                } else {
                    format!("l{i}")
                },
                if i == 0 {
                    "a".to_string()
                } else {
                    format!("l{i}")
                },
                if i == 0 {
                    "a".to_string()
                } else {
                    format!("l{i}")
                },
                if i == 0 {
                    "a".to_string()
                } else {
                    format!("l{i}")
                },
                if i == 0 {
                    "a".to_string()
                } else {
                    format!("l{i}")
                },
                if i == 0 {
                    "a".to_string()
                } else {
                    format!("l{i}")
                },
                if i == 0 {
                    "a".to_string()
                } else {
                    format!("l{i}")
                },
                if i == 0 {
                    "a".to_string()
                } else {
                    format!("l{i}")
                },
            ));
        }
        check(&bomb, &mut failures);
        let mut many = String::from("a: &a 1\n");
        for i in 0..300 {
            many.push_str(&format!("k{i}: *a\n"));
        }
        check(&many, &mut failures);
        report(failures);
    });
}

#[test]
fn parser_and_emitter_agree_on_checked_in_fixtures_and_extra_documents() {
    big_stack(|| {
        let mut dirs = vec![std::path::PathBuf::from("tests/resources")];
        if let Ok(dir) = std::env::var("YAML_DIFF_DIR") {
            dirs.push(dir.into());
        }
        let mut failures = Vec::new();
        let mut seen = 0;
        for dir in dirs {
            let Ok(entries) = std::fs::read_dir(&dir) else {
                continue;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                let Ok(text) = std::fs::read_to_string(&path) else {
                    continue;
                };
                seen += 1;
                if let Err(m) = compare_parse(&text) {
                    failures.push(format!("--- {}\n{m}", path.display()));
                    continue;
                }
                if let Ok(value) = engine::from_str::<Value>(&text) {
                    let (e, o) = (engine::to_string(&value).unwrap(), ours::to_string(&value));
                    if e != o {
                        let line = e.lines().zip(o.lines()).position(|(a, b)| a != b);
                        failures.push(format!(
                            "--- {}: emitter output differs (first differing line {line:?})",
                            path.display()
                        ));
                    }
                }
            }
        }
        assert!(
            seen > 0,
            "no fixture documents found (run from crates/core)"
        );
        report(failures);
    });
}

/// Documented differences from the engine (see `src/yaml/lean/NOTICE.md`), asserted so they stay
/// deliberate.
#[test]
fn known_differences_are_the_documented_ones() {
    big_stack(|| {
        // A scalar root followed by content that is not a document is accepted by the engine
        // (it stops reading after the root value); the lean layer reports the syntax error. OpenAPI
        // roots are mappings, where both reject trailing garbage.
        assert!(engine::from_str::<Value>("1E-2\n: v\n").is_ok());
        assert!(ours::parse("1E-2\n: v\n").is_err());
        // `!!binary` scalars are decoded to text by the engine; the lean layer does not support them.
        assert!(engine::from_str::<Value>("k: !!binary aGVsbG8=\n").is_ok());
        assert!(ours::parse("k: !!binary aGVsbG8=\n").is_err());
    });
}
